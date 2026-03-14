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
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{self, Command};

    use assert_cmd::assert::OutputAssertExt as _;
    use predicates::prelude::*;
    use tempfile::TempDir;

    use super::support::*;
    const FULL_SHA_LEN: usize = 40;

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

    #[test]
    fn rejects_duplicate_pick_flags_after_sha_normalization() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        let long_sha = "a".repeat(FULL_SHA_LEN);
        let resolved = format!("abc1234{}", "0".repeat(33));
        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                "--quiet".to_owned(),
                format!("{long_sha}^{{commit}}"),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];
        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_editor_with_prefixed_path(
            &["--pick", "abc1234", "--pick", long_sha.as_str()],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                ordered_git_replay: Some(OrderedGitReplayExpectation::from_count_file(
                    count_file,
                    steps.len(),
                )),
                stderr: "sha specified multiple times: abc1234\n".to_owned(),
                todo_content: Some("pick abc1234 first\n".to_owned()),
                ..Default::default()
            },
            prefixed_path,
            &path,
        );
        let _keep_alive = wrap_dir;
    }

    #[test]
    fn rejects_duplicate_edit_flags_after_sha_normalization() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        let long_sha = "a".repeat(FULL_SHA_LEN);
        let resolved = format!("abc1234{}", "0".repeat(33));
        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                "--quiet".to_owned(),
                format!("{long_sha}^{{commit}}"),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];
        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_editor_with_prefixed_path(
            &["--edit", "abc1234", "--edit", long_sha.as_str()],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                ordered_git_replay: Some(OrderedGitReplayExpectation::from_count_file(
                    count_file,
                    steps.len(),
                )),
                stderr: "sha specified multiple times: abc1234\n".to_owned(),
                todo_content: Some("pick abc1234 first\n".to_owned()),
                ..Default::default()
            },
            prefixed_path,
            &path,
        );
        let _keep_alive = wrap_dir;
    }

    #[test]
    fn rejects_duplicate_pick_flags_after_sha_normalization_via_resolved_long_sha() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        let requested = "a".repeat(FULL_SHA_LEN);
        let resolved = format!("abc1234{}", "0".repeat(33));
        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                "--quiet".to_owned(),
                format!("{requested}^{{commit}}"),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];

        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);
        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_editor_with_prefixed_path(
            &["--pick", "abc1234", "--pick", requested.as_str()],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                ordered_git_replay: Some(OrderedGitReplayExpectation::from_count_file(
                    count_file,
                    steps.len(),
                )),
                stderr: "sha specified multiple times: abc1234\n".to_owned(),
                todo_content: Some("pick abc1234 first\n".to_owned()),
                ..Default::default()
            },
            prefixed_path,
            &path,
        );
        let _keep_alive = wrap_dir;
    }

    #[test]
    fn rejects_duplicate_drop_flags_after_sha_normalization() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        let long_sha = "a".repeat(FULL_SHA_LEN);
        let resolved = format!("abc1234{}", "0".repeat(33));
        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                "--quiet".to_owned(),
                format!("{long_sha}^{{commit}}"),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];
        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_editor_with_prefixed_path(
            &["--drop", "abc1234", "--drop", long_sha.as_str()],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                ordered_git_replay: Some(OrderedGitReplayExpectation::from_count_file(
                    count_file,
                    steps.len(),
                )),
                stderr: "sha specified multiple times: abc1234\n".to_owned(),
                todo_content: Some("pick abc1234 first\n".to_owned()),
                ..Default::default()
            },
            prefixed_path,
            &path,
        );
        let _keep_alive = wrap_dir;
    }

    #[test]
    fn rejects_drop_sha_not_present_in_todo() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        run_editor(
            &["--drop", "deadbeef"],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: "sha not present in todo: deadbeef\n".to_owned(),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn rejects_pick_sha_not_present_in_todo() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        run_editor(
            &["--pick", "deadbeef"],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: "sha not present in todo: deadbeef\n".to_owned(),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn rejects_edit_sha_not_present_in_todo() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        run_editor(
            &["--edit", "deadbeef"],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: "sha not present in todo: deadbeef\n".to_owned(),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn rejects_contradictory_action_requests() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        run_editor(
            &["--pick", "abc1234", "--drop", "abc1234"],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: "sha specified multiple times: abc1234\n".to_owned(),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn rejects_contradictory_pick_and_edit_requests() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        run_editor(
            &["--pick", "abc1234", "--edit", "abc1234"],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: "sha specified multiple times: abc1234\n".to_owned(),
                todo_content: Some("pick abc1234 first\n".to_owned()),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn rejects_contradictory_edit_and_drop_requests() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        run_editor(
            &["--edit", "abc1234", "--drop", "abc1234"],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: "sha specified multiple times: abc1234\n".to_owned(),
                todo_content: Some("pick abc1234 first\n".to_owned()),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn rejects_non_hex_40_char_sha_without_rev_parse() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        let requested = format!("{}g", "a".repeat(39));

        run_editor(
            &["--drop", requested.as_str()],
            GitSequenceEditorExpectation {
                code: 2,
                stderr: format!(
                    "error: invalid value '{requested}' for '--drop <SHA>': invalid todo sha token: {requested}\n\nFor more information, try '--help'.\n"
                ),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn resolves_hex40_via_rev_parse_and_maps_to_todo_token() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        let requested = "a".repeat(FULL_SHA_LEN);
        let resolved = format!("abc1234{}", "0".repeat(33));

        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                "--quiet".to_owned(),
                format!("{requested}^{{commit}}"),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];

        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_editor_with_prefixed_path(
            &["--drop", requested.as_str()],
            GitSequenceEditorExpectation {
                ordered_git_replay: Some(OrderedGitReplayExpectation::from_count_file(
                    count_file,
                    steps.len(),
                )),
                todo_content: Some("drop abc1234 first\n".to_owned()),
                ..Default::default()
            },
            prefixed_path,
            &path,
        );
        let _keep_alive = wrap_dir;
    }

    #[test]
    fn resolves_hex40_to_full_sha_token_present_in_todo() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");

        let requested = "a".repeat(FULL_SHA_LEN);
        let resolved = format!("deadbeef{}", "0".repeat(32));
        fs::write(&path, format!("pick {resolved} first\n")).or_abort();

        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                "--quiet".to_owned(),
                format!("{requested}^{{commit}}"),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];

        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_editor_with_prefixed_path(
            &["--drop", requested.as_str()],
            GitSequenceEditorExpectation {
                ordered_git_replay: Some(OrderedGitReplayExpectation::from_count_file(
                    count_file,
                    steps.len(),
                )),
                todo_content: Some(format!("drop {resolved} first\n")),
                ..Default::default()
            },
            prefixed_path,
            &path,
        );
        let _keep_alive = wrap_dir;
    }

    #[test]
    fn resolves_uppercase_hex40_via_rev_parse_and_maps_to_todo_token() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        let requested = "A".repeat(FULL_SHA_LEN);
        let resolved = format!("abc1234{}", "0".repeat(33));

        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                "--quiet".to_owned(),
                format!("{requested}^{{commit}}"),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];

        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);
        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_editor_with_prefixed_path(
            &["--drop", requested.as_str()],
            GitSequenceEditorExpectation {
                ordered_git_replay: Some(OrderedGitReplayExpectation::from_count_file(
                    count_file,
                    steps.len(),
                )),
                todo_content: Some("drop abc1234 first\n".to_owned()),
                ..Default::default()
            },
            prefixed_path,
            &path,
        );
        let _keep_alive = wrap_dir;
    }

    #[test]
    fn reports_error_when_rev_parse_fails() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        let requested = "a".repeat(FULL_SHA_LEN);
        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                "--quiet".to_owned(),
                format!("{requested}^{{commit}}"),
            ],
            stdout: String::new(),
            stderr: "fatal: bad object\n".to_owned(),
            exit_code: 1,
        }];

        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_editor_with_prefixed_path(
            &["--drop", requested.as_str()],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                ordered_git_replay: Some(OrderedGitReplayExpectation::from_count_file(
                    count_file,
                    steps.len(),
                )),
                stderr: "git rev-parse failed (exit 1): fatal: bad object\n".to_owned(),
                todo_content: Some("pick abc1234 first\n".to_owned()),
                ..Default::default()
            },
            prefixed_path,
            &path,
        );
        let _keep_alive = wrap_dir;
    }

    #[test]
    fn reports_error_when_rev_parse_cannot_spawn() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        let no_git_path = TempDir::new().or_abort();
        let prefixed_path = OsString::from(no_git_path.path().as_os_str());
        let requested = "a".repeat(FULL_SHA_LEN);

        run_editor_with_prefixed_path(
            &["--drop", requested.as_str()],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: "failed to run git rev-parse: No such file or directory (os error 2)\n"
                    .to_owned(),
                todo_content: Some("pick abc1234 first\n".to_owned()),
                ..Default::default()
            },
            prefixed_path,
            &path,
        );
    }

    #[test]
    fn rejects_ambiguous_long_sha_resolution_in_todo() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc first\npick abc1234 second\n").or_abort();

        let requested = "a".repeat(FULL_SHA_LEN);
        let resolved = format!("abc1234{}", "0".repeat(33));

        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                "--quiet".to_owned(),
                format!("{requested}^{{commit}}"),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];

        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_editor_with_prefixed_path(
            &["--drop", requested.as_str()],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                ordered_git_replay: Some(OrderedGitReplayExpectation::from_count_file(
                    count_file,
                    steps.len(),
                )),
                stderr: format!("sha is ambiguous in todo: {resolved}\n"),
                todo_content: Some("pick abc first\npick abc1234 second\n".to_owned()),
                ..Default::default()
            },
            prefixed_path,
            &path,
        );
        let _keep_alive = wrap_dir;
    }

    #[test]
    fn warns_when_pick_request_is_idempotent() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        run_editor(
            &["--pick", "abc1234"],
            GitSequenceEditorExpectation {
                stderr: "WARN: abc1234: requested 'pick', but todo already had 'pick'\n".to_owned(),
                todo_content: Some("pick abc1234 first\n".to_owned()),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn warns_when_edit_request_is_idempotent() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "edit abc1234 first\n").or_abort();

        run_editor(
            &["--edit", "abc1234"],
            GitSequenceEditorExpectation {
                stderr: "WARN: abc1234: requested 'edit', but todo already had 'edit'\n".to_owned(),
                todo_content: Some("edit abc1234 first\n".to_owned()),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn warns_when_drop_request_is_idempotent() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "drop abc1234 first\n").or_abort();

        run_editor(
            &["--drop", "abc1234"],
            GitSequenceEditorExpectation {
                stderr: "WARN: abc1234: requested 'drop', but todo already had 'drop'\n".to_owned(),
                todo_content: Some("drop abc1234 first\n".to_owned()),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn warns_when_drop_request_is_idempotent_for_short_action() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "d abc1234 first\n").or_abort();

        run_editor(
            &["--drop", "abc1234"],
            GitSequenceEditorExpectation {
                stderr: "WARN: abc1234: requested 'drop', but todo already had 'drop'\n".to_owned(),
                todo_content: Some("d abc1234 first\n".to_owned()),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn rejects_unsupported_todo_actions() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        let original = "foo abc1234 first\n";
        fs::write(&path, original).or_abort();

        run_editor(
            &["--drop", "abc1234"],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: "unsupported todo action: foo\n".to_owned(),
                todo_content: Some(original.to_owned()),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn rejects_uppercase_todo_action() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        let original = "Pick abc1234 first\n";
        fs::write(&path, original).or_abort();

        run_editor(
            &["--drop", "abc1234"],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: "unsupported todo action: Pick\n".to_owned(),
                todo_content: Some(original.to_owned()),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn rejects_non_hex_todo_sha_token_on_commit_actions() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        let original = "pick not_hex first\n";
        fs::write(&path, original).or_abort();

        run_editor(
            &[],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: "invalid todo sha token: not_hex\n".to_owned(),
                todo_content: Some(original.to_owned()),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn preserves_comments_and_blank_lines() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        let original = "\
# comment

pick abc1234 first # trailing comment
exec echo hi
";
        fs::write(&path, original).or_abort();

        run_editor(
            &["--drop", "abc1234"],
            GitSequenceEditorExpectation {
                todo_content: Some(
                    "\
# comment

drop abc1234 first # trailing comment
exec echo hi
"
                    .to_owned(),
                ),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn accepts_all_supported_non_commit_actions() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        let original = "\
pick abc1234 first
exec echo hi
break
label topic
reset topic
merge -C deadbeef topic
noop
update-ref refs/heads/main
";
        fs::write(&path, original).or_abort();

        run_editor(
            &["--drop", "abc1234"],
            GitSequenceEditorExpectation {
                todo_content: Some(
                    "\
drop abc1234 first
exec echo hi
break
label topic
reset topic
merge -C deadbeef topic
noop
update-ref refs/heads/main
"
                    .to_owned(),
                ),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn accepts_abbreviated_commit_and_non_commit_actions() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        let original = "\
p abc1234 first
x echo hi
b
l topic
t topic
m -C deadbeef topic
u refs/heads/main
";
        fs::write(&path, original).or_abort();

        run_editor(
            &["--drop", "abc1234"],
            GitSequenceEditorExpectation {
                todo_content: Some(
                    "\
drop abc1234 first
x echo hi
b
l topic
t topic
m -C deadbeef topic
u refs/heads/main
"
                    .to_owned(),
                ),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn accepts_indented_comments_and_blank_lines() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        let original = "\
   # comment with indentation

pick abc1234 first
";
        fs::write(&path, original).or_abort();

        run_editor(
            &["--edit", "abc1234"],
            GitSequenceEditorExpectation {
                todo_content: Some(
                    "\
   # comment with indentation

edit abc1234 first
"
                    .to_owned(),
                ),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn preserves_malformed_pick_line_without_sha_when_no_actions_requested() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick\n").or_abort();

        run_editor(
            &[],
            GitSequenceEditorExpectation {
                todo_content: Some("pick\n".to_owned()),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn rejects_resolved_long_sha_when_not_present_in_todo() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        let requested = "a".repeat(FULL_SHA_LEN);
        let resolved = format!("deadbeef{}", "0".repeat(32));

        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                "--quiet".to_owned(),
                format!("{requested}^{{commit}}"),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];

        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_editor_with_prefixed_path(
            &["--drop", requested.as_str()],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                ordered_git_replay: Some(OrderedGitReplayExpectation::from_count_file(
                    count_file,
                    steps.len(),
                )),
                stderr: format!("sha not present in todo: {resolved}\n"),
                todo_content: Some("pick abc1234 first\n".to_owned()),
                ..Default::default()
            },
            prefixed_path,
            &path,
        );
        let _keep_alive = wrap_dir;
    }

    #[test]
    fn reports_error_when_todo_file_is_missing() {
        let dir = TempDir::new().or_abort();
        let path = dir.path().join("missing");

        run_editor(
            &["--drop", "abc1234"],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: "failed to read todo file: No such file or directory (os error 2)\n"
                    .to_owned(),
                todo_exists: false,
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    #[cfg(unix)]
    fn reports_error_when_todo_file_is_not_writable() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = TempDir::new().or_abort();
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").or_abort();

        let mut perms = fs::metadata(dir.path()).or_abort().permissions();
        perms.set_mode(0o555);
        fs::set_permissions(dir.path(), perms).or_abort();

        run_editor(
            &["--drop", "abc1234"],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: format!(
                    "failed to create temporary todo file for {}: Permission denied (os error 13)\n",
                    path.display()
                ),
                todo_content: Some("pick abc1234 first\n".to_owned()),
                ..Default::default()
            },
            &path,
        );
    }
}
