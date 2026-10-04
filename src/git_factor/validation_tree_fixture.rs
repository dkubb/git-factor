use super::*;
use crate::test_support::OrAbort as _;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

pub(in crate::git_factor::validation) struct Fixture {
    directory: TempDir,
    span: CommitSpan,
}
impl Fixture {
    pub(in crate::git_factor::validation) fn context(&self) -> Ctx<'_> {
        Ctx {
            cwd: self.directory.path().to_path_buf(),
            env: &REAL_ENV,
            fs: &REAL_FS,
            io: &REAL_IO,
            runner: &REAL_RUNNER,
        }
    }
    pub(in crate::git_factor::validation) fn directory(&self) -> &Path {
        self.directory.path()
    }
    pub(in crate::git_factor::validation) fn git(&self, args: &[&str]) -> String {
        let output = REAL_RUNNER
            .output("git", args, &[], self.directory())
            .or_abort("native Git arrangement/readback");
        assert!(output.status.success(), "native Git {args:?}: {output:?}");
        let text = String::from_utf8(output.stdout).or_abort("native ASCII readback");
        text.strip_suffix('\n').unwrap_or(&text).to_owned()
    }
    pub(in crate::git_factor::validation) fn new(root: bool, content: Option<&[u8]>) -> Self {
        let directory = TempDir::new().or_abort("tree boundary resources");
        let git = |args: &[&str]| {
            let output = REAL_RUNNER
                .output("git", args, &[], directory.path())
                .or_abort("native Git arrangement");
            assert!(output.status.success(), "native Git {args:?}: {output:?}");
            let text = String::from_utf8(output.stdout).or_abort("native ASCII arrangement");
            text.strip_suffix('\n').unwrap_or(&text).to_owned()
        };
        git(&["init", "--quiet"]);
        git(&["config", "user.name", "Tree boundary"]);
        git(&["config", "user.email", "boundary@example.test"]);
        git(&["config", "commit.gpgSign", "false"]);
        git(&["config", "core.hooksPath", "/dev/null"]);
        if !root {
            fs::write(
                directory.path().join("retained anchor"),
                b"nonempty parent\0\n",
            )
            .or_abort("nonempty parent bytes");
            git(&["add", "--", "retained anchor"]);
            git(&["commit", "--quiet", "-m", "Add base"]);
        }
        if let Some(bytes) = content {
            fs::write(directory.path().join("selected binary"), bytes).or_abort("selected bytes");
            git(&["add", "--", "selected binary"]);
        }
        git(&[
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "Add selected change",
        ]);
        let tip = CommitSha::new(git(&["rev-parse", "HEAD"])).or_abort("actual tip");
        let span = CommitSpan::new(
            NonEmpty::singleton(tip),
            if root {
                BaseParent::Root
            } else {
                BaseParent::Commit
            },
        );
        fs::write(directory.path().join("user"), b"unrelated user bytes")
            .or_abort("protected bytes");
        Self { directory, span }
    }
    pub(in crate::git_factor::validation) const fn span(&self) -> &CommitSpan {
        &self.span
    }
}
