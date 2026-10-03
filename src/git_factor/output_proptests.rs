mod completed {
    use super::super::*;
    use core::ops::RangeInclusive;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn reports_generated_positive_count_and_command(
            finish in any::<bool>(), count in RangeInclusive::<u8>::new(1, u8::MAX),
        ) {
            let operation = if finish { CompletionOperation::Finish } else { CompletionOperation::Continue };
            let positive_count = NonZeroU8::new(count).or_abort("generated positive completion count");
            tests::verify_completed(operation, positive_count);
        }
    }

    use crate::test_support::OrAbort as _;
}

mod aborted {
    use super::super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn reports_generated_native_rebase_observation(in_progress in any::<bool>()) {
            tests::verify_aborted(in_progress);
        }
    }
}

mod session_status {
    mod new {
        use super::super::super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn preserves_checked_generated_facts(
                index in any::<usize>(), count in any::<u8>(), pending in any::<bool>(),
                required in any::<bool>(), root in any::<bool>(), progress in any::<bool>(),
            ) {
                tests::verify_status(Some((index, count,
                    if pending { SessionPhase::PendingStart } else { SessionPhase::Splitting },
                    required, root, progress)));
            }
        }
    }
}

mod status {
    use super::super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn preserves_generated_result_and_write_boundaries(
            active in any::<bool>(), index in any::<usize>(), count in any::<u8>(), pending in any::<bool>(),
            required in any::<bool>(), root in any::<bool>(), progress in any::<bool>(),
        ) {
            tests::verify_status(active.then_some((index, count,
                if pending { SessionPhase::PendingStart } else { SessionPhase::Splitting },
                required, root, progress)));
        }
    }
}
