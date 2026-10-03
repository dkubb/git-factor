use super::*;
use crate::exit_codes::{EXIT_OK, EXIT_SOFTWARE, EXIT_USAGE};
use crate::git_factor::tests::abort_contracts;
use crate::git_factor::tests::continue_contracts;
use crate::git_factor::tests::dispatch_contracts::{
    HeadResolutionFault, ReopenFault, default_head_failure, default_head_success, dirty_route,
    parser, parser_write_failure, refusal, reopen_failure, supplied_commit_failure,
    supplied_commit_success,
};
use crate::git_factor::tests::finish_contracts::{FinishCase, FinishStage, verify_finish};
use crate::git_factor::tests::start_contracts::launcher::{LaunchFault, LaunchFixture};
use crate::git_factor::tests::start_contracts::replay;
use crate::git_factor::tests::start_contracts::{DirectStart, GateCase, query};
use crate::git_factor::tests::status_contracts;
use crate::git_factor::tests::verify_public_abort_fallback_refusal;
use crate::git_factor::tests::verify_public_status_phase_refusal;
use crate::git_factor::tests::{
    verify_public_abort, verify_public_inactive_status, verify_public_status,
};
use core::iter::repeat_n;
use core::num::{NonZeroU8, NonZeroUsize};
use core::ops::RangeInclusive;
use proptest::collection::vec as generated_bytes;
use proptest::prelude::*;

proptest! {
    #[test]
    fn public_abort_preserves_generated_saved_head_and_rebase_routes(
        sha in "[0-9a-f]{40}", started in any::<bool>(), rebase in any::<bool>(), saved_head in any::<bool>(),
    ) { abort_contracts::successful(&sha, started, rebase, saved_head, 0); }

    #[test]
    fn public_abort_reports_generated_output_refusals_after_cleanup(
        sha in "[0-9a-f]{40}", fail_at in RangeInclusive::<usize>::new(1, 2),
    ) { abort_contracts::successful(&sha, false, true, false, fail_at); }

    #[test]
    fn public_abort_preserves_generated_inactive_user_input(path in "[a-z]{1,12}") {
        abort_contracts::inactive(&path);
    }

    #[test]
    fn public_abort_reports_generated_required_field_read_refusals(
        sha in "[0-9a-f]{40}", key in prop::sample::select(vec!["commits", "requires_rebase", "started_rebase", "start_head"]),
    ) { abort_contracts::read_refusal(&sha, key); }

    #[test]
    fn public_abort_reports_generated_native_refusals_before_cleanup(
        sha in "[0-9a-f]{40}", operation in prop::sample::select(vec!["rebase", "reset", "clean"]), exit in { let first: i32 = 1; let last: i32 = 255; RangeInclusive::new(first, last) },
    ) { abort_contracts::native_refusal(&sha, operation, exit); }

    #[test]
    fn public_abort_preserves_generated_cleanup_refusals(
        sha in "[0-9a-f]{40}", leaves_path in any::<bool>(),
    ) { abort_contracts::cleanup_refusal(&sha, leaves_path); }
}

proptest! {
    #[test]
    fn public_abort_refuses_generated_unavailable_fallback_without_mutation(
        index in RangeInclusive::<u8>::new(1, 9),
    ) {
        verify_public_abort_fallback_refusal(index);
    }
}

proptest! {
    #[test]
    fn public_status_preserves_generated_active_frames_and_write_refusals(
        sha in "[0-9a-f]{40}",
        split in any::<u8>(),
        requires in any::<bool>(),
        root in any::<bool>(),
        phase in prop::sample::select(vec!["splitting", "pending_start"]),
        rebase in any::<bool>(),
        fail_at in RangeInclusive::<usize>::new(0, 2),
    ) {
        status_contracts::active(&sha, split, requires, root, phase, rebase, fail_at);
    }

    #[test]
    fn public_status_preserves_generated_inactive_frames_and_write_refusals(
        path in "[a-z]{1,12}",
        fail_at in RangeInclusive::<usize>::new(0, 2),
    ) {
        status_contracts::inactive(&path, fail_at);
    }

    #[test]
    fn public_status_reports_generated_required_field_read_refusals(
        sha in "[0-9a-f]{40}",
        field in prop::sample::select(Vec::<(&str, usize)>::from([("commits", 1), ("current_index", 1), ("current_index", 2), ("split_count", 1), ("requires_rebase", 1), ("is_root", 1), ("phase", 1)])),
    ) {
        status_contracts::read_refusal(&sha, field.0, field.1);
    }
}

proptest! {
    #[test]
    fn public_status_refuses_generated_unknown_phase_without_mutation(
        suffix in "[a-zA-Z0-9]{0,24}",
    ) {
        verify_public_status_phase_refusal(&format!("unsupported:{suffix}"));
    }
}

proptest! {
    #[test]
    fn preserves_selected_span_and_gate_output(
        shas in prop::collection::vec(
            string_regex("[0-9a-f]{40}").or_abort("SHA strategy"),
            1..=4,
        ),
        root in any::<bool>(),
        pass in any::<bool>(),
        stdout in string_regex("[a-z]{0,12}").or_abort("stdout strategy"),
        stderr in string_regex("[a-z]{0,12}").or_abort("stderr strategy"),
    ) {
        let selected = NonEmpty::from_vec(
            shas.into_iter().map(|sha| CommitSha::new(sha).or_abort("admitted SHA")).collect(),
        ).or_abort("nonempty generated span");
        let fixture = DirectStart::new(
            &selected,
            root,
            if pass { GateCase::Pass } else { GateCase::Fail },
            &stdout,
            &stderr,
        );
        let ctx = fixture.ctx();

        let result = cmd_start_with_resolved_in(
            &ctx,
            &fixture.exec,
            &fixture.state,
            &fixture.selected,
        );

        prop_assert_eq!(
            &result.map_err(|err| err.to_string()),
            &fixture.expected_result,
        );
        prop_assert_eq!(&fixture.observed_calls(), &fixture.expected_calls);
        prop_assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
        prop_assert_eq!(&fixture.io.stdout(), &fixture.expected_stdout);
        prop_assert_eq!(&fixture.io.stderr(), &fixture.expected_stderr);
        prop_assert_eq!(&fixture.direct_files_after(), &fixture.direct_files_before);
        prop_assert_eq!(&fixture.observed_journal(), &fixture.expected_journal);
    }

    #[test]
    fn preserves_the_mutation_boundary_when_output_fails(
        sha in string_regex("[0-9a-f]{40}").or_abort("SHA strategy"),
        // Intentionally fix span/root/text here; property1 explores those inputs separately.
        // Fault bounds include the single-commit guide and two nonempty gate streams.
        case in prop_oneof![
            RangeInclusive::<usize>::new(
                1, DirectStart::single_commit_two_stream_fault_end(true).get(),
            )
                .prop_map(|at| GateCase::PassOutputFailure(
                    NonZeroUsize::new(at).or_abort("positive fault"),
                )),
            RangeInclusive::<usize>::new(
                1, DirectStart::single_commit_two_stream_fault_end(false).get(),
            )
                .prop_map(|at| GateCase::FailOutputFailure(
                    NonZeroUsize::new(at).or_abort("positive fault"),
                )),
        ],
    ) {
        let selected = NonEmpty::new(CommitSha::new(sha).or_abort("admitted SHA"));
        let fixture = DirectStart::new(&selected, false, case, "gate out", "gate err");
        let ctx = fixture.ctx();

        let result = cmd_start_with_resolved_in(
            &ctx,
            &fixture.exec,
            &fixture.state,
            &fixture.selected,
        );

        prop_assert_eq!(
            &result.map_err(|err| err.to_string()),
            &fixture.expected_result,
        );
        prop_assert_eq!(&fixture.observed_calls(), &fixture.expected_calls);
        prop_assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
        prop_assert_eq!(&fixture.io.stdout(), &fixture.expected_stdout);
        prop_assert_eq!(&fixture.io.stderr(), &fixture.expected_stderr);
        prop_assert_eq!(&fixture.direct_files_after(), &fixture.direct_files_before);
        prop_assert_eq!(&fixture.observed_journal(), &fixture.expected_journal);
    }

    #[test]
    fn characterizes_selected_query_failure_boundaries(
        shas in prop::collection::vec(
            string_regex("[0-9a-f]{40}").or_abort("SHA strategy"), 1..=4,
        ),
        case in prop_oneof![
            1 => Just(query::QueryCase::EmptyShort),
            34 => (
                prop_oneof![
                    prop_oneof![Just(query::QueryPosition::First), Just(query::QueryPosition::Last)]
                        .prop_map(query::QueryTarget::Ancestor),
                    Just(query::QueryTarget::Diff), Just(query::QueryTarget::EmptyRoot),
                    Just(query::QueryTarget::Gate), Just(query::QueryTarget::Head),
                    Just(query::QueryTarget::Message), Just(query::QueryTarget::Parent),
                    Just(query::QueryTarget::Reset), Just(query::QueryTarget::Short),
                    Just(query::QueryTarget::Syntax), Just(query::QueryTarget::TopLevel),
                    Just(query::QueryTarget::Tree), Just(query::QueryTarget::Untracked),
                    Just(query::QueryTarget::Worktree),
                ],
                prop_oneof![Just(query::QueryReply::Io), Just(query::QueryReply::Rejected)],
            ).prop_map(|(target, reply)| query::QueryCase::Failure { target, reply }),
        ],
        root in any::<bool>(),
        stdout in string_regex("[a-z]{0,12}").or_abort("stdout strategy"),
        stderr in string_regex("[a-z]{0,12}").or_abort("stderr strategy"),
    ) {
        let selected = NonEmpty::from_vec(
            shas.into_iter().map(|sha| CommitSha::new(sha).or_abort("admitted SHA")).collect(),
        ).or_abort("nonempty generated span");
        let fixture = query::direct_start(&selected, root, &case, &stdout, &stderr);
        let ctx = fixture.ctx();

        let result = cmd_start_with_resolved_in(
            &ctx, &fixture.exec, &fixture.state, &fixture.selected,
        );

        prop_assert_eq!(&result.map_err(|err| err.to_string()), &fixture.expected_result);
        prop_assert_eq!(&fixture.observed_calls(), &fixture.expected_calls);
        prop_assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
        prop_assert_eq!(&fixture.io.stdout(), &fixture.expected_stdout);
        prop_assert_eq!(&fixture.io.stderr(), &fixture.expected_stderr);
        prop_assert_eq!(&fixture.observed_journal(), &fixture.expected_journal);
        // End-state contents/layout only, inside the owned tempdir and outside factor state.
        prop_assert_eq!(&fixture.direct_files_after(), &fixture.direct_files_before);
    }

    #[test]
    fn characterizes_selected_object_admission_failures(
        shas in prop::collection::vec(
            string_regex("[0-9a-f]{40}").or_abort("SHA strategy"), 1..=4,
        ),
        position in prop_oneof![Just(query::QueryPosition::First), Just(query::QueryPosition::Last)],
        reply in prop_oneof![Just(query::QueryReply::Io), Just(query::QueryReply::Rejected)],
    ) {
        let selected = NonEmpty::from_vec(
            shas.into_iter().map(|sha| CommitSha::new(sha).or_abort("admitted SHA")).collect(),
        ).or_abort("nonempty generated span");
        let case = query::QueryCase::Failure {
            target: query::QueryTarget::MergeParent(position),
            reply,
        };
        let fixture = query::selected_start(&selected, &case);
        let ctx = fixture.ctx();
        let refs = NonEmpty::new(
            NonEmptyString::try_from("base..tip".to_owned()).or_abort("explicit range"),
        );

        let result = cmd_start_in(&ctx, &fixture.exec, &refs);

        prop_assert_eq!(&result.map_err(|err| err.to_string()), &fixture.expected_result);
        prop_assert_eq!(&fixture.observed_calls(), &fixture.expected_calls);
        prop_assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
        prop_assert_eq!(&fixture.io.stdout(), &fixture.expected_stdout);
        prop_assert_eq!(&fixture.io.stderr(), &fixture.expected_stderr);
        prop_assert_eq!(&fixture.observed_journal(), &fixture.expected_journal);
        // End-state contents/layout only, inside the owned tempdir and outside factor state.
        prop_assert_eq!(&fixture.direct_files_after(), &fixture.direct_files_before);
    }
}

proptest! {
    #[test]
    fn characterizes_opening_replay_and_recovery_boundaries(
        shas in prop::collection::vec(
            string_regex("[0-9a-f]{40}").or_abort("SHA strategy"), 1..=4),
        root in any::<bool>(),
        case in prop_oneof![
            Just(replay::ReplayCase::FailedWithoutPause),
            Just(replay::ReplayCase::FinishedWithoutPause),
            Just(replay::ReplayCase::InvalidCommits),
            Just(replay::ReplayCase::InvalidPhase),
            Just(replay::ReplayCase::Paused),
            Just(replay::ReplayCase::Waiting),
            Just(replay::ReplayCase::WaitingAfterBegin),
            (1..=replay::ReplayStart::single_commit_banner_fault_end().get())
                .prop_map(|at| replay::ReplayCase::BannerFailure(
                    NonZeroUsize::new(at).or_abort("positive banner fault"))),
        ],
    ) {
        let commits = shas.into_iter().map(|sha| CommitSha::new(sha).or_abort("admitted SHA"))
            .collect::<Vec<_>>();
        let selected = NonEmpty::from_vec(commits).or_abort("nonempty selected span");
        let fixture = replay::direct_start(&selected, root, &case);
        let ctx = fixture.ctx();

        let result = cmd_start_with_resolved_in(
            &ctx, &fixture.exec, &fixture.state, &fixture.selected,
        );

        prop_assert_eq!(result.map_err(|err| err.to_string()), fixture.expected_result.clone());
        prop_assert_eq!(fixture.io.stdout(), fixture.expected_stdout.clone());
        prop_assert_eq!(fixture.io.stderr(), "");
        prop_assert_eq!(fixture.observed_journal(), fixture.expected_journal.clone());
        prop_assert_eq!(fixture.observed_calls(), fixture.expected_calls.clone());
        prop_assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
        prop_assert_eq!(&fixture.direct_files_after(), &fixture.direct_files_before);
    }
}

proptest! {
    #[test]
    fn refuses_initial_state_creation_after_passing_gate(
        shas in prop::collection::vec(
            string_regex("[0-9a-f]{40}").or_abort("SHA strategy"), 1..=4,
        ),
        root in any::<bool>(),
        stdout in string_regex("[a-z]{0,12}").or_abort("stdout strategy"),
        stderr in string_regex("[a-z]{0,12}").or_abort("stderr strategy"),
    ) {
        use crate::git_factor::tests::start_contracts::state_creation;

        let selected = NonEmpty::from_vec(
            shas.into_iter().map(|sha| CommitSha::new(sha).or_abort("admitted SHA")).collect(),
        ).or_abort("nonempty generated span");
        let fixture = state_creation::direct_start(&selected, root, &stdout, &stderr);
        let fault = state_creation::RefusingStateCreationFs::default();
        let mut ctx = fixture.ctx();
        ctx.fs = &fault;

        let result = cmd_start_with_resolved_in(
            &ctx, &fixture.exec, &fixture.state, &fixture.selected,
        );

        prop_assert_eq!(
            result.map_err(|err| err.to_string()),
            Err("failed to write state: factor state creation refused".to_owned()),
        );
        prop_assert_eq!(fixture.io.stdout(), stdout);
        prop_assert_eq!(fixture.io.stderr(), stderr);
        prop_assert_eq!(fixture.observed_journal(), None);
        prop_assert_eq!(&fixture.observed_calls(), &fixture.expected_calls);
        prop_assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
        let attempts = fault.attempts.borrow().clone();
        prop_assert_eq!(attempts, vec![fixture.state.as_path().to_path_buf()]);
        // Only net contents/layout inside this tempdir, excluding factor state.
        prop_assert_eq!(&fixture.direct_files_after(), &fixture.direct_files_before);
    }
}

#[test]
fn cmd_abort_uses_current_commit_when_start_head_is_missing() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let commit = "a".repeat(COMMIT_SHA_HEX_LEN);
    fs::write(state_dir.join("commits"), format!("{commit}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");

    let io = Box::leak(Box::new(TestIo::default()));
    let env = Box::leak(Box::new(TestEnv));
    let fs = &REAL_FS;
    let runner = Box::leak(Box::new(ScriptedRunner::new(
        vec![
            Ok(Output {
                status: success_status(),
                stdout: b".git\n".to_vec(),
                stderr: Vec::new(),
            }),
            Ok(Output {
                status: success_status(),
                stdout: b".git\n".to_vec(),
                stderr: Vec::new(),
            }),
            Ok(Output {
                status: success_status(),
                stdout: b".git\n".to_vec(),
                stderr: Vec::new(),
            }),
        ],
        vec![Ok(success_status()), Ok(success_status())],
    )));
    let ctx = Ctx {
        cwd: repo.to_path_buf(),
        env,
        fs,
        io,
        runner,
    };

    let code = cmd_abort_in(&ctx).or_abort("abort should succeed");
    assert_eq!(code, EXIT_OK);
    assert_eq!(
        io.out.borrow().as_str(),
        "{\"operation\":\"abort\",\"rebase\":{\"in_progress\":false},\"actions\":{}}\n"
    );
    assert!(!state_dir.exists(), "state dir should be removed");
}

#[test]
fn cmd_abort_propagates_current_commit_error_when_start_head_is_missing() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let commit = "a".repeat(COMMIT_SHA_HEX_LEN);
    fs::write(state_dir.join("commits"), format!("{commit}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "1\n").or_abort("write current_index");

    let io = Box::leak(Box::new(TestIo::default()));
    let env = Box::leak(Box::new(TestEnv));
    let fs = &REAL_FS;
    let runner = Box::leak(Box::new(ScriptedRunner::new(
        vec![
            Ok(Output {
                status: success_status(),
                stdout: b".git\n".to_vec(),
                stderr: Vec::new(),
            }),
            Ok(Output {
                status: success_status(),
                stdout: b".git\n".to_vec(),
                stderr: Vec::new(),
            }),
        ],
        Vec::new(),
    )));
    let ctx = Ctx {
        cwd: repo.to_path_buf(),
        env,
        fs,
        io,
        runner,
    };

    let err = cmd_abort_in(&ctx)
        .err_or_abort("missing start_head should fall back to current commit lookup");
    assert_eq!(
        err.to_string(),
        "git command failed: commit index 1 out of range (have 1 commits)"
    );
}

proptest! {
    #[test]
    fn preserves_populated_begin_journal_when_no_pause_cleanup_is_denied(
        shas in prop::collection::vec(
            string_regex("[0-9a-f]{40}").or_abort("cleanup SHA strategy"), 1..=4,
        ),
        root in any::<bool>(),
        outcome in prop_oneof![
            Just(replay::cleanup::NoPause::Failed),
            Just(replay::cleanup::NoPause::Successful),
        ],
    ) {
        use alloc::collections::BTreeMap;

        let selected = NonEmpty::from_vec(
            shas.into_iter().map(|sha| CommitSha::new(sha).or_abort("admitted SHA"))
                .collect(),
        ).or_abort("nonempty selected span");
        let fixture = replay::cleanup::direct_launcher(&selected, root, outcome);
        let ctx = fixture.ctx();
        // Calculate the entire expected journal from generated input, not a SUT state writer.
        let expected_head = "0123456789abcdef".chars()
            .map(|digit| digit.to_string().repeat(COMMIT_SHA_HEX_LEN))
            .find(|head| selected.iter().all(|sha| sha.as_str() != head))
            .or_abort("bounded selection leaves an independent HEAD");
        let expected_commits = selected.iter().map(CommitSha::as_str)
            .collect::<Vec<_>>().join("\n");
        let expected_journal = [
            ("commits", format!("{expected_commits}\n")),
            ("current_index", format!("{}\n", selected.tail.len())),
            ("exec", "true\n".to_owned()),
            ("expected_tree", "dddddddddddddddddddddddddddddddddddddddd\n".to_owned()),
            ("is_root", format!("{root}\n")),
            ("phase", "pending_start\n".to_owned()),
            ("requires_rebase", "true\n".to_owned()),
            ("split_count", "0\n".to_owned()),
            ("start_head", format!("{expected_head}\n")),
            ("started_rebase", "true\n".to_owned()),
        ].into_iter().map(|(name, value)| (OsString::from(name), value.into_bytes()))
            .collect::<BTreeMap<_, _>>();
        prop_assert_eq!(fixture.journal_bytes(), None);
        prop_assert_eq!(&fixture.journal_at_begin(), &expected_journal);

        let result = run_start_rebase_in(
            &ctx, &fixture.span, &fixture.replay.state, &fixture.head,
            fixture.replay.exec.first(),
        );

        prop_assert_eq!(result.map_err(|err| err.to_string()), Err(format!(
            "git command failed: failed to remove factor state path '{}': \
             selected factor removal denied",
            fixture.replay.state.as_path().display(),
        )));
        prop_assert_eq!(fixture.replay.io.stdout(), "");
        prop_assert_eq!(fixture.replay.io.stderr(), "");
        prop_assert_eq!(
            fixture.removal_attempts(), vec![fixture.replay.state.as_path().to_path_buf()],
        );
        prop_assert_eq!(fixture.journal_bytes(), Some(expected_journal));
        prop_assert_eq!(fixture.replay.observed_calls(), fixture.replay.expected_calls.clone());
        prop_assert_eq!(fixture.replay.remaining_keys(), Vec::<String>::new());
        prop_assert_eq!(&fixture.replay.direct_files_after(), &fixture.replay.direct_files_before);
        prop_assert!(!ctx.cwd.join(".git/rebase-merge").exists());
        prop_assert!(!ctx.cwd.join(".git/rebase-apply").exists());
    }
}

proptest! {
    #[test]
    fn characterizes_launcher_capability_failure_boundaries(
        shas in prop::collection::vec(
            string_regex("[0-9a-f]{40}").or_abort("SHA strategy"), 1..=4,
        ),
        root in any::<bool>(),
        fault in prop_oneof![
            Just(LaunchFault::BeginExecutable),
            Just(LaunchFault::EditorCanonicalize),
            Just(LaunchFault::EditorExecutable),
            Just(LaunchFault::NativeLaunch),
            Just(LaunchFault::PreflightExecutable),
            Just(LaunchFault::ShortIo),
            RangeInclusive::<u8>::new(1, 255).prop_map(|code| LaunchFault::ShortRejected(
                NonZeroU8::new(code).or_abort("nonzero rejected exit"),
            )),
        ],
    ) {
        let selected = NonEmpty::from_vec(
            shas.into_iter().map(|sha| CommitSha::new(sha).or_abort("admitted SHA")).collect(),
        ).or_abort("nonempty selected span");
        let fixture = LaunchFixture::new(selected, root, &fault);
        let ctx = fixture.ctx();

        let result = run_start_rebase_in(
            &ctx, &fixture.span, &fixture.state, &fixture.start_head, &fixture.exec,
        );

        prop_assert_eq!(
            result.map_err(|err| err.to_string()),
            Err(fixture.expected_error().to_owned()),
        );
        prop_assert_eq!(fixture.calls(), fixture.expected_calls());
        prop_assert_eq!(fixture.remaining_executable_replies(), 0);
        prop_assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    }
}

#[cfg(unix)]
proptest! {
    #[test]
    fn rejects_generated_non_utf8_launcher_paths(
        fault in prop_oneof![
            "[a-z]{0,8}".prop_map(LaunchFault::EditorEncoding),
            "[a-z]{0,8}".prop_map(LaunchFault::PreflightEncoding),
            "[a-z]{0,8}".prop_map(LaunchFault::BeginEncoding),
        ],
    ) {
        let selected = NonEmpty::new(
            CommitSha::new("a".repeat(COMMIT_SHA_HEX_LEN)).or_abort("admitted SHA")
        );
        let fixture = LaunchFixture::new(selected, false, &fault);
        let ctx = fixture.ctx();

        let result = run_start_rebase_in(
            &ctx, &fixture.span, &fixture.state, &fixture.start_head, &fixture.exec,
        );

        prop_assert_eq!(
            result.map_err(|err| err.to_string()),
            Err(fixture.expected_error().to_owned()),
        );
        prop_assert_eq!(fixture.calls(), fixture.expected_calls());
        prop_assert_eq!(fixture.remaining_executable_replies(), 0);
        prop_assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    }
}

proptest! {
    #[test]
    fn public_continue_preserves_generated_completion(
        original in string_regex("[0-9a-f]{40}").or_abort("original identity"),
        split_count in prop_oneof![
            8 => u8::MIN..=254,
            1 => Just(253),
            1 => Just(254),
        ],
    ) {
        let fixture = continue_contracts::Continuation::new(
            &original, split_count, continue_contracts::ContinueCase::Complete,
        );
        let before = fixture.protected_bytes();
        let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

        let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

        prop_assert_eq!(code, EXIT_OK);
        prop_assert_eq!(fixture.stdout(), format!(
            "FACTOR: Complete. Final commit split into {} commits.\n", u16::from(split_count) + 1,
        ));
        prop_assert_eq!(fixture.stderr(), "");
        prop_assert_eq!(fixture.journal(), fixture.expected_journal());
        prop_assert_eq!(fixture.protected_bytes(), before);
        prop_assert_eq!(fixture.full_effect_requests(), fixture.expected_effect_requests());
        prop_assert!(!fixture.session_active());
        prop_assert_eq!(fixture.deleted_requests(), Vec::<PathBuf>::new());
    }
}

proptest! {
    #[test]
    fn public_continue_propagates_generated_query_failures(
        original in string_regex("[0-9a-f]{40}").or_abort("original identity"),
        fault in prop::sample::select(vec![
            continue_contracts::ContinueFault::SessionDir,
            continue_contracts::ContinueFault::StagedStatus,
            continue_contracts::ContinueFault::Checkout,
            continue_contracts::ContinueFault::Clean,
            continue_contracts::ContinueFault::CheckoutIndex,
            continue_contracts::ContinueFault::BeforeStatus,
            continue_contracts::ContinueFault::AfterStatus,
            continue_contracts::ContinueFault::Metadata,
            continue_contracts::ContinueFault::Commit,
            continue_contracts::ContinueFault::HeadTree,
            continue_contracts::ContinueFault::Restore,
            continue_contracts::ContinueFault::RestoredTree,
            continue_contracts::ContinueFault::Reset,
            continue_contracts::ContinueFault::DiffStat,
            continue_contracts::ContinueFault::Untracked,

        ]),
    ) {
        let fixture = continue_contracts::Continuation::with_fault(
            &original, fault);
        let before = fixture.protected_bytes();
        let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

        let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

        let expected_code = if matches!(fault,
            continue_contracts::ContinueFault::SessionDir) {
            EXIT_DATAERR
        } else { EXIT_SOFTWARE };
        prop_assert_eq!(code, expected_code);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(),
            continue_contracts::Continuation::fault_stderr(fault));
        prop_assert_eq!(fixture.journal(), fixture.expected_journal());
        prop_assert_eq!(fixture.protected_bytes(), before);
        prop_assert_eq!(fixture.full_effect_requests(), fixture.expected_effect_requests());
    }
}

proptest! {
    #[test]
    fn public_continue_preserves_generated_gate_and_remainder_outcomes(
        original in string_regex("[0-9a-f]{40}").or_abort("original identity"),
        split_count in prop_oneof![
            8 => u8::MIN..=254,
            1 => Just(253),
            1 => Just(254),
        ],
        case in prop::sample::select(vec![
            continue_contracts::ContinueCase::Complete,
            continue_contracts::ContinueCase::GateRejected,
            continue_contracts::ContinueCase::GateSpawn,
            continue_contracts::ContinueCase::NoStaged,
            continue_contracts::ContinueCase::PostGateDirty,
            continue_contracts::ContinueCase::PreGateDirty,
            continue_contracts::ContinueCase::Remainder,
        ]),
    ) {
        let fixture = continue_contracts::Continuation::new(
            &original, split_count, case);
        let before = fixture.protected_bytes();
        let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

        let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

        prop_assert_eq!(code, fixture.expected_code());
        prop_assert_eq!(fixture.stdout(), fixture.expected_stdout());
        prop_assert_eq!(fixture.stderr(), fixture.expected_stderr());
        prop_assert_eq!(fixture.journal(), fixture.expected_journal());
        prop_assert_eq!(fixture.protected_bytes(), before);
        prop_assert_eq!(fixture.full_effect_requests(), fixture.expected_effect_requests());
        prop_assert_eq!(fixture.session_active(),
            !matches!(case, continue_contracts::ContinueCase::Complete));
    }
}

proptest! {
    #[test]
    fn public_continue_preserves_generated_state_failure_boundaries(
        original in string_regex("[0-9a-f]{40}").or_abort("original identity"),
        fault in prop::sample::select(vec![
            continue_contracts::ContinueFileFault::ReadCommits,
            continue_contracts::ContinueFileFault::ReadPhase,
            continue_contracts::ContinueFileFault::ReadRequires,
            continue_contracts::ContinueFileFault::ReadIndex,
            continue_contracts::ContinueFileFault::ReadExec,
            continue_contracts::ContinueFileFault::ReadCount,
            continue_contracts::ContinueFileFault::ReadCountAfterCommit,
            continue_contracts::ContinueFileFault::ReadExpected,
            continue_contracts::ContinueFileFault::WriteCount,

        ]),
    ) {
        use continue_contracts::{Continuation, ContinueFileFault};
        let fixture = Continuation::with_file_fault(&original, fault);
        let mut journal = fixture.journal();
        if matches!(fault, ContinueFileFault::ReadExpected | ContinueFileFault::ReadCountAfterCommit) {
            journal.insert("split_count".to_owned(), "1\n".to_owned());
        }
        let bytes = fixture.protected_bytes();
        let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);
        let stderr = if matches!(fault, ContinueFileFault::WriteCount) {
            "failed to write state: continuation state write denied\n"
        } else if matches!(fault, ContinueFileFault::ReadExpected) {
            "git command failed: expected tree fallback denied\n"
        } else {
            "failed to read state: continuation state read denied\n"
        };

        let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

        prop_assert_eq!(code, EXIT_SOFTWARE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), stderr);
        prop_assert!(fixture.file_fault_observed());
        prop_assert_eq!(fixture.journal(), journal);
        prop_assert_eq!(fixture.protected_bytes(), bytes);
        prop_assert_eq!(fixture.full_effect_requests(), fixture.expected_effect_requests());
    }
}

proptest! {
    #[test]
    fn public_continue_preserves_generated_output_failure_frontiers(
        original in string_regex("[0-9a-f]{40}").or_abort("original identity"),
        write in prop::sample::select(vec![
            continue_contracts::ContinueWrite::Complete,
            continue_contracts::ContinueWrite::GateBanner,
            continue_contracts::ContinueWrite::GateCommand,
            continue_contracts::ContinueWrite::GateCode,
            continue_contracts::ContinueWrite::GateSeparator,
            continue_contracts::ContinueWrite::GateNext,
            continue_contracts::ContinueWrite::GateUsage,
            continue_contracts::ContinueWrite::Remainder,

        ]),
    ) {
        let fixture = continue_contracts::Continuation::with_write(
            &original, write);
        let bytes = fixture.protected_bytes();
        let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

        let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

        prop_assert_eq!(code, EXIT_SOFTWARE);
        prop_assert_eq!(fixture.stdout(), write.prefix());
        prop_assert_eq!(fixture.stderr(), "failed to write output: continuation output denied\n");
        prop_assert!(fixture.write_observed());
        prop_assert_eq!(fixture.journal(), fixture.expected_journal());
        prop_assert_eq!(fixture.protected_bytes(), bytes);
        prop_assert_eq!(fixture.full_effect_requests(), fixture.expected_effect_requests());
    }
}

proptest! {
    #[test]
    fn public_continue_preserves_generated_precondition_refusals(
        original in string_regex("[0-9a-f]{40}").or_abort("original identity"),
        state in prop::sample::select(vec![
            continue_contracts::ContinueState::Absent,
            continue_contracts::ContinueState::Pending,
            continue_contracts::ContinueState::RebaseRequired,
            continue_contracts::ContinueState::IndexOutsideSpan,
        ]),
    ) {
        use continue_contracts::{Continuation, ContinueState};
        let fixture = Continuation::with_state(&original, state);
        let journal = fixture.journal();
        let bytes = fixture.protected_bytes();
        let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);
        let (expected_code, stderr) = match state {
            ContinueState::Absent => (EXIT_USAGE, "no active factor session\n"),
            ContinueState::Pending => (EXIT_USAGE,
                "run 'git factor --continue' with no --message to begin splitting this commit\n"),
            ContinueState::RebaseRequired => (EXIT_SOFTWARE,
                "git command failed: no rebase in progress\n"),
            ContinueState::IndexOutsideSpan => (EXIT_SOFTWARE,
                "git command failed: commit index 1 out of range (have 1 commits)\n"),
        };

        let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

        prop_assert_eq!(code, expected_code);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), stderr);
        prop_assert_eq!(fixture.journal(), journal);
        prop_assert_eq!(fixture.protected_bytes(), bytes);
        prop_assert_eq!(fixture.full_effect_requests(), Vec::new());
    }

}

proptest! {
    #[test]
    fn public_continue_preserves_generated_rehydration_failure_boundaries(
        original in string_regex("[0-9a-f]{40}").or_abort("original identity"),
        failure in prop::sample::select(vec![
            continue_contracts::ContinueRecoveryFailure::GateRejected,
            continue_contracts::ContinueRecoveryFailure::GateSpawn,
            continue_contracts::ContinueRecoveryFailure::PostGateDirty,
        ]),
    ) {
        let fixture = continue_contracts::Continuation::with_recovery_failure(
            &original, failure);
        let journal = fixture.journal();
        let bytes = fixture.protected_bytes();
        let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

        let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

        prop_assert_eq!(code, EXIT_SOFTWARE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), "git command failed: git write-tree: continuation query denied\n");
        prop_assert_eq!(fixture.journal(), journal);
        prop_assert_eq!(fixture.protected_bytes(), bytes);
        prop_assert_eq!(fixture.full_effect_requests(), fixture.expected_effect_requests());
    }
}

proptest! {
    #[test]
    fn public_continue_rejects_generated_invalid_tree_replies_after_commit(
        original in string_regex("[0-9a-f]{40}").or_abort("original identity"),
        invalid in prop_oneof![
            string_regex("[g-z]{1,12}").or_abort("obvious invalid tree"),
            string_regex("[0-9a-f]{39}").or_abort("short hex tree"),
            string_regex("[0-9a-f]{41}").or_abort("long hex tree"),
            string_regex("[0-9a-f]{39}g").or_abort("nonhex forty-byte tree"),
        ],
        expected in any::<bool>(),
    ) {
        let fixture = continue_contracts::Continuation::with_invalid_tree(
            &original, &invalid, expected);
        let mut journal = fixture.journal();
        journal.insert("split_count".to_owned(), "1\n".to_owned());
        let bytes = fixture.protected_bytes();
        let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

        let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

        prop_assert_eq!(code, EXIT_SOFTWARE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), format!("git command failed: invalid tree hash: '{invalid}'\n"));
        prop_assert_eq!(fixture.journal(), journal);
        prop_assert_eq!(fixture.protected_bytes(), bytes);
        prop_assert_eq!(fixture.full_effect_requests(), fixture.expected_effect_requests());
    }
}

proptest! {
    #[test]
    fn public_continue_preserves_generated_required_active_rebase_admission(
        original in string_regex("[0-9a-f]{40}").or_abort("original identity"),
        merge in any::<bool>(),
    ) {
        let fixture = continue_contracts::Continuation::with_active_rebase(&original, merge);
        let before_journal = fixture.journal();
        let before_bytes = fixture.protected_bytes();
        let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

        let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

        prop_assert_eq!(code, EXIT_USAGE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), concat!(
            "no staged changes to commit\n",
            "NEXT: stage exactly one atomic change, then rerun:\n",
            "  git factor --continue --message \"type: description\"\n",
        ));
        prop_assert_eq!(fixture.journal(), before_journal);
        prop_assert_eq!(fixture.protected_bytes(), before_bytes);
        prop_assert_eq!(fixture.full_effect_requests(), fixture.expected_effect_requests());
    }
}

proptest! {
    #[test]
    fn public_continue_reports_generated_counter_overflow_after_commit(
        original in string_regex("[0-9a-f]{40}").or_abort("original identity"),
    ) {
        let fixture = continue_contracts::Continuation::with_counter_overflow(&original);
        let journal = fixture.journal();
        let bytes = fixture.protected_bytes();
        let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

        let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

        prop_assert_eq!(code, EXIT_SOFTWARE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), "git command failed: split_count overflow\n");
        prop_assert_eq!(fixture.journal(), journal);
        prop_assert_eq!(fixture.protected_bytes(), bytes);
        prop_assert_eq!(fixture.full_effect_requests(), fixture.expected_effect_requests());
    }

    #[test]
    fn public_continue_reports_generated_gate_rejection_codes(
        original in string_regex("[0-9a-f]{40}").or_abort("original identity"),
        rejected in 1..=u8::MAX,
    ) {
        let fixture = continue_contracts::Continuation::with_gate_code(&original, rejected);
        let journal = fixture.journal();
        let bytes = fixture.protected_bytes();
        let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

        let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

        prop_assert_eq!(code, EXIT_TEMPFAIL);
        prop_assert_eq!(fixture.stdout(), format!(concat!(
            "FACTOR: Exec gate failed. No commit created.\nEXEC: true\nCODE: {}\n\n",
            "NEXT: Adjust staged changes so the exec gate passes, then retry:\n",
            "  git factor --continue --message \"type: description\"\n",
        ), rejected));
        prop_assert_eq!(fixture.stderr(), format!("exec gate failed: true (exit code {rejected})\n"));
        prop_assert_eq!(fixture.journal(), journal);
        prop_assert_eq!(fixture.protected_bytes(), bytes);
        prop_assert_eq!(fixture.full_effect_requests(), fixture.expected_effect_requests());
    }

    #[test]
    fn public_status_preserves_generated_session_facts(
        index in RangeInclusive::<usize>::new(0, 2), count in any::<u8>(), pending in any::<bool>(),
        required in any::<bool>(), root in any::<bool>(), progress in any::<bool>(),
    ) {
        verify_public_status(index, count,
            if pending { SessionPhase::PendingStart } else { SessionPhase::Splitting },
            required, root, progress);
    }
}

proptest! {
    #[test]
    fn public_continue_routes_generated_no_message_active_phases(
        original in string_regex("[0-9a-f]{40}").or_abort("original identity"),
        pending in any::<bool>(),
    ) {
        use continue_contracts::{Continuation, ContinueCase, ContinueState};
        let fixture = if pending {
            Continuation::with_state(&original, ContinueState::Pending)
        } else {
            Continuation::new(&original, 0, ContinueCase::Remainder)
        };
        let journal = fixture.journal();
        let bytes = fixture.protected_bytes();
        let args = ["git-factor", "--continue"].map(OsString::from);
        let (expected_code, stderr) = if pending {
            (EXIT_SOFTWARE, concat!(
                "git command failed: baseline commit must be fully clean before opening the split session\n",
                "STATUS:\nM  file.txt\n",
            ))
        } else {
            (EXIT_USAGE, "--continue requires --message <MSG>\n")
        };

        let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

        prop_assert_eq!(code, expected_code);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), stderr);
        prop_assert_eq!(fixture.journal(), journal);
        prop_assert_eq!(fixture.protected_bytes(), bytes);
        prop_assert_eq!(fixture.effect_requests(), Vec::<Vec<String>>::new());
    }
}

proptest! {
    #[test]
    fn message_text_never_overrides_query_or_retry(
        operation in prop::sample::select(vec!["--status", "--retry"]),
        message in "[a-zA-Z][a-zA-Z0-9 ]{0,40}",
    ) {
        let diagnostic = match operation {
            "--status" => "--status cannot be combined with other options",
            _ => "--retry cannot be combined with other options",
        };
        refusal(&[operation, "--message", &message], diagnostic);
    }

    #[test]
    fn gate_text_never_overrides_explicit_operation(
        operation in prop::sample::select(vec!["--abort", "--status", "--retry", "--finish", "--continue"]),
        gate in "[a-zA-Z][a-zA-Z0-9 ]{0,40}",
    ) {
        let diagnostic = match operation {
            "--abort" => "--abort cannot be combined with other options",
            "--status" => "--status cannot be combined with other options",
            "--retry" => "--retry cannot be combined with other options",
            "--finish" => "--finish cannot be combined with --continue, --exec, or COMMIT",
            _ => "--continue cannot be combined with --exec or COMMIT",
        };
        refusal(&[operation, "--exec", &gate], diagnostic);
    }
}

proptest! {
    #[test]
    fn help_spelling_preserves_the_complete_stream(explicit in any::<bool>()) {
        let arguments: &[&str] = if explicit { &["--help"] } else { &[] };
        parser(arguments, EXIT_OK, include_str!("../tests/main_entry/help.txt"), "");
    }
}

proptest! {
    #[test]
    fn hidden_preflight_index_does_not_bypass_cleanliness(index in any::<usize>()) {
        dirty_route(&["rebase-exec-preflight", &index.to_string(), "true"], EXIT_SOFTWARE, "git command failed: cannot run the start gate because the repository is not clean\nSTATUS:\n M unrelated", &["git rev-parse --git-dir", "git status --porcelain=v1", "git rev-parse --git-dir"]);
    }
}

proptest! {
    #[test]
    fn parser_write_failures_preserve_user_bytes(
        arguments in prop::sample::select(vec![vec!["--help"], vec!["--not-real"], Vec::<&str>::new()]),
    ) {
        parser_write_failure(&arguments);
    }
}

proptest! {
    #[test]
    fn incompatible_operation_inputs_preserve_the_public_refusal(
        case in prop::sample::select(vec![
            ("abort","--status","--abort cannot be combined with other options"),
            ("abort","--continue","--abort cannot be combined with other options"),
            ("abort","--retry","--abort cannot be combined with other options"),
            ("abort","--finish","--abort cannot be combined with other options"),
            ("abort","--exec","--abort cannot be combined with other options"),
            ("abort","HEAD","--abort cannot be combined with other options"),
            ("status","--continue","--status cannot be combined with other options"),
            ("status","--retry","--status cannot be combined with other options"),
            ("status","--finish","--status cannot be combined with other options"),
            ("status","--exec","--status cannot be combined with other options"),
            ("status","HEAD","--status cannot be combined with other options"),
            ("status","--message","--status cannot be combined with other options"),
            ("retry","--continue","--retry cannot be combined with other options"),
            ("retry","--finish","--retry cannot be combined with other options"),
            ("retry","--exec","--retry cannot be combined with other options"),
            ("retry","HEAD","--retry cannot be combined with other options"),
            ("retry","--message","--retry cannot be combined with other options"),
            ("finish","--continue","--finish cannot be combined with --continue, --exec, or COMMIT"),
            ("finish","--exec","--finish cannot be combined with --continue, --exec, or COMMIT"),
            ("finish","HEAD","--finish cannot be combined with --continue, --exec, or COMMIT"),
            ("continue","--exec","--continue cannot be combined with --exec or COMMIT"),
            ("continue","HEAD","--continue cannot be combined with --exec or COMMIT")
        ]),
        value in "[a-zA-Z][a-zA-Z0-9 ]{0,40}",
    ) {
        let (operation, other, diagnostic) = case;
        let flag = format!("--{operation}");
        let mut arguments = vec![flag.as_str(), other];
        if matches!(other, "--exec" | "--message") { arguments.push(&value); }
        refusal(&arguments, diagnostic);
    }

    #[test]
    fn implicit_head_success_preserves_selected_identity(
        sha in "[0-9a-f]{40}",
    ) {
        default_head_success(&sha);
    }

    #[test]
    fn implicit_head_resolution_faults_preserve_the_public_error(
        fault in prop::sample::select(vec![HeadResolutionFault::LaunchFailure, HeadResolutionFault::NonzeroExit]),
    ) {
        default_head_failure(fault);
    }

    #[test]
    fn unknown_flags_preserve_the_exact_parser_error(suffix in "[0-9]{1,12}") {
        let flag = format!("--zzzzzzzzzz-{suffix}");
        let diagnostic = format!("error: unexpected argument '{flag}' found\n\n  tip: to pass '{flag}' as a value, use '-- {flag}'\n\nUsage: git-factor [OPTIONS] [COMMIT]...\n\nFor more information, try '--help'.\n");
        parser(&[&flag], EXIT_USAGE, "", &diagnostic);
    }

    #[test]
    fn version_spelling_preserves_the_exact_stream(explicit in any::<bool>()) {
        let flag = if explicit { "--version" } else { "-v" };
        parser(&[flag], EXIT_OK, "git-factor 0.1.0\n", "");
    }

    #[test]
    fn begin_index_and_root_flag_do_not_bypass_cleanliness(index in any::<usize>(), root in any::<bool>()) {
        dirty_route(&["rebase-exec-begin", &index.to_string(), "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", if root { "true" } else { "false" }, "true", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"], EXIT_SOFTWARE, "git command failed: cannot begin the factor session because the repository is not clean\nSTATUS:\n M unrelated", &["git status --porcelain=v1", "git rev-parse --git-dir"]);
    }

    #[test]
    fn default_head_gate_text_does_not_bypass_cleanliness(gate in "[a-zA-Z][a-zA-Z0-9 ]{0,40}") {
        dirty_route(&["--exec", &gate], EXIT_SOFTWARE, "git command failed: working tree must be clean before starting; stash, commit, or remove local changes\nSTATUS:\n M unrelated", &["git rev-parse --git-dir", "git rev-parse --git-dir", "git status --porcelain=v1", "git rev-parse --git-dir"]);
    }

    #[test]
    fn retry_and_continue_require_a_session(operation in prop::sample::select(vec!["--retry", "--continue"])) {
        let diagnostic = if operation == "--retry" { "no active factor session" } else { "--continue requires --message <MSG>" };
        dirty_route(&[operation], EXIT_USAGE, diagnostic, &["git rev-parse --git-dir"]);
    }
}

proptest! {
    #[test]
    fn callback_arity_is_refused_before_any_route(
        begin in any::<bool>(),
        count in prop::sample::select(vec![usize::MIN, 1, 3, 4, 6, 7]),
    ) {
        let command = if begin { "rebase-exec-begin" } else { "rebase-exec-preflight" };
        let diagnostic = if begin { "rebase-exec-begin requires exactly five arguments: current-index, start-head, is-root, exec-command, and commits" } else { "rebase-exec-preflight requires exactly two arguments: current-index and exec-command" };
        let mut arguments = vec![command];
        arguments.extend(repeat_n("argument", count));
        refusal(&arguments, diagnostic);
    }

    #[test]
    fn commit_without_gate_is_refused_before_any_route(commit in "[a-zA-Z][a-zA-Z0-9_-]{0,40}") {
        refusal(&[&commit], "--exec <COMMAND> is required when starting a factor session");
    }

    #[test]
    fn baseline_message_without_an_operation_is_refused(
        gate in "[a-zA-Z][a-zA-Z0-9 ]{0,40}",
        message in "[a-zA-Z][a-zA-Z0-9 ]{0,40}",
    ) {
        refusal(&["--exec", &gate, "--message", &message], "--message can only be used with --continue or --finish");
    }
}

proptest! {
    #[test]
    fn parser_suggestions_preserve_the_exact_stream(flag in prop::sample::select(vec!["--not-iu", "--continu"])) {
        let diagnostic = format!("error: unexpected argument '{flag}' found\n\n  tip: a similar argument exists: '--continue'\n\nUsage: git-factor --continue [COMMIT]...\n\nFor more information, try '--help'.\n");
        parser(&[flag], EXIT_USAGE, "", &diagnostic);
    }
}

proptest! {
    #[test]
    fn finish_message_presence_preserves_the_session_requirement(
        message in prop::option::of("[a-zA-Z][a-zA-Z0-9 ]{0,40}"),
    ) {
        let arguments = message.as_deref().map_or_else(
            || vec!["--finish"],
            |text| vec!["--finish", "--message", text],
        );
        dirty_route(&arguments, EXIT_USAGE, "no active factor session", &["git rev-parse --git-dir"]);
    }
}

proptest! {
    #[test]
    fn continue_without_message_preserves_generated_session_reopen_failures(
        fault in prop_oneof![Just(ReopenFault::GitDirectory), Just(ReopenFault::PhaseEncoding), Just(ReopenFault::PhaseValue)],
        sha in "[0-9a-f]{40}",
        phase_suffix in "[a-z]{1,16}",
        user_bytes in generated_bytes(any::<u8>(), 0..32),
    ) {
        reopen_failure(fault, &sha, &format!("unsupported-{phase_suffix}"), &user_bytes);
    }
}

proptest! {
    #[test]
    fn supplied_commit_starts_generated_selected_pools(
        sha in "[0-9a-f]{40}", revision_suffix in "[a-z]{1,16}",
    ) {
        supplied_commit_success(&sha, &format!("topic-{revision_suffix}"));
    }

    #[test]
    fn message_only_without_exec_preserves_generated_refusals(
        message in "[a-zA-Z][a-zA-Z0-9 ]{0,40}",
    ) {
        refusal(&["--message", &message], "--exec <COMMAND> is required when starting a factor session");
    }
}

proptest! {
    #[test]
    fn supplied_commit_preparation_preserves_generated_dirty_refusals(
        revision_suffix in "[a-z]{1,16}",
    ) {
        dirty_route(
            &["--exec", "true", &format!("topic-{revision_suffix}")],
            EXIT_SOFTWARE,
            "git command failed: working tree must be clean before starting; stash, commit, or remove local changes\nSTATUS:\n M unrelated",
            &["git rev-parse --git-dir", "git rev-parse --git-dir", "git status --porcelain=v1", "git rev-parse --git-dir"],
        );
    }

    #[test]
    fn supplied_commit_resolution_preserves_generated_rejections(
        revision_suffix in "[a-z]{1,16}",
        rejected_exit in RangeInclusive::<u8>::new(1, u8::MAX),
        user_bytes in generated_bytes(any::<u8>(), 0..32),
    ) {
        supplied_commit_failure(&format!("topic-{revision_suffix}"), rejected_exit, &user_bytes);
    }

    #[test]
    fn public_inactive_status_preserves_unrelated_work(
        body in string_regex("[a-z]{0,24}").or_abort("inactive user bytes strategy"),
    ) {
        verify_public_inactive_status(&body);
    }
}

proptest! {
    #[test]
    fn public_abort_reports_generated_reset_identity_and_rebase_observation(
        in_progress in any::<bool>(), sha in "[0-9a-f]{40}",
    ) {
        verify_public_abort(in_progress, &sha);
    }
}

proptest! {
    #[test]
    fn abort_messages_preserve_generated_dispatch_refusals(
        message in "[a-zA-Z][a-zA-Z0-9 ]{0,40}",
    ) {
        refusal(
            &["--abort", "--message", &message],
            "--abort cannot be combined with other options",
        );
    }
}

proptest! {
    #[test]
    fn public_finish_generated_reconstructed_message_and_tree(
        case in prop::sample::select(vec![
            FinishCase::Supplied,
            FinishCase::Original,
            FinishCase::Empty,
            FinishCase::Fallback,
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_native_and_root_completion(
        case in prop::sample::select(vec![
            FinishCase::RebaseComplete,
            FinishCase::RootComplete,
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_session_admission(
        case in prop::sample::select(vec![
            FinishCase::Absent,
            FinishCase::ReopenQuery,
            FinishCase::Pending,
            FinishCase::InvalidPhase,
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_saved_commit_selection(
        case in prop::sample::select(vec![
            FinishCase::InvalidCommits,
            FinishCase::InvalidIndex,
            FinishCase::OutsideIndex,
            FinishCase::InvalidRequires,
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_required_rebase_and_tree(
        case in prop::sample::select(vec![
            FinishCase::RebaseMissing,
            FinishCase::InvalidExpected,
            FinishCase::InvalidActual,
            FinishCase::UnequalTree,
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_message_metadata_and_count(
        case in prop::sample::select(vec![
            FinishCase::EmptyOriginal,
            FinishCase::InvalidMetadata,
            FinishCase::InvalidCount,
            FinishCase::Overflow,
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_count_and_cleanup_durability(
        case in prop::sample::select(vec![
            FinishCase::WriteCount,
            FinishCase::RemoveDenied,
            FinishCase::RemoveRetained,
            FinishCase::RebaseRetained,
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_session_and_selection_reads(
        case in prop::sample::select(vec![
            FinishCase::Read("commits", 1),
            FinishCase::Read("phase", 1),
            FinishCase::Read("requires_rebase", 1),
            FinishCase::Read("current_index", 1),
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_baseline_and_count_reads(
        case in prop::sample::select(vec![
            FinishCase::Read("current_index", 2),
            FinishCase::Read("expected_tree", 1),
            FinishCase::Read("split_count", 1),
            FinishCase::Read("split_count", 2),
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_advance_policy_read(
        case in prop::sample::select(vec![
            FinishCase::Read("requires_rebase", 2),
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_final_output_writes(
        case in prop::sample::select(vec![
            FinishCase::Output(1),
            FinishCase::Output(2),
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_checkout_and_clean_requests(
        case in prop::sample::select(vec![
            FinishCase::Process(FinishStage::Checkout, true),
            FinishCase::Process(FinishStage::Checkout, false),
            FinishCase::Process(FinishStage::Clean, true),
            FinishCase::Process(FinishStage::Clean, false),
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_restore_and_original_message_requests(
        case in prop::sample::select(vec![
            FinishCase::Process(FinishStage::Restore, true),
            FinishCase::Process(FinishStage::Restore, false),
            FinishCase::Process(FinishStage::OriginalMessage, true),
            FinishCase::Process(FinishStage::OriginalMessage, false),
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_expected_and_actual_tree_queries(
        case in prop::sample::select(vec![
            FinishCase::Process(FinishStage::ExpectedTree, true),
            FinishCase::Process(FinishStage::ExpectedTree, false),
            FinishCase::Process(FinishStage::WriteTree, true),
            FinishCase::Process(FinishStage::WriteTree, false),
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_staged_diff_and_metadata_requests(
        case in prop::sample::select(vec![
            FinishCase::Process(FinishStage::StagedDiff, true),
            FinishCase::Process(FinishStage::StagedDiff, false),
            FinishCase::Process(FinishStage::Metadata, true),
            FinishCase::Process(FinishStage::Metadata, false),
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_commit_and_native_advance_requests(
        case in prop::sample::select(vec![
            FinishCase::Process(FinishStage::Commit, true),
            FinishCase::Process(FinishStage::Commit, false),
            FinishCase::Process(FinishStage::Rebase, true),
            FinishCase::Process(FinishStage::Rebase, false),
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

    #[test]
    fn public_finish_generated_root_cleanup_queries(
        case in prop::sample::select(vec![
            FinishCase::Process(FinishStage::Root, true),
            FinishCase::Process(FinishStage::Root, false),
        ]),
        sha in "[0-9a-f]{40}",
        previous in RangeInclusive::<u8>::new(0, 254),
        subject in "[A-Za-z][A-Za-z 0-9]{0,20}",
        body in "[A-Za-z][A-Za-z 0-9]{0,30}",
        user in generated_bytes(any::<u8>(), 0..64),
    ) {
        verify_finish(case, &sha, previous, &subject, &body, &user);
    }

}
