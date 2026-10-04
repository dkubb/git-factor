mod runner {
    mod output {
        use super::super::super::super::runner_observation::{Observation, Request};
        use super::super::super::super::{ActorRunner, Runner as _};
        use crate::test_support::OrAbort as _;
        use proptest::prelude::*;
        const LAST_EXIT: u8 = 7;
        proptest! {
            #[test]
            fn preserves_generated_native_output_and_ordered_overrides(value in "[A-Za-z0-9 $;]{0,24}", exit in u8::MIN..=LAST_EXIT, private in any::<bool>()) {
                let root = tempfile::tempdir().or_abort("actor runner fixture");
                let observer = Observation::default();
                let actor = ActorRunner { inner: &observer };
                let code = exit.to_string();
                let args = ["-c", "printf '%s' \"$VALUE\"; printf '%s' \"$VALUE\" >&2; exit \"$CODE\""];
                let index = private.then_some("trusted-private-index");
                let environment = [("VALUE", Some(value.as_str())), ("CODE", Some(code.as_str())), ("GIT_INDEX_FILE", index)];
                let expected_environment = [("GIT_COMMON_DIR", None), ("GIT_DIR", None),
                        ("GIT_INDEX_FILE", None), ("GIT_WORK_TREE", None),
                        ("VALUE", Some(value.as_str())), ("CODE", Some(code.as_str())),
                        ("GIT_INDEX_FILE", index)];
                let expected = Request::new("/bin/sh", &args, &expected_environment, None, root.path());
                let actual = actor.output("/bin/sh", &args, &environment, root.path()).or_abort("native output");
                prop_assert_eq!(actual.status.code(), Some(i32::from(exit)));
                prop_assert_eq!(actual.stdout, value.as_bytes());
                prop_assert_eq!(actual.stderr, value.as_bytes());
                prop_assert_eq!(observer.requests(), vec![expected]);
            }
        }
    }
    mod status {
        use super::super::super::super::runner_observation::{Observation, Request};
        use super::super::super::super::{ActorRunner, Runner as _};
        use crate::test_support::OrAbort as _;
        use proptest::prelude::*;
        use std::fs;
        const LAST_EXIT: u8 = 7;
        proptest! {
            #[test]
            fn preserves_generated_native_status_directory_quiet_and_assignments(value in "[A-Za-z0-9 $;]{0,24}", exit in u8::MIN..=LAST_EXIT, quiet in any::<bool>()) {
                let root = tempfile::tempdir().or_abort("actor runner fixture");
                let observer = Observation::default();
                let actor = ActorRunner { inner: &observer };
                let code = exit.to_string();
                let args = ["-c", "printf '%s' \"$VALUE\" > observed; exit \"$CODE\""];
                let environment = [("VALUE", Some(value.as_str())), ("CODE", Some(code.as_str()))];
                let expected_environment = [("GIT_COMMON_DIR", None), ("GIT_DIR", None),
                        ("GIT_INDEX_FILE", None), ("GIT_WORK_TREE", None),
                        ("VALUE", Some(value.as_str())), ("CODE", Some(code.as_str()))];
                let expected = Request::new("/bin/sh", &args, &expected_environment, Some(quiet), root.path());
                let actual = actor.status("/bin/sh", &args, &environment, quiet, root.path()).or_abort("native status");
                prop_assert_eq!(actual.code(), Some(i32::from(exit)));
                prop_assert_eq!(fs::read(root.path().join("observed")).or_abort("child bytes"), value.as_bytes());
                prop_assert_eq!(observer.requests(), vec![expected]);
            }
        }
    }
}
