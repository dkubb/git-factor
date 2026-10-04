#[path = "../ctx_remove_atomic_file_tests.rs"]
mod real_fs;

mod real_runner {
    mod runner {
        mod output {
            use super::super::super::*;

            #[test]
            fn removes_routes_and_distinguishes_empty_literal_values() {
                let (directory, home) = arrange_environment();
                let environment = [
                    ("HOME", None),
                    ("FACTOR_VALUE", Some("literal $value; ")),
                    ("FACTOR_EMPTY", Some("")),
                ];
                let observed = REAL_RUNNER.output("/bin/sh", &["-c",
                    "test \"${HOME+x}\" != x && test \"${FACTOR_EMPTY+x}\" = x && printf '%s' \"$FACTOR_VALUE\""],
                    &environment, directory.path()).or_abort("captured environment");
                assert!(observed.status.success());
                assert_eq!(observed.stdout, b"literal $value; ");
                assert!(observed.stderr.is_empty());
                assert_eq!(env::var_os("HOME"), home);
            }
        }
        mod status {
            use super::super::super::*;
            use std::fs;
            use std::io;

            use crate::test_support::OrAbort as _;

            use super::super::super::super::status_contracts::{self, Launch};

            const CHILD_SUCCESS: i32 = 0;

            #[test]
            fn removes_routes_and_preserves_ordered_literal_assignments() {
                let (directory, home) = arrange_environment();
                let environment = [
                    ("HOME", None),
                    ("FACTOR_VALUE", Some("discard")),
                    ("FACTOR_VALUE", None),
                    ("FACTOR_VALUE", Some("literal $value; ")),
                ];
                let observed = REAL_RUNNER
                    .status(
                        "/bin/sh",
                        &[
                            "-c",
                            "test \"${HOME+x}\" != x && printf '%s' \"$FACTOR_VALUE\" > observed",
                        ],
                        &environment,
                        true,
                        directory.path(),
                    )
                    .or_abort("status environment");
                assert!(observed.success());
                assert_eq!(
                    fs::read(directory.path().join("observed")).or_abort("written literal"),
                    b"literal $value; "
                );
                assert_eq!(env::var_os("HOME"), home);
            }

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
                assert_eq!(actual.stdout, b"\nrunning 1 test\n");
                assert_eq!(actual.stderr, b"stdout:<a $; b>\nstderr:<a $; b>\n");
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
