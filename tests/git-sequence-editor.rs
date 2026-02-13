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

    #[test]
    fn updates_todo_for_drop_and_edit_requests() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");

        let original = "\
pick abc1234 first\n\
pick def5678 second\n\
exec echo hi\n\
";
        fs::write(&path, original).expect("write todo file");

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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

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
    fn rejects_duplicate_pick_flags_after_sha_normalization() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

        let long_sha = "a".repeat(40);
        let resolved = format!("abc1234{}", "0".repeat(33));
        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                long_sha.clone(),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];
        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);

        let original_path = env::var_os("PATH").expect("PATH");
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
    fn rejects_duplicate_edit_flags() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

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
    fn rejects_duplicate_edit_flags_after_sha_normalization() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

        let long_sha = "a".repeat(40);
        let resolved = format!("abc1234{}", "0".repeat(33));
        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                long_sha.clone(),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];
        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);

        let original_path = env::var_os("PATH").expect("PATH");
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
    fn rejects_contradictory_action_requests() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

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
    fn rejects_duplicate_pick_flags_after_sha_normalization_via_resolved_long_sha() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

        let requested = "a".repeat(40);
        let resolved = format!("abc1234{}", "0".repeat(33));
        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                requested.clone(),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];

        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);
        let original_path = env::var_os("PATH").expect("PATH");
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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

        let long_sha = "a".repeat(40);
        let resolved = format!("abc1234{}", "0".repeat(33));
        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                long_sha.clone(),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];
        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);

        let original_path = env::var_os("PATH").expect("PATH");
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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

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
    fn rejects_non_hex_40_char_sha_without_rev_parse() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

        let requested = format!("{}g", "a".repeat(39));

        run_editor(
            &["--drop", requested.as_str()],
            GitSequenceEditorExpectation {
                code: EXIT_FAILURE,
                stderr: format!("sha not present in todo: {requested}\n"),
                ..Default::default()
            },
            &path,
        );
    }

    #[test]
    fn resolves_hex40_via_rev_parse_and_maps_to_todo_token() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

        let requested = "a".repeat(40);
        let resolved = format!("abc1234{}", "0".repeat(33));

        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                requested.clone(),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];

        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);

        let original_path = env::var_os("PATH").expect("PATH");
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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");

        let requested = "a".repeat(40);
        let resolved = format!("deadbeef{}", "0".repeat(32));
        fs::write(&path, format!("pick {resolved} first\n")).expect("write todo file");

        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                requested.clone(),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];

        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);

        let original_path = env::var_os("PATH").expect("PATH");
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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

        let requested = "A".repeat(40);
        let resolved = format!("abc1234{}", "0".repeat(33));

        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                requested.clone(),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];

        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);
        let original_path = env::var_os("PATH").expect("PATH");
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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

        let requested = "a".repeat(40);
        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                requested.clone(),
            ],
            stdout: String::new(),
            stderr: "fatal: bad object\n".to_owned(),
            exit_code: 1,
        }];

        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);

        let original_path = env::var_os("PATH").expect("PATH");
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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

        let no_git_path = TempDir::new().expect("tempdir");
        let prefixed_path = OsString::from(no_git_path.path().as_os_str());
        let requested = "a".repeat(40);

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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc first\npick abc1234 second\n").expect("write todo file");

        let requested = "a".repeat(40);
        let resolved = format!("abc1234{}", "0".repeat(33));

        let steps = vec![GitWrapperStep {
            args: vec![
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                requested.clone(),
            ],
            stdout: format!("{resolved}\n"),
            stderr: String::new(),
            exit_code: 0,
        }];

        let (wrap_dir, wrap_bin, count_file) = make_ordered_git_wrapper(&steps);

        let original_path = env::var_os("PATH").expect("PATH");
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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 first\n").expect("write todo file");

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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "edit abc1234 first\n").expect("write todo file");

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
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "drop abc1234 first\n").expect("write todo file");

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
