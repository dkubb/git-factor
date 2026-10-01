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
