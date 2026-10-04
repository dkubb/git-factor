mod read_rebase_counter {
    use super::super::{RebaseCounter, log_contracts::LogFixture, read_rebase_counter};
    use crate::test_support::OrAbort as _;
    use std::fs;
    #[test]
    fn preserves_zero_counter_from_a_real_file() {
        let fixture = LogFixture::new(false);
        let path = fixture.directory().join("counter");
        fs::write(&path, " \t0\r\n").or_abort("counter file");
        let actual = read_rebase_counter(&fixture.context(), &path).map(RebaseCounter::as_u32);
        assert_eq!(actual, Some(u32::MIN));
        assert_eq!(fs::read(&path).or_abort("file unchanged"), b" \t0\r\n");
        assert_eq!(fixture.queries(), 0);
    }
}
mod push_snapshot_fields {
    use super::super::{RebaseCounter, RebaseState, RepoSnapshot, push_snapshot_fields};
    use crate::test_support::OrAbort as _;
    #[test]
    fn emits_the_complete_nullable_snapshot_shape() {
        let mut text = "{".to_owned();
        let snapshot = RepoSnapshot::default();
        let expected = serde_json::from_str::<serde_json::Value>(r#"{ "state_head": null, "state_head_tree": null, "state_git_dir": null, "state_toplevel": null,
"state_staged_paths": [], "state_unstaged_paths": [], "state_untracked_paths": [],
"state_factor_checkpoint": null, "state_factor_final_tree": null, "state_factor_phase": null, "state_factor_source": null,
"state_rebase_state": null, "state_rebase_msgnum": null, "state_rebase_end": null, "state_rebase_todo_head": null, "state_rebase_done_tail": null }"#).or_abort("expected JSON");
        push_snapshot_fields(&mut text, "state", &snapshot);
        text.push('}');
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).or_abort("snapshot JSON"),
            expected
        );
    }
    #[test]
    fn preserves_distinct_present_snapshot_sources() {
        let snapshot = RepoSnapshot {
            head: Some("head-id".to_owned()),
            head_tree: Some("tree-id".to_owned()),
            git_dir: Some("admin".to_owned()),
            toplevel: Some("worktree".to_owned()),
            staged_paths: vec!["staged".to_owned()],
            unstaged_paths: vec!["unstaged".to_owned()],
            untracked_paths: vec!["untracked".to_owned()],
            factor_checkpoint: Some("checkpoint-id".to_owned()),
            factor_final_tree: Some("final-tree-id".to_owned()),
            factor_phase: Some("selecting".to_owned()),
            factor_source: Some("source-id".to_owned()),
            rebase_state: Some(RebaseState::Merge),
            rebase_msgnum: Some(RebaseCounter(3)),
            rebase_end: Some(RebaseCounter(4)),
            rebase_todo_head: Some("pending todo".to_owned()),
            rebase_done_tail: Some("completed done".to_owned()),
        };
        let expected = serde_json::from_str::<serde_json::Value>(r#"{"state_head": "head-id", "state_head_tree": "tree-id", "state_git_dir": "admin", "state_toplevel": "worktree", "state_staged_paths": ["staged"], "state_unstaged_paths": ["unstaged"], "state_untracked_paths": ["untracked"], "state_factor_checkpoint": "checkpoint-id", "state_factor_final_tree": "final-tree-id", "state_factor_phase": "selecting", "state_factor_source": "source-id", "state_rebase_state": "rebase-merge", "state_rebase_msgnum": "3", "state_rebase_end": "4", "state_rebase_todo_head": "pending todo", "state_rebase_done_tail": "completed done"}"#).or_abort("independent expected snapshot");
        let mut text = "{".to_owned();
        push_snapshot_fields(&mut text, "state", &snapshot);
        text.push('}');
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).or_abort("snapshot JSON"),
            expected
        );
    }
}
mod trace_note {
    use super::super::{log_contracts::LogFixture, trace_note};
    use crate::test_support::OrAbort as _;
    use std::fs;
    #[test]
    fn records_one_event_with_observed_unavailable_snapshot_and_literal_fields() {
        let fixture = LogFixture::new(true);
        let expected = serde_json::from_str::<serde_json::Value>(r#"{ "event": "factor_cmd_status", "detail": "quoted \"detail\"", "state_head": null, "state_head_tree": null, "state_git_dir": null, "state_toplevel": null,
"state_staged_paths": [], "state_unstaged_paths": [], "state_untracked_paths": [],
"state_factor_checkpoint": null, "state_factor_final_tree": null, "state_factor_phase": null, "state_factor_source": null,
"state_rebase_state": null, "state_rebase_msgnum": null, "state_rebase_end": null, "state_rebase_todo_head": null, "state_rebase_done_tail": null }"#).or_abort("expected JSON");
        trace_note(
            &fixture.context(),
            "factor_cmd_status",
            &[("detail", "quoted \"detail\"")],
        );
        let text = fs::read_to_string(fixture.directory().join("trace")).or_abort("trace");
        let mut actual =
            serde_json::from_str::<serde_json::Value>(&text).or_abort("one trace record");
        assert!(
            actual
                .as_object_mut()
                .or_abort("trace object")
                .remove("ts_unix_ms")
                .and_then(|value| value.as_u64())
                .is_some()
        );
        assert_eq!(actual, expected);
        assert_eq!(fixture.queries(), 4);
        assert_eq!(text.lines().count(), 1);
    }
}
mod trace_process_command {
    use super::super::{
        ProcessTrace, RepoSnapshot, log_contracts::LogFixture, trace_process_command,
    };
    use crate::test_support::OrAbort as _;
    use std::fs;
    #[test]
    fn records_exact_child_fields_without_running_native_queries() {
        let fixture = LogFixture::new(true);
        let before = RepoSnapshot {
            head: Some("before-head".to_owned()),
            head_tree: Some("before-tree".to_owned()),
            staged_paths: vec!["before-staged".to_owned()],
            ..RepoSnapshot::default()
        };
        let after = RepoSnapshot {
            head: Some("after-head".to_owned()),
            head_tree: Some("after-tree".to_owned()),
            staged_paths: vec!["after-staged".to_owned()],
            ..RepoSnapshot::default()
        };
        let trace = ProcessTrace {
            mode: "output",
            bin: "git",
            args: &["query"],
            envs: &[("REMOVE", None), ("EMPTY", Some(""))],
            quiet: false,
            spawned: false,
            duration_ms: 17,
            exit_code: None,
            stdout: None,
            stderr: Some("spawn failure"),
            before: &before,
            after: &after,
        };
        let mut expected = serde_json::from_str::<serde_json::Value>(r#"{ "event": "process", "mode": "output", "bin": "git", "args": ["query"], "env": ["REMOVE", "EMPTY="], "quiet": false, "spawned": false, "duration_ms": 17, "exit_code": null, "stdout": null, "stderr": "spawn failure" }"#).or_abort("expected JSON");
        let expected_fields = expected.as_object_mut().or_abort("expected process object");
        expected_fields.extend(serde_json::from_str::<serde_json::Value>(r#"{ "before_head": "before-head", "before_head_tree": "before-tree", "before_git_dir": null, "before_toplevel": null,
"before_staged_paths": ["before-staged"], "before_unstaged_paths": [], "before_untracked_paths": [],
"before_factor_checkpoint": null, "before_factor_final_tree": null, "before_factor_phase": null, "before_factor_source": null,
"before_rebase_state": null, "before_rebase_msgnum": null, "before_rebase_end": null, "before_rebase_todo_head": null, "before_rebase_done_tail": null }"#).or_abort("expected JSON").as_object().or_abort("expected before snapshot").clone());
        expected_fields.extend(serde_json::from_str::<serde_json::Value>(r#"{ "after_head": "after-head", "after_head_tree": "after-tree", "after_git_dir": null, "after_toplevel": null,
"after_staged_paths": ["after-staged"], "after_unstaged_paths": [], "after_untracked_paths": [],
"after_factor_checkpoint": null, "after_factor_final_tree": null, "after_factor_phase": null, "after_factor_source": null,
"after_rebase_state": null, "after_rebase_msgnum": null, "after_rebase_end": null, "after_rebase_todo_head": null, "after_rebase_done_tail": null }"#).or_abort("expected JSON").as_object().or_abort("expected after snapshot").clone());
        trace_process_command(&fixture.context(), trace);
        let text = fs::read_to_string(fixture.directory().join("trace")).or_abort("trace");
        let mut actual =
            serde_json::from_str::<serde_json::Value>(&text).or_abort("one trace record");
        assert!(
            actual
                .as_object_mut()
                .or_abort("trace object")
                .remove("ts_unix_ms")
                .and_then(|value| value.as_u64())
                .is_some()
        );
        assert_eq!(actual, expected);
        assert_eq!(fixture.queries(), 0);
        assert_eq!(text.lines().count(), 1);
    }
}
mod write_error_log {
    use super::super::{FactorError, StateDir, log_contracts::LogFixture, write_error_log};
    use crate::test_support::OrAbort as _;
    use std::ffi::OsString;
    use std::fs;
    #[test]
    fn writes_the_admitted_directory_when_native_observations_are_unavailable() {
        let fixture = LogFixture::new(false);
        let state = StateDir::new(fixture.directory().join("owned"));
        fs::create_dir_all(state.as_path()).or_abort("admitted directory");
        fs::write(state.as_path().join("error.log"), b"old diagnostic").or_abort("old log");
        let expected = format!(
            "argv=git-factor --status\ncwd={}\nerror=no active factor session\nstaged_paths=\nunstaged_paths=\nuntracked_paths=\n",
            fixture.directory().display()
        );
        let actual = write_error_log(
            &fixture.context(),
            &state,
            &[OsString::from("git-factor"), OsString::from("--status")],
            &FactorError::NoActiveSession,
        );
        actual.or_abort("diagnostic publication");
        let text = fs::read_to_string(state.as_path().join("error.log")).or_abort("diagnostic");
        let (clock, body) = text.split_once('\n').or_abort("clock header");
        assert!(
            clock
                .strip_prefix("ts_unix_ms=")
                .and_then(|value| value.parse::<u64>().ok())
                .is_some()
        );
        assert_eq!(body, expected);
        assert_eq!(fixture.queries(), 4);
        assert_eq!(
            fs::read_dir(fixture.directory())
                .or_abort("inventory")
                .count(),
            1
        );
    }
}
