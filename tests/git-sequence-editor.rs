#![expect(
    clippy::implicit_return,
    reason = "integration tests favor concise tail expressions"
)]
//! Contract integration tests for `git-sequence-editor`.

#![forbid(unsafe_code)]

#[cfg(test)]
#[path = "support/mod.rs"]
mod support;

#[cfg(test)]
mod tests {
    use std::env;
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{self, Command};

    use assert_cmd::assert::OutputAssertExt as _;
    use predicates::prelude::*;
    use tempfile::TempDir;

    use super::support::*;

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct GitSequenceEditorExpectation {
        args: Vec<OsString>,
        code: i32,
        ordered_git_replay: Option<OrderedGitReplayExpectation>,
        path_env: Option<OsString>,
        stderr: String,
        stdout: String,
        todo_content: Option<String>,
        todo_exists: bool,
        todo_path: PathBuf,
    }

    trait OrAbort<T> {
        fn or_abort(self) -> T;
    }

    impl<T, E> OrAbort<T> for Result<T, E> {
        fn or_abort(self) -> T {
            self.unwrap_or_else(|_| process::abort())
        }
    }

    impl<T> OrAbort<T> for Option<T> {
        fn or_abort(self) -> T {
            self.unwrap_or_else(|| process::abort())
        }
    }

    impl Default for GitSequenceEditorExpectation {
        fn default() -> Self {
            Self {
                args: Vec::new(),
                code: EXIT_OK,
                ordered_git_replay: None,
                path_env: None,
                stderr: String::new(),
                stdout: String::new(),
                todo_content: None,
                todo_exists: true,
                todo_path: PathBuf::new(),
            }
        }
    }

    impl CommandExpectation for GitSequenceEditorExpectation {
        fn expected_code(&self) -> i32 {
            self.code
        }

        fn expected_stderr(&self) -> &str {
            self.stderr.as_str()
        }

        fn expected_stdout(&self) -> &str {
            self.stdout.as_str()
        }
    }

    impl GitSequenceEditorExpectation {
        fn assert(self) {
            let mut command = Command::new(assert_cmd::cargo::cargo_bin!("git-sequence-editor"));
            if let Some(path_env) = self.path_env.as_ref() {
                command.env("PATH", path_env);
            }
            command.args(&self.args).arg(&self.todo_path);
            command
                .assert()
                .code(self.expected_code())
                .stdout(predicate::str::diff(self.expected_stdout().to_owned()))
                .stderr(predicate::str::diff(self.expected_stderr().to_owned()));

            let todo_path = self.todo_path.clone();
            let exists = todo_path.exists();
            assert!(
                exists == self.todo_exists,
                "todo file existence mismatch: expected {}, got {exists}",
                self.todo_exists
            );

            if let Some(expected_todo) = self.todo_content.as_ref() {
                let actual = fs::read_to_string(&todo_path).or_abort();
                assert!(
                    actual == *expected_todo,
                    "todo content mismatch:\nexpected:\n{expected_todo}\nactual:\n{actual}"
                );
            }

            if let Some(ordered_git_replay) = self.ordered_git_replay {
                ordered_git_replay.assert_consumed().or_abort();
            }
        }
    }

    fn run_editor(args: &[&str], expected: GitSequenceEditorExpectation, todo_path: &Path) {
        let mut expectation = expected;
        expectation.args = args.iter().map(OsString::from).collect();
        expectation.todo_path = todo_path.to_path_buf();
        expectation.assert();
    }

    #[test]
    fn help_flag_exits_success_and_writes_stdout() {
        Command::new(assert_cmd::cargo::cargo_bin!("git-sequence-editor"))
            .arg("--help")
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::str::contains("Usage:"))
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn version_flag_exits_success_and_writes_stdout() {
        Command::new(assert_cmd::cargo::cargo_bin!("git-sequence-editor"))
            .arg("--version")
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")))
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn updates_todo_for_drop_and_edit_requests() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");

        let original = "\
pick abc1234 first\n\
pick def5678 second\n\
exec echo hi\n\
";
        fs::write(&path, original).or_abort();

        run_editor(
            &["--drop", "abc1234", "--edit", "def5678"],
            GitSequenceEditorExpectation {
                todo_content: Some(
                    "\
drop abc1234 first\n\
edit def5678 second\n\
exec echo hi\n\
"
                    .to_owned(),
                ),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn rejects_duplicate_drop_flags() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        run_editor(
            &["--drop", "abc1234", "--drop", "abc1234"],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: "duplicate drop sha: abc1234\n".to_owned(),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn rejects_duplicate_pick_flags() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        run_editor(
            &["--pick", "abc1234", "--pick", "abc1234"],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: "duplicate pick sha: abc1234\n".to_owned(),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn rejects_duplicate_edit_flags() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        run_editor(
            &["--edit", "abc1234", "--edit", "abc1234"],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: "duplicate edit sha: abc1234\n".to_owned(),
                ..Default::default()
            },
            &path,
        );
    }
}
