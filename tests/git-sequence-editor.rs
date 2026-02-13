//! Contract integration tests for `git-sequence-editor`.

#![forbid(unsafe_code)]

#[cfg(test)]
#[path = "support/mod.rs"]
mod support;

#[cfg(test)]
mod tests {
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

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
            let mut command = Command::new(git_sequence_editor_bin());
            if let Some(path_env) = self.path_env.as_ref() {
                command.env("PATH", path_env);
            }
            command.args(&self.args).arg(&self.todo_path);
            assert_git_sequence_editor_command(&mut command, self);
        }
    }

    fn assert_git_sequence_editor_command(
        command: &mut Command,
        expected: GitSequenceEditorExpectation,
    ) {
        assert_command_exact(command, &expected);
        let todo_path = expected.todo_path.clone();
        assert_git_sequence_editor_postconditions(&todo_path, expected)
            .expect("git-sequence-editor postcondition assertions should pass");
    }

    fn assert_git_sequence_editor_postconditions(
        todo_path: &Path,
        expected: GitSequenceEditorExpectation,
    ) -> Result<(), String> {
        let exists = todo_path.exists();
        if exists != expected.todo_exists {
            return Err(format!(
                "todo file existence mismatch: expected {}, got {exists}",
                expected.todo_exists
            ));
        }

        if let Some(expected_todo) = expected.todo_content.as_ref() {
            let actual = fs::read_to_string(todo_path)
                .map_err(|err| format!("failed to read todo file for assertion: {err}"))?;
            if actual != *expected_todo {
                return Err(format!(
                    "todo content mismatch:\nexpected:\n{expected_todo}\nactual:\n{actual}"
                ));
            }
        }

        if let Some(ordered_git_replay) = expected.ordered_git_replay {
            ordered_git_replay.assert_consumed()?;
        }

        Ok(())
    }

    fn run_editor(args: &[&str], expected: GitSequenceEditorExpectation, todo_path: &Path) {
        let mut expectation = expected;
        expectation.args = args.iter().map(OsString::from).collect();
        expectation.todo_path = todo_path.to_path_buf();
        expectation.assert();
    }

    fn run_editor_with_prefixed_path(
        args: &[&str],
        expected: GitSequenceEditorExpectation,
        path_env: OsString,
        todo_path: &Path,
    ) {
        let mut expectation = expected;
        expectation.args = args.iter().map(OsString::from).collect();
        expectation.path_env = Some(path_env);
        expectation.todo_path = todo_path.to_path_buf();
        expectation.assert();
    }
