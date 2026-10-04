//! Native private-index consumer worlds; test bodies keep their actual Acts inline.
use super::path_queries::{PathQuery, PathQueryFrame};
use super::{CommitSha, Ctx, TreeHash};
use crate::test_support::OrAbort as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

pub(in crate::git_factor::candidate) struct ConfiguredIndex {
    base: CommitSha,
    paths: PathQuery,
    scratch: TempDir,
    source: CommitSha,
    tree: TreeHash,
}

impl ConfiguredIndex {
    pub(in crate::git_factor::candidate) fn arrange(detached: bool) -> Self {
        let paths = PathQuery::arrange("configured", b"native source bytes", false);
        let base = CommitSha::new(paths.base().to_owned()).or_abort("native base");
        let source = CommitSha::new(paths.tip().to_owned()).or_abort("native source");
        let root = paths.context().cwd;
        if detached {
            native_git(&root, &["checkout", "--quiet", "--detach", source.as_str()]);
        } else {
            native_git(&root, &["reset", "--mixed", base.as_str()]);
            native_git(&root, &["read-tree", source.as_str()]);
        }
        let tree = TreeHash::new(&native_git(
            &root,
            &["rev-parse", &format!("{source}^{{tree}}")],
        ))
        .or_abort("native source tree");
        Self {
            base,
            paths,
            scratch: TempDir::new().or_abort("external private observation scratch"),
            source,
            tree,
        }
    }
    pub(in crate::git_factor::candidate) fn arrange_actor(&self) -> PathBuf {
        let actor = self.scratch.path().join("actor");
        native_git(
            &self.context().cwd,
            &[
                "worktree",
                "add",
                "--quiet",
                "--detach",
                actor.to_str().or_abort("native actor path"),
                self.source.as_str(),
            ],
        );
        actor
    }
    pub(in crate::git_factor::candidate) const fn base(&self) -> &CommitSha {
        &self.base
    }
    pub(in crate::git_factor::candidate) fn context(&self) -> Ctx<'_> {
        self.paths.context()
    }
    pub(in crate::git_factor::candidate) fn enable_split_index(&self) {
        native_git(&self.context().cwd, &["config", "core.splitIndex", "true"]);
    }
    pub(in crate::git_factor::candidate) fn frame(&self) -> PathQueryFrame {
        self.paths.frame()
    }
    pub(in crate::git_factor::candidate) fn scratch(&self) -> &Path {
        self.scratch.path()
    }
    pub(in crate::git_factor::candidate) const fn source(&self) -> &CommitSha {
        &self.source
    }
    pub(in crate::git_factor::candidate) const fn tree(&self) -> &TreeHash {
        &self.tree
    }
}

fn native_git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .or_abort("native private-index arrangement");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .or_abort("native query text")
        .trim_end_matches('\n')
        .to_owned()
}
