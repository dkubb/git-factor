//! Native source-path query arrangement and exact readback; provider Acts stay inline.
use super::Ctx;
use crate::git_factor::{REAL_ENV, REAL_FS, REAL_IO, REAL_RUNNER};
use crate::test_support::OrAbort as _;
use alloc::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

pub(in crate::git_factor::candidate) struct PathQuery {
    base: String,
    directory: TempDir,
    expected: Vec<u8>,
    tip: String,
}
#[derive(Debug, Eq, PartialEq)]
pub(in crate::git_factor::candidate) struct PathQueryFrame {
    files: BTreeMap<PathBuf, Option<Vec<u8>>>,
    head: String,
    index: Vec<u8>,
    refs: String,
}
impl PathQuery {
    pub(in crate::git_factor::candidate) fn arrange(
        stem: &str,
        content: &[u8],
        newline: bool,
    ) -> Self {
        let directory = TempDir::new().or_abort("native path query repository");
        let root = directory.path();
        native_git(root, &["init", "--quiet", "--initial-branch=main"]);
        native_git(root, &["config", "user.name", "Path Query"]);
        native_git(root, &["config", "user.email", "path@example.com"]);
        let removed = "removed\npath";
        fs::write(root.join(removed), b"old bytes").or_abort("removed source path");
        native_git(root, &["add", "--all"]);
        native_git(
            root,
            &["commit", "--quiet", "--message", "Add original path"],
        );
        let base = native_git(root, &["rev-parse", "HEAD"]);
        let selected = format!(
            "selected {stem}{}",
            if newline { "\npath" } else { " path" }
        );
        fs::remove_file(root.join(removed)).or_abort("source deletion");
        fs::write(root.join(&selected), content).or_abort("selected source bytes");
        native_git(root, &["add", "--all"]);
        native_git(
            root,
            &["commit", "--quiet", "--message", "Change source paths"],
        );
        let tip = native_git(root, &["rev-parse", "HEAD"]);
        native_git(root, &["branch", "protected", &base]);
        native_git(root, &["tag", "protected-tag", &tip]);
        fs::write(root.join("unrelated"), b"user bytes\0\n").or_abort("protected user bytes");
        let mut names = [removed.as_bytes(), selected.as_bytes()];
        names.sort_unstable();
        let mut expected = Vec::new();
        for name in names {
            expected.extend_from_slice(name);
            expected.push(0);
        }
        Self {
            base,
            directory,
            expected,
            tip,
        }
    }
    pub(in crate::git_factor::candidate) fn base(&self) -> &str {
        &self.base
    }
    pub(in crate::git_factor::candidate) fn context(&self) -> Ctx<'_> {
        Ctx {
            cwd: self.directory.path().to_path_buf(),
            env: &REAL_ENV,
            fs: &REAL_FS,
            io: &REAL_IO,
            runner: &REAL_RUNNER,
        }
    }
    pub(in crate::git_factor::candidate) fn expected(&self) -> &[u8] {
        &self.expected
    }
    pub(in crate::git_factor::candidate) fn frame(&self) -> PathQueryFrame {
        let root = self.directory.path();
        let index = fs::read(root.join(".git/index")).or_abort("raw index first");
        let mut files = BTreeMap::new();
        collect_files(root, root, &mut files);
        PathQueryFrame {
            files,
            head: native_git(root, &["rev-parse", "HEAD"]),
            index,
            refs: native_git(root, &["show-ref"]),
        }
    }
    pub(in crate::git_factor::candidate) fn tip(&self) -> &str {
        &self.tip
    }
}
fn collect_files(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
    for item in fs::read_dir(directory).or_abort("native query inventory") {
        let path = item.or_abort("native query entry").path();
        let metadata = fs::symlink_metadata(&path).or_abort("native query metadata");
        assert!(
            !metadata.file_type().is_symlink(),
            "native fixture contains only files and directories"
        );
        let relative = path
            .strip_prefix(root)
            .or_abort("native query path")
            .to_path_buf();
        let content = if metadata.is_dir() {
            None
        } else {
            Some(fs::read(&path).or_abort("native query bytes"))
        };
        assert!(files.insert(relative, content).is_none());
        if metadata.is_dir() {
            collect_files(root, &path, files);
        }
    }
}
fn native_git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .or_abort("native query Git");
    assert!(
        output.status.success(),
        "native Git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .or_abort("native Git text")
        .trim_end_matches('\n')
        .to_owned()
}
