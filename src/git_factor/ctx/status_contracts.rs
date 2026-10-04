use std::env;
use std::fs;
use std::io;
use std::path::Path;
use std::process::{self, Command, Output};

use crate::test_support::OrAbort as _;

use super::{RealRunner, Runner as _};

const CHILD_ROOT: &str = "GIT_FACTOR_STATUS_CONTRACT_ROOT";
const ENTRY: &str = "git_factor::ctx::tests::real_runner::runner::status::inherits_both_streams";
const SCRIPT: &str = concat!(
    "printf 'stdout:<%s>\\n' \"$1\"; ",
    "printf 'stderr:<%s>\\n' \"$1\" >&2; ",
    "printf 'command:<%s>\\nargument:<%s>\\nenvironment:<%s>\\n' ",
    "\"$0\" \"$1\" \"$GIT_FACTOR_STATUS_VALUE\" > native; ",
    "pwd -P >> native; exit \"$2\""
);

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::ctx) enum Launch {
    MissingDirectory,
    MissingExecutable,
    Shell,
}

impl Launch {
    const fn name(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::MissingExecutable => "missing-executable",
            Self::MissingDirectory => "missing-directory",
        }
    }
}

/// This branch is consumed only by the parent contracts below. It records actual outcomes,
/// then omits libtest's timing-dependent epilogue; it contains no parent oracle.
#[expect(
    clippy::exit,
    reason = "the owned child records its outcome before ending the test image"
)]
#[expect(
    clippy::single_call_fn,
    reason = "one owned test entry isolates the actor from the parent unit and property oracles"
)]
pub(in crate::git_factor::ctx) fn run_child_if_requested() {
    let Some(root) = env::var_os(CHILD_ROOT) else {
        return;
    };
    let owned_root = Path::new(&root);
    let payload = env::var("GIT_FACTOR_STATUS_PAYLOAD").or_abort("status contract arrangement");
    let value =
        env::var("GIT_FACTOR_STATUS_EXPECTED_VALUE").or_abort("status contract arrangement");
    let exit = env::var("GIT_FACTOR_STATUS_EXIT").or_abort("status contract arrangement");
    let quiet_input = env::var("GIT_FACTOR_STATUS_QUIET").or_abort("status contract arrangement");
    let quiet = match quiet_input.as_str() {
        "true" => true,
        "false" => false,
        _ => process::abort(),
    };
    let missing = owned_root.join("missing");
    let missing_bin = owned_root.join("missing-executable");
    let launch = env::var("GIT_FACTOR_STATUS_LAUNCH").or_abort("status contract arrangement");
    let (bin, cwd) = match launch.as_str() {
        "shell" => ("/bin/sh", owned_root),
        "missing-executable" => (
            missing_bin
                .to_str()
                .or_abort("owned executable path is UTF-8"),
            owned_root,
        ),
        "missing-directory" => ("/bin/sh", missing.as_path()),
        _ => process::abort(),
    };
    let actual = RealRunner.status(
        bin,
        &["-c", SCRIPT, "status-contract", &payload, &exit],
        &[("GIT_FACTOR_STATUS_VALUE", Some(value.as_str()))],
        quiet,
        cwd,
    );
    let receipt = match actual {
        Ok(status) => {
            let code = status
                .code()
                .or_abort("the owned shell exits without a signal");
            format!("exit:{code}\n")
        }
        Err(error) => format!("error:{:?}\n", error.kind()),
    };
    fs::write(owned_root.join("receipt"), receipt).or_abort("status contract arrangement");
    process::exit(0)
}

pub(in crate::git_factor::ctx) fn observe(
    root: &Path,
    launch: Launch,
    quiet: bool,
    exit: u8,
    payload: &str,
    value: &str,
) -> Output {
    Command::new(env::current_exe().or_abort("status contract arrangement"))
        .args([
            "--exact",
            ENTRY,
            "--nocapture",
            "--test-threads=1",
            "--quiet",
        ])
        .env(CHILD_ROOT, root)
        .env("GIT_FACTOR_STATUS_LAUNCH", launch.name())
        .env(
            "GIT_FACTOR_STATUS_QUIET",
            if quiet { "true" } else { "false" },
        )
        .env("GIT_FACTOR_STATUS_EXIT", exit.to_string())
        .env("GIT_FACTOR_STATUS_PAYLOAD", payload)
        .env("GIT_FACTOR_STATUS_EXPECTED_VALUE", value)
        .env_remove("GIT_FACTOR_STATUS_VALUE")
        .output()
        .or_abort("status contract arrangement")
}

pub(in crate::git_factor::ctx) fn receipt(root: &Path) -> Vec<u8> {
    fs::read(root.join("receipt")).or_abort("status contract arrangement")
}

pub(in crate::git_factor::ctx) fn native(root: &Path) -> io::Result<Vec<u8>> {
    fs::read(root.join("native"))
}
