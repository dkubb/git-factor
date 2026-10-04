#[path = "../ctx_remove_atomic_file_proptests.rs"]
mod real_fs;

mod real_runner {
    mod runner {
        mod output {
            use super::super::super::*;
            use proptest::prelude::*;

            proptest! {
                #[test]
                fn preserves_generated_literals_after_removal(value in "[A-Za-z0-9 $;]{0,24}") {
                    let (directory, home) = arrange_environment();
                    let environment = [("HOME", None), ("FACTOR_VALUE", Some(value.as_str())),
                        ("FACTOR_EMPTY", Some(""))];
                    let observed = REAL_RUNNER.output("/bin/sh", &["-c",
                        "test \"${HOME+x}\" != x && test \"${FACTOR_EMPTY+x}\" = x && printf '%s' \"$FACTOR_VALUE\""],
                        &environment, directory.path()).or_abort("captured environment");
                    assert!(observed.status.success());
                    assert_eq!(observed.stdout, value.as_bytes());
                    assert!(observed.stderr.is_empty());
                    assert_eq!(env::var_os("HOME"), home);
                }
            }
        }
        mod status {
            use super::super::super::*;
            use proptest::prelude::*;

            proptest! {
                #[test]
                fn preserves_generated_last_assignment(value in "[A-Za-z0-9 $;]{0,24}", quiet in any::<bool>()) {
                    let (directory, home) = arrange_environment();
                    let environment = [("HOME", None), ("FACTOR_VALUE", Some("discard")),
                        ("FACTOR_VALUE", None), ("FACTOR_VALUE", Some(value.as_str()))];
                    let observed = REAL_RUNNER.status("/bin/sh", &["-c",
                        "test \"${HOME+x}\" != x && printf '%s' \"$FACTOR_VALUE\" > observed"],
                        &environment, quiet, directory.path()).or_abort("status environment");
                    assert!(observed.success());
                    assert_eq!(fs::read(directory.path().join("observed")).or_abort("written literal"),
                        value.as_bytes());
                    assert_eq!(env::var_os("HOME"), home);
                }
            }
            use std::fs;
            use std::io;

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

use super::*;
use crate::test_support::OrAbort as _;
use tempfile::TempDir;

fn arrange_environment() -> (TempDir, Option<OsString>) {
    let directory = TempDir::new().or_abort("environment fixture");
    let home = env::var_os("HOME");
    assert!(
        home.is_some(),
        "inherited-variable removal requires HOME to be present"
    );
    (directory, home)
}
