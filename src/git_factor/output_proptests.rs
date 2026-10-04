mod file_path {
    mod from {
        mod vec {
            use proptest::collection::vec;
            use proptest::prelude::*;
            proptest! {
                #[test]
                fn preserves_generated_unicode_paths(input in any::<String>()) {
                    let actual = super::super::super::super::FilePath::from(input.as_bytes().to_vec());
                    prop_assert!(matches!(actual, super::super::super::super::FilePath::Text(value) if value == input));
                }
                #[test]
                fn preserves_generated_non_utf8_path_bytes(prefix in vec(u8::MIN..=127, 0..24), suffix in vec(u8::MIN..=127, 0..24)) {
                    let mut input = prefix;
                    input.push(255);
                    input.extend(suffix);
                    let actual = super::super::super::super::FilePath::from(input.clone());
                    prop_assert!(matches!(actual, super::super::super::super::FilePath::Bytes { bytes } if bytes == input), "non-UTF8 bytes must remain exact");
                }
            }
        }
    }
}
mod empty_selection {
    use super::super::tests::verify_empty_selection;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn emits_the_single_refusal_state_and_preserves_output_failures(_input in Just(())) {
            verify_empty_selection();
        }
    }
}

mod aborted {
    use super::super::tests::verify_aborted;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn preserves_generated_rebase_observation_and_output_failures(in_progress in any::<bool>()) {
            verify_aborted(in_progress);
        }
    }
}

mod gate_failed {
    use super::super::tests::{gate_failure_origins, verify_gate_failed};
    use core::num::NonZeroI32;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn preserves_generated_gate_identity_exit_and_recovery_with_output_failures(
            command in "[^\x00]{1,100}", exit_code in any::<NonZeroI32>(),
        ) {
            for origin in gate_failure_origins() {
                verify_gate_failed(&command, exit_code, origin);
            }
        }
    }
}

mod start_recovery {
    use super::super::CommandOperation;
    use super::super::tests::verify_recovery;
    use core::num::NonZeroUsize;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn preserves_recovery_under_generated_output_availability(
            fail_at in prop::sample::select(vec![None, NonZeroUsize::new(1), NonZeroUsize::new(2)]),
        ) {
            for operation in [CommandOperation::Start, CommandOperation::Continue, CommandOperation::Finish] {
                verify_recovery(fail_at, operation);
            }
        }
    }
}

mod checkpoint_counts {
    mod new {
        use super::super::super::tests::verify_checkpoint_counts;
        use core::num::NonZeroU64;
        use proptest::prelude::*;
        proptest! {
            #[test]
            fn preserves_generated_counts(commit_count in any::<NonZeroU64>(), split_count in any::<u64>()) {
                verify_checkpoint_counts(commit_count, split_count);
            }
        }
    }
}
mod checkpoint_completed {
    use super::super::CheckpointCompletionOperation;
    use super::super::tests::verify_checkpoint_completed;
    use core::num::NonZeroU64;
    use proptest::prelude::*;
    use proptest::sample::select;
    proptest! {
        #[test]
        fn preserves_generated_progress_and_output_failures(
            operation in select(vec![
                CheckpointCompletionOperation::Abort,
                CheckpointCompletionOperation::Continue,
                CheckpointCompletionOperation::Finish,
            ]),
            count in any::<NonZeroU64>(),
        ) {
            verify_checkpoint_completed(operation, count);
        }
    }
}
mod inactive_status {
    use super::super::tests::verify_inactive_status;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn emits_null_and_preserves_write_failures(_input in Just(())) {
            verify_inactive_status();
        }
    }
}
mod checkpoint_status {
    use super::super::tests::{CheckpointFacts, checkpoint_phases, verify_checkpoint_status};
    use core::num::NonZeroU64;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn preserves_generated_checkpoint_facts(target in "[0-9a-f]{40}", checkpoint in "[0-9a-f]{40}",
            commit_count in any::<NonZeroU64>(), split_count in any::<u64>(),
            root in any::<bool>(), in_progress in any::<bool>(), phase in prop::sample::select(checkpoint_phases().to_vec())) {
            verify_checkpoint_status(&CheckpointFacts { target:&target,checkpoint:&checkpoint,
                commit_count,split_count,root,in_progress,phase });
        }
    }
}
mod checkpoint_remaining {
    use super::super::CheckpointTransition;
    use super::super::tests::verify_checkpoint_remaining;
    use core::num::NonZeroU64;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn preserves_generated_transitions_and_paths(count in any::<u64>(), committed in any::<NonZeroU64>()) {
            verify_checkpoint_remaining(CheckpointTransition::Committed(committed));
            verify_checkpoint_remaining(CheckpointTransition::Retried(count));
        }
    }
}
mod started_checkpoint {
    use super::super::tests::{
        verify_started_bytes, verify_started_details, verify_started_guidance,
        verify_started_output_failures, verify_started_refusals,
    };
    use core::num::NonZeroUsize;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn preserves_generated_metadata_and_selection_boundaries(path in "[^\x00]{1,40}", message in "[^\x00]{0,100}",
            added in any::<u64>(), deleted in any::<u64>(), resumed in any::<bool>(), count in any::<NonZeroUsize>()) {
            verify_started_details(&path,&message,added,deleted,resumed,count);
            verify_started_bytes(); verify_started_guidance(); verify_started_output_failures(); verify_started_refusals();
        }
    }
}

mod closing_status {
    use super::super::ClosingOutcome;
    use super::super::tests::verify_closing_status;
    use core::num::NonZeroU64;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn preserves_generated_terminal_progress_without_historical_queries(
            abandoned in any::<u64>(), completed in any::<NonZeroU64>(),
            root in any::<bool>(), in_progress in any::<bool>(),
        ) {
            verify_closing_status(ClosingOutcome::Aborted(abandoned), root, in_progress);
            verify_closing_status(ClosingOutcome::Complete(completed), root, in_progress);
        }
    }
}
