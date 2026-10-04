mod runner {
    mod output {
        use super::super::super::super::runner_observation::{Observation, Request};
        use super::super::super::super::{ActorRunner, Runner as _};
        use crate::test_support::OrAbort as _;
        const CHILD_EXIT: i32 = 7;
        #[test]
        fn delegates_the_exact_native_request_and_removes_inherited_routes_first() {
            let root = tempfile::tempdir().or_abort("actor runner fixture");
            let observer = Observation::default();
            let actor = ActorRunner { inner: &observer };
            let args = [
                "-c",
                "printf '%s' \"$VALUE\"; printf child-stderr >&2; exit 7",
            ];
            let environment = [
                ("VALUE", Some("literal $;")),
                ("GIT_INDEX_FILE", Some("trusted-private-index")),
            ];
            let expected_environment = [
                ("GIT_COMMON_DIR", None),
                ("GIT_DIR", None),
                ("GIT_INDEX_FILE", None),
                ("GIT_WORK_TREE", None),
                ("VALUE", Some("literal $;")),
                ("GIT_INDEX_FILE", Some("trusted-private-index")),
            ];
            let expected = Request::new("/bin/sh", &args, &expected_environment, None, root.path());
            let actual = actor
                .output("/bin/sh", &args, &environment, root.path())
                .or_abort("native output");
            assert_eq!(actual.status.code(), Some(CHILD_EXIT));
            assert_eq!(actual.stdout, b"literal $;");
            assert_eq!(actual.stderr, b"child-stderr");
            assert_eq!(observer.requests(), vec![expected]);
        }
    }
    mod status {
        use super::super::super::super::runner_observation::{Observation, Request};
        use super::super::super::super::{ActorRunner, Runner as _};
        use crate::test_support::OrAbort as _;
        use std::fs;
        const CHILD_EXIT: i32 = 7;
        #[test]
        fn delegates_native_status_with_exact_quiet_directory_and_assignments() {
            let root = tempfile::tempdir().or_abort("actor runner fixture");
            let observer = Observation::default();
            let actor = ActorRunner { inner: &observer };
            let args = ["-c", "printf '%s' \"$VALUE\" > observed; exit 7"];
            let environment = [("VALUE", Some("literal $;"))];
            let expected_environment = [
                ("GIT_COMMON_DIR", None),
                ("GIT_DIR", None),
                ("GIT_INDEX_FILE", None),
                ("GIT_WORK_TREE", None),
                ("VALUE", Some("literal $;")),
            ];
            let expected = Request::new(
                "/bin/sh",
                &args,
                &expected_environment,
                Some(true),
                root.path(),
            );
            let actual = actor
                .status("/bin/sh", &args, &environment, true, root.path())
                .or_abort("native status");
            assert_eq!(actual.code(), Some(CHILD_EXIT));
            assert_eq!(
                fs::read(root.path().join("observed")).or_abort("child bytes"),
                b"literal $;"
            );
            assert_eq!(observer.requests(), vec![expected]);
        }
    }
}
