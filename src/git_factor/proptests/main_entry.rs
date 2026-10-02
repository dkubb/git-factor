use super::*;
use crate::git_factor::tests::start_contracts::replay;
use crate::git_factor::tests::start_contracts::{DirectStart, GateCase, query};
use core::num::NonZeroUsize;
use core::ops::RangeInclusive;

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
                    prop_oneof![Just(query::QueryPosition::First), Just(query::QueryPosition::Last)]
                        .prop_map(query::QueryTarget::MergeParent),
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
