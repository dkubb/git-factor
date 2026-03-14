//! Shared integration-test harness for `git-factor` and `git-sequence-editor`.

#![forbid(unsafe_code)]
#![expect(
    dead_code,
    reason = "shared helpers are consumed differently by each integration test target"
)]
#![expect(
    unreachable_pub,
    reason = "shared integration fixtures are exported within the tests crate namespace"
)]
#![expect(
    clippy::implicit_return,
    reason = "integration support helpers are intentionally narrow scenario utilities"
)]
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

use assert_cmd::prelude::*;
use tempfile::TempDir;

pub const EXIT_OK: i32 = 0;
pub const EXIT_USAGE: i32 = 64;
pub const EXIT_DATAERR: i32 = 65;
pub const EXIT_SOFTWARE: i32 = 70;
pub const EXIT_TEMPFAIL: i32 = 75;
pub const EXIT_FAILURE: i32 = 1;

pub trait CommandExpectation {
    fn expected_code(&self) -> i32;
    fn expected_stderr(&self) -> &str;
    fn expected_stdout(&self) -> &str;
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OrderedGitReplayExpectation {
    expected_invocations: Option<usize>,
    invocation_count_file: Option<PathBuf>,
}

impl OrderedGitReplayExpectation {
    pub fn assert_consumed(&self) -> Result<(), String> {
        match (
            self.invocation_count_file.as_ref(),
            self.expected_invocations,
        ) {
            (Some(path), Some(expected_count)) => {
                let parsed = fs::read_to_string(path)
                    .unwrap_or_else(|_| "0".to_owned())
                    .parse::<usize>();
                let actual = match parsed {
                    Ok(value) => value,
                    Err(err) => return Err(format!("failed to parse invocation count: {err}")),
                };
                if actual != expected_count {
                    return Err(format!(
                        "ordered git invocation count mismatch: expected {expected_count}, got {actual}"
                    ));
                }
                Ok(())
            }
            (None, None) => Ok(()),
            _ => Err(
                "ordered replay expectation must set both count file and expected invocation count"
                    .to_owned(),
            ),
        }
    }

    pub const fn from_count_file(path: PathBuf, expected_invocations: usize) -> Self {
        Self {
            expected_invocations: Some(expected_invocations),
            invocation_count_file: Some(path),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GitWrapperStep {
    pub args: Vec<String>,
    pub exit_code: i32,
    pub stderr: String,
    pub stdout: String,
}

fn must_ok<T, E>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|_| process::abort())
}

fn must_some<T>(value: Option<T>) -> T {
    value.unwrap_or_else(|| process::abort())
}

pub fn git_factor_bin() -> PathBuf {
    assert_cmd::cargo::cargo_bin!("git-factor").to_path_buf()
}

pub fn git(repo: &Path, args: &[&str]) -> String {
    let output = must_ok(
        Command::new("/usr/bin/git")
            .args(args)
            .current_dir(repo)
            .output(),
    );

    assert!(
        output.status.success(),
        "git {:?} failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        args,
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub fn git_status_porcelain(repo: &Path) -> String {
    git(repo, &["status", "--porcelain"])
}

pub fn git_dir(repo: &Path) -> PathBuf {
    let dir = git(repo, &["rev-parse", "--git-dir"]);
    repo.join(dir)
}

pub fn write_file(repo: &Path, rel: &str, content: &str) {
    let path = repo.join(rel);
    must_ok(fs::write(&path, content));
}

pub fn init_repo() -> TempDir {
    let dir = must_ok(TempDir::new());
    let repo = dir.path();

    git(repo, &["init"]);
    git(repo, &["config", "user.name", "Git Factor Tests"]);
    git(
        repo,
        &["config", "user.email", "git-factor-tests@example.invalid"],
    );
    // Ensure local tests are not affected by user/global git hook configuration.
    git(repo, &["config", "core.hooksPath", ".git/hooks"]);

    dir
}

pub fn commit_file(repo: &Path, rel: &str, content: &str, message: &str) {
    must_ok(fs::write(repo.join(rel), content));
    git(repo, &["add", rel]);
    git(repo, &["commit", "--message", message]);
}

pub fn start_session(repo: &Path) {
    Command::new(git_factor_bin())
        .current_dir(repo)
        .args(["--exec", "true", "HEAD"])
        .assert()
        .success();
}
