mod real_runner {
    mod runner {
        mod status {
            use std::fs;
            use std::io;

            use crate::test_support::OrAbort as _;

            use super::super::super::super::status_contracts::{self, Launch};

            const CHILD_SUCCESS: i32 = 0;

            #[test]
            fn inherits_both_streams() {
                status_contracts::run_child_if_requested();
                let root = tempfile::tempdir().or_abort("status contract arrangement");
                let cwd = fs::canonicalize(root.path()).or_abort("status contract arrangement");

                let actual = status_contracts::observe(
                    root.path(),
                    Launch::Shell,
                    false,
                    7,
                    "a $; b",
                    "v $; z",
                );

                assert_eq!(actual.status.code(), Some(CHILD_SUCCESS));
                assert_eq!(actual.stdout, b"\nrunning 1 test\nstdout:<a $; b>\n");
                assert_eq!(actual.stderr, b"stderr:<a $; b>\n");
                assert_eq!(status_contracts::receipt(root.path()), b"exit:7\n");
                assert_eq!(
                    status_contracts::native(root.path()).or_abort("status contract arrangement"),
                    format!(
                        concat!(
                            "command:<status-contract>\nargument:<a $; b>\n",
                            "environment:<v $; z>\n{}\n"
                        ),
                        cwd.display()
                    )
                    .as_bytes()
                );
            }

            #[test]
            fn quiet_discards_both_streams() {
                let root = tempfile::tempdir().or_abort("status contract arrangement");
                let cwd = fs::canonicalize(root.path()).or_abort("status contract arrangement");

                let actual = status_contracts::observe(
                    root.path(),
                    Launch::Shell,
                    true,
                    0,
                    "quiet",
                    "value",
                );

                assert_eq!(actual.status.code(), Some(CHILD_SUCCESS));
                assert_eq!(actual.stdout, b"\nrunning 1 test\n");
                assert_eq!(actual.stderr, b"");
                assert_eq!(status_contracts::receipt(root.path()), b"exit:0\n");
                assert_eq!(
                    status_contracts::native(root.path()).or_abort("status contract arrangement"),
                    format!(
                        concat!(
                            "command:<status-contract>\nargument:<quiet>\n",
                            "environment:<value>\n{}\n"
                        ),
                        cwd.display()
                    )
                    .as_bytes()
                );
            }

            #[test]
            fn missing_executable_has_no_native_effects() {
                let root = tempfile::tempdir().or_abort("status contract arrangement");

                let actual = status_contracts::observe(
                    root.path(),
                    Launch::MissingExecutable,
                    false,
                    0,
                    "unused",
                    "unused",
                );

                assert_eq!(actual.status.code(), Some(CHILD_SUCCESS));
                assert_eq!(actual.stdout, b"\nrunning 1 test\n");
                assert_eq!(actual.stderr, b"");
                assert_eq!(status_contracts::receipt(root.path()), b"error:NotFound\n");
                assert_eq!(
                    status_contracts::native(root.path())
                        .err()
                        .or_abort("status contract arrangement")
                        .kind(),
                    io::ErrorKind::NotFound
                );
                assert_eq!(
                    fs::read_dir(root.path())
                        .or_abort("status contract arrangement")
                        .count(),
                    1
                );
            }
        }
    }
}
