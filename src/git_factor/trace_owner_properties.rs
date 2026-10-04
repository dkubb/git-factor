mod read_rebase_counter {
    use super::super::{RebaseCounter, log_contracts::LogFixture, read_rebase_counter};
    use crate::test_support::OrAbort as _;
    use proptest::prelude::*;
    use std::fs;
    #[derive(Clone, Copy, Debug)]
    enum CounterFile {
        Absent,
        Malformed(u32),
        Present(u32),
    }
    proptest! {
        #[test]
        fn reads_generated_native_counters_and_refuses_missing_or_malformed_files(world in prop_oneof![Just(CounterFile::Absent), any::<u32>().prop_map(CounterFile::Malformed), any::<u32>().prop_map(CounterFile::Present)]) {
            let fixture = LogFixture::new(false);
            let path = fixture.directory().join("counter");
            let input = match world { CounterFile::Absent => None, CounterFile::Malformed(value) => Some(format!("-{value}")), CounterFile::Present(value) => Some(format!(" \t{value}\r\n")) };
            if let Some(text) = input.as_ref() { fs::write(&path, text).or_abort("counter file"); }
            let expected = match world { CounterFile::Present(value) => Some(value), CounterFile::Absent | CounterFile::Malformed(_) => None };
            let actual = read_rebase_counter(&fixture.context(), &path).map(RebaseCounter::as_u32);
            prop_assert_eq!(actual, expected);
            prop_assert_eq!(fs::read_to_string(&path).ok(), input);
            prop_assert_eq!(fixture.queries(), 0);
        }
    }
}
mod push_snapshot_fields {
    use super::super::{RebaseCounter, RebaseState, RepoSnapshot, push_snapshot_fields};
    use crate::test_support::OrAbort as _;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn preserves_generated_optional_facts_paths_and_counter_widths(facts in prop::option::of((any::<String>(), "[0-9a-f]{40}", any::<u32>(), any::<bool>()))) {
            let present = facts.is_some();
        let (text, hash, count, apply) = facts.unwrap_or_default();
        let suffix: String = hash.chars().skip(1).collect();
        let head = present.then(|| format!("0{suffix}"));
        let tree = present.then(|| format!("1{suffix}"));
        let checkpoint = present.then(|| format!("2{suffix}"));
        let final_tree = present.then(|| format!("3{suffix}"));
        let source = present.then(|| format!("4{suffix}"));
        let directory = present.then(|| format!("directory/{text}"));
        let toplevel = present.then(|| format!("toplevel/{text}"));
        let todo = present.then(|| format!("todo/{text}"));
        let done = present.then(|| format!("done/{text}"));
        let staged = if present { vec![format!("staged/{text}")] } else { Vec::new() };
        let unstaged = if present { vec![format!("unstaged/{text}")] } else { Vec::new() };
        let untracked = if present { vec![format!("untracked/{text}")] } else { Vec::new() };
        let phase = present.then_some("selecting");
        let state = present.then_some(if apply { "rebase-apply" } else { "rebase-merge" });
        let msgnum = present.then(|| count.to_string());
        let end = present.then(|| (!count).to_string());
        let snapshot = RepoSnapshot {
            head: head.clone(), head_tree: tree.clone(), git_dir: directory.clone(), toplevel: toplevel.clone(),
            staged_paths: staged.clone(), unstaged_paths: unstaged.clone(), untracked_paths: untracked.clone(),
            factor_checkpoint: checkpoint.clone(), factor_final_tree: final_tree.clone(), factor_phase: phase.map(str::to_owned), factor_source: source.clone(),
            rebase_state: present.then_some(if apply { RebaseState::Apply } else { RebaseState::Merge }),
            rebase_msgnum: present.then_some(RebaseCounter(count)), rebase_end: present.then_some(RebaseCounter(!count)),
            rebase_todo_head: todo.clone(), rebase_done_tail: done.clone(),
        };
        let expected = serde_json::Value::Object([
            ("state_head".to_owned(), serde_json::to_value(&head).or_abort("expected JSON field")),
            ("state_head_tree".to_owned(), serde_json::to_value(&tree).or_abort("expected JSON field")),
            ("state_git_dir".to_owned(), serde_json::to_value(&directory).or_abort("expected JSON field")),
            ("state_toplevel".to_owned(), serde_json::to_value(&toplevel).or_abort("expected JSON field")),
            ("state_staged_paths".to_owned(), serde_json::to_value(&staged).or_abort("expected JSON field")),
            ("state_unstaged_paths".to_owned(), serde_json::to_value(&unstaged).or_abort("expected JSON field")),
            ("state_untracked_paths".to_owned(), serde_json::to_value(&untracked).or_abort("expected JSON field")),
            ("state_factor_checkpoint".to_owned(), serde_json::to_value(&checkpoint).or_abort("expected JSON field")),
            ("state_factor_final_tree".to_owned(), serde_json::to_value(&final_tree).or_abort("expected JSON field")),
            ("state_factor_phase".to_owned(), serde_json::to_value(phase).or_abort("expected JSON field")),
            ("state_factor_source".to_owned(), serde_json::to_value(&source).or_abort("expected JSON field")),
            ("state_rebase_state".to_owned(), serde_json::to_value(state).or_abort("expected JSON field")),
            ("state_rebase_msgnum".to_owned(), serde_json::to_value(&msgnum).or_abort("expected JSON field")),
            ("state_rebase_end".to_owned(), serde_json::to_value(&end).or_abort("expected JSON field")),
            ("state_rebase_todo_head".to_owned(), serde_json::to_value(&todo).or_abort("expected JSON field")),
            ("state_rebase_done_tail".to_owned(), serde_json::to_value(&done).or_abort("expected JSON field")),
        ].into_iter().collect());
            let mut encoded = "{".to_owned();
            push_snapshot_fields(&mut encoded, "state", &snapshot);
            encoded.push('}');
            let actual = serde_json::from_str::<serde_json::Value>(&encoded).or_abort("snapshot JSON");
            prop_assert_eq!(actual, expected);
        }
    }
}
mod trace_note {
    use super::super::{log_contracts::LogFixture, trace_note};
    use crate::test_support::OrAbort as _;
    use proptest::prelude::*;
    use std::fs;
    proptest! {
            #[test]
            fn preserves_generated_literal_event_fields_or_omits_disabled_trace(detail in prop::option::of(any::<String>())) {
                let enabled = detail.is_some();
                let fixture = LogFixture::new(enabled);
                let expected = serde_json::Value::Object([("event".to_owned(), serde_json::to_value("factor_cmd_status").or_abort("expected JSON field")),
    ("detail".to_owned(), serde_json::to_value(&(detail)).or_abort("expected JSON field")),
    ("state_head".to_owned(), serde_json::Value::Null),
    ("state_head_tree".to_owned(), serde_json::Value::Null),
    ("state_git_dir".to_owned(), serde_json::Value::Null),
    ("state_toplevel".to_owned(), serde_json::Value::Null),
    ("state_staged_paths".to_owned(), serde_json::Value::Array(Vec::new())),
    ("state_unstaged_paths".to_owned(), serde_json::Value::Array(Vec::new())),
    ("state_untracked_paths".to_owned(), serde_json::Value::Array(Vec::new())),
    ("state_factor_checkpoint".to_owned(), serde_json::Value::Null),
    ("state_factor_final_tree".to_owned(), serde_json::Value::Null),
    ("state_factor_phase".to_owned(), serde_json::Value::Null),
    ("state_factor_source".to_owned(), serde_json::Value::Null),
    ("state_rebase_state".to_owned(), serde_json::Value::Null),
    ("state_rebase_msgnum".to_owned(), serde_json::Value::Null),
    ("state_rebase_end".to_owned(), serde_json::Value::Null),
    ("state_rebase_todo_head".to_owned(), serde_json::Value::Null),
    ("state_rebase_done_tail".to_owned(), serde_json::Value::Null)].into_iter().collect());
                let fields = [("detail", detail.as_deref().unwrap_or("inactive"))];
                trace_note(&fixture.context(), "factor_cmd_status", &fields);
                if enabled {
                    let text = fs::read_to_string(fixture.directory().join("trace")).or_abort("trace");
                    let mut actual = serde_json::from_str::<serde_json::Value>(&text).or_abort("one trace record");
                    prop_assert!(actual.as_object_mut().or_abort("trace object").remove("ts_unix_ms").and_then(|clock| clock.as_u64()).is_some());
                    prop_assert_eq!(actual, expected);
                    prop_assert_eq!(fixture.queries(), 4);
                    prop_assert_eq!(text.lines().count(), 1);
                } else {
                    prop_assert_eq!(fixture.queries(), 0);
                    prop_assert_eq!(fs::read_dir(fixture.directory()).or_abort("native inventory").count(), 0);
                }
            }
        }
}
mod trace_process_command {
    use super::super::{
        ProcessTrace, RepoSnapshot, log_contracts::LogFixture, trace_process_command,
    };
    use crate::test_support::OrAbort as _;
    use proptest::prelude::*;
    use std::fs;
    proptest! {
            #[test]
            fn preserves_generated_native_child_fields_and_env_assignment_distinctions(value in any::<String>(), exit in any::<u8>(), duration in any::<u64>(), quiet in any::<bool>(), spawned in any::<bool>(), assigned in any::<bool>()) {
                let fixture = LogFixture::new(true);
                let before = RepoSnapshot { head: Some(format!("before/{value}")), head_tree: Some(format!("before-tree/{value}")), staged_paths: vec![format!("before-staged/{value}")], ..RepoSnapshot::default() };
            let after = RepoSnapshot { head: Some(format!("after/{value}")), head_tree: Some(format!("after-tree/{value}")), staged_paths: vec![format!("after-staged/{value}")], ..RepoSnapshot::default() };
                let environment = [("VALUE", assigned.then_some(value.as_str()))];
                let args = [value.as_str()];
                let code = spawned.then_some(i32::from(exit));
                let stdout = spawned.then_some(value.as_str());
                let trace = ProcessTrace { mode: "output", bin: "git", args: &args, envs: &environment, quiet, spawned, duration_ms: duration, exit_code: code, stdout, stderr: Some(&value), before: &before, after: &after };
                let expected_environment = if assigned { format!("VALUE={value}") } else { "VALUE".to_owned() };
                let mut expected = serde_json::Value::Object([("event".to_owned(), serde_json::to_value("process").or_abort("expected JSON field")),
    ("mode".to_owned(), serde_json::to_value("output").or_abort("expected JSON field")),
    ("bin".to_owned(), serde_json::to_value("git").or_abort("expected JSON field")),
    ("args".to_owned(), serde_json::to_value([&(value)]).or_abort("expected JSON field")),
    ("env".to_owned(), serde_json::to_value([&(expected_environment)]).or_abort("expected JSON field")),
    ("quiet".to_owned(), serde_json::to_value(quiet).or_abort("expected JSON field")),
    ("spawned".to_owned(), serde_json::to_value(spawned).or_abort("expected JSON field")),
    ("duration_ms".to_owned(), serde_json::to_value(duration).or_abort("expected JSON field")),
    ("exit_code".to_owned(), serde_json::to_value(code).or_abort("expected JSON field")),
    ("stdout".to_owned(), serde_json::to_value(stdout).or_abort("expected JSON field")),
    ("stderr".to_owned(), serde_json::to_value(&(value)).or_abort("expected JSON field"))].into_iter().collect());
                let expected_fields = expected.as_object_mut().or_abort("expected process object");
                expected_fields.extend(serde_json::Value::Object([("before_head".to_owned(), serde_json::Value::from(format!("before/{value}"))),
    ("before_head_tree".to_owned(), serde_json::Value::from(format!("before-tree/{value}"))),
    ("before_git_dir".to_owned(), serde_json::Value::Null),
    ("before_toplevel".to_owned(), serde_json::Value::Null),
    ("before_staged_paths".to_owned(), serde_json::Value::Array(vec![serde_json::Value::from(format!("before-staged/{value}"))])),
    ("before_unstaged_paths".to_owned(), serde_json::Value::Array(Vec::new())),
    ("before_untracked_paths".to_owned(), serde_json::Value::Array(Vec::new())),
    ("before_factor_checkpoint".to_owned(), serde_json::Value::Null),
    ("before_factor_final_tree".to_owned(), serde_json::Value::Null),
    ("before_factor_phase".to_owned(), serde_json::Value::Null),
    ("before_factor_source".to_owned(), serde_json::Value::Null),
    ("before_rebase_state".to_owned(), serde_json::Value::Null),
    ("before_rebase_msgnum".to_owned(), serde_json::Value::Null),
    ("before_rebase_end".to_owned(), serde_json::Value::Null),
    ("before_rebase_todo_head".to_owned(), serde_json::Value::Null),
    ("before_rebase_done_tail".to_owned(), serde_json::Value::Null)].into_iter().collect()).as_object().or_abort("expected before snapshot").clone());
                expected_fields.extend(serde_json::Value::Object([("after_head".to_owned(), serde_json::Value::from(format!("after/{value}"))),
    ("after_head_tree".to_owned(), serde_json::Value::from(format!("after-tree/{value}"))),
    ("after_git_dir".to_owned(), serde_json::Value::Null),
    ("after_toplevel".to_owned(), serde_json::Value::Null),
    ("after_staged_paths".to_owned(), serde_json::Value::Array(vec![serde_json::Value::from(format!("after-staged/{value}"))])),
    ("after_unstaged_paths".to_owned(), serde_json::Value::Array(Vec::new())),
    ("after_untracked_paths".to_owned(), serde_json::Value::Array(Vec::new())),
    ("after_factor_checkpoint".to_owned(), serde_json::Value::Null),
    ("after_factor_final_tree".to_owned(), serde_json::Value::Null),
    ("after_factor_phase".to_owned(), serde_json::Value::Null),
    ("after_factor_source".to_owned(), serde_json::Value::Null),
    ("after_rebase_state".to_owned(), serde_json::Value::Null),
    ("after_rebase_msgnum".to_owned(), serde_json::Value::Null),
    ("after_rebase_end".to_owned(), serde_json::Value::Null),
    ("after_rebase_todo_head".to_owned(), serde_json::Value::Null),
    ("after_rebase_done_tail".to_owned(), serde_json::Value::Null)].into_iter().collect()).as_object().or_abort("expected after snapshot").clone());
                trace_process_command(&fixture.context(), trace);
                let text = fs::read_to_string(fixture.directory().join("trace")).or_abort("trace");
                let mut actual = serde_json::from_str::<serde_json::Value>(&text).or_abort("one trace record");
                prop_assert!(actual.as_object_mut().or_abort("trace object").remove("ts_unix_ms").and_then(|clock| clock.as_u64()).is_some());
                prop_assert_eq!(actual, expected);
                prop_assert_eq!(fixture.queries(), 0);
                prop_assert_eq!(text.lines().count(), 1);
            }
        }
}
mod write_error_log {
    use super::super::{FactorError, StateDir, log_contracts::LogFixture, write_error_log};
    use crate::test_support::OrAbort as _;
    use proptest::prelude::*;
    use std::ffi::OsString;
    use std::fs;
    use std::io;
    proptest! {
        #[test]
        fn preserves_generated_primary_and_source_diagnostics_in_the_admitted_directory(diagnostic in "[A-Za-z0-9 $;]{1,40}", argument in "[A-Za-z0-9 $;]{0,24}", kind in prop::sample::select(vec![io::ErrorKind::BrokenPipe, io::ErrorKind::PermissionDenied])) {
            let fixture = LogFixture::new(false);
            let state = StateDir::new(fixture.directory().join("owned"));
            fs::create_dir_all(state.as_path()).or_abort("admitted directory");
            fs::write(state.as_path().join("error.log"), b"old diagnostic").or_abort("old log");
            let expected = format!("argv=git-factor {argument}\ncwd={}\nerror=failed to write output: {diagnostic}\nsource_0={diagnostic}\nstaged_paths=\nunstaged_paths=\nuntracked_paths=\n", fixture.directory().display());
            let args = [OsString::from("git-factor"), OsString::from(argument)];
            let error = FactorError::Io(io::Error::new(kind, diagnostic));
            let actual = write_error_log(&fixture.context(), &state, &args, &error);
            actual.or_abort("diagnostic publication");
            let text = fs::read_to_string(state.as_path().join("error.log")).or_abort("diagnostic");
            let (clock, body) = text.split_once('\n').or_abort("clock header");
            prop_assert!(clock.strip_prefix("ts_unix_ms=").and_then(|value| value.parse::<u64>().ok()).is_some());
            prop_assert_eq!(body, expected);
            prop_assert_eq!(fixture.queries(), 4);
            prop_assert_eq!(fs::read_dir(fixture.directory()).or_abort("inventory").count(), 1);
        }
    }
}
