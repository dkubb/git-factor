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
