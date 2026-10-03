mod real_runner {
    mod runner {
        mod status {
            use std::fs;
            use std::io;

            use proptest::prelude::*;

            use crate::test_support::OrAbort as _;

            use super::super::super::super::status_contracts::{self, Launch};

            const CHILD_SUCCESS: i32 = 0;

            proptest! {
                #[test]
                fn preserves_streams_status_arguments_environment_and_directory(
                    quiet in any::<bool>(),
                    exit in u8::MIN..=7,
                    payload in "[a-zA-Z0-9 $;]{0,24}",
                    value in "[a-zA-Z0-9 $;]{0,24}",
                ) {
                    let root = tempfile::tempdir().or_abort("status contract arrangement");
                    let cwd = fs::canonicalize(root.path()).or_abort("status contract arrangement");

                    let actual = status_contracts::observe(
                        root.path(), Launch::Shell, quiet, exit, &payload, &value
                    );

                    prop_assert_eq!(actual.status.code(), Some(CHILD_SUCCESS));
                    let stdout = "\nrunning 1 test\n";
                    let stderr = if quiet {
                        String::new()
                    } else {
                        format!("stdout:<{payload}>\nstderr:<{payload}>\n")
                    };
                    prop_assert_eq!(actual.stdout, stdout.as_bytes());
                    prop_assert_eq!(actual.stderr, stderr.as_bytes());
                    let receipt = format!("exit:{exit}\n");
                    prop_assert_eq!(status_contracts::receipt(root.path()), receipt.as_bytes());
                    let native = format!(
                        concat!(
                            "command:<status-contract>\nargument:<{}>\n",
                            "environment:<{}>\n{}\n"
                        ),
                        payload, value, cwd.display()
                    );
                    prop_assert_eq!(
                        status_contracts::native(root.path()).or_abort("status contract arrangement"),
                        native.as_bytes()
                    );
                }

                #[test]
                fn spawn_failures_preserve_not_found_without_native_effects(
                    missing_directory in any::<bool>(),
                    quiet in any::<bool>(),
                    payload in "[a-zA-Z0-9 $;]{0,24}",
                ) {
                    let root = tempfile::tempdir().or_abort("status contract arrangement");
                    let launch = if missing_directory {
                        Launch::MissingDirectory
                    } else {
                        Launch::MissingExecutable
                    };

                    let actual = status_contracts::observe(
                        root.path(), launch, quiet, 0, &payload, "unused"
                    );

                    prop_assert_eq!(actual.status.code(), Some(CHILD_SUCCESS));
                    prop_assert_eq!(actual.stdout, b"\nrunning 1 test\n");
                    prop_assert_eq!(actual.stderr, b"");
                    prop_assert_eq!(status_contracts::receipt(root.path()), b"error:NotFound\n");
                    prop_assert_eq!(
                        status_contracts::native(root.path())
                            .err().or_abort("status contract arrangement")
                            .kind(),
                        io::ErrorKind::NotFound
                    );
                    prop_assert_eq!(
                        fs::read_dir(root.path()).or_abort("status contract arrangement").count(),
                        1
                    );
                }
            }
        }
    }
}
