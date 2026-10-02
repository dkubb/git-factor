use super::*;
use crate::git_factor::tests::abort_contracts;
use crate::git_factor::tests::continue_contracts;
use crate::git_factor::tests::start_contracts::launcher::{LaunchFault, LaunchFixture};
use crate::git_factor::tests::start_contracts::replay;
use crate::git_factor::tests::start_contracts::{DirectStart, GateCase, query};
use crate::git_factor::tests::status_contracts;
use crate::git_factor::tests::verify_public_abort_fallback_refusal;
use crate::git_factor::tests::verify_public_status_phase_refusal;
use core::num::{NonZeroU8, NonZeroUsize};
use core::ops::RangeInclusive;

proptest! {
    #[test]
    fn public_abort_preserves_generated_saved_head_and_rebase_routes(
        sha in "[0-9a-f]{40}", started in any::<bool>(), rebase in any::<bool>(), saved_head in any::<bool>(),
    ) { abort_contracts::successful(&sha, started, rebase, saved_head, 0); }

    #[test]
    fn public_abort_reports_generated_output_refusals_after_cleanup(
        sha in "[0-9a-f]{40}", fail_at in RangeInclusive::<usize>::new(1, 4),
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
        fail_at in RangeInclusive::<usize>::new(0, 16),
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
        "FACTOR: Session aborted for current commit step.\n"
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
        deleted in any::<bool>(),
    ) {
        let fixture = if deleted {
            continue_contracts::Continuation::with_deleted(
                &original, split_count)
        } else {
            continue_contracts::Continuation::new(
                &original, split_count,
                continue_contracts::ContinueCase::Complete)
        };
        let before = fixture.protected_bytes();
        let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

        let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

        prop_assert_eq!(code, EXIT_OK);
        prop_assert_eq!(fixture.stdout(), format!(
            "FACTOR: Complete. Final commit split into {} commits.\n", u16::from(split_count) + 1,
        ));
        prop_assert_eq!(fixture.stderr(), "");
        prop_assert!(!fixture.deleted_exists());
        prop_assert_eq!(fixture.journal(), fixture.expected_journal());
        prop_assert_eq!(fixture.protected_bytes(), before);
        prop_assert_eq!(fixture.full_effect_requests(), fixture.expected_effect_requests());
        prop_assert!(!fixture.session_active());
        let expected_removals = if deleted {
            vec![PathBuf::from("deleted.txt")]
        } else {
            Vec::new()
        };
        prop_assert_eq!(fixture.deleted_requests(), expected_removals);
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
            continue_contracts::ContinueFault::DeletedPaths,
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
}
