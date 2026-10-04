#[path = "tests/main_entry.rs"]
mod main_entry;
#[path = "tests/public_cli.rs"]
pub(in crate::git_factor) mod public_cli;

use alloc::collections::VecDeque;
use core::cell::RefCell;
use core::error::Error;
use core::fmt;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::panic::resume_unwind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;

use super::*;

use crate::git_factor::validation::{resolve_commit_refs, validate_not_merge};
use crate::non_empty_string::NonEmptyString;
use tempfile::TempDir;

const SHA_LEN: usize = COMMIT_SHA_HEX_LEN;
const HEAD_TREEISH: &str = "HEAD^{tree}";

#[derive(Clone, Default)]
pub(in crate::git_factor) struct ScriptedRunner {
    outputs: RefCell<HashMap<String, VecDeque<Output>>>,
    statuses: RefCell<HashMap<String, VecDeque<ExitStatus>>>,
}

impl ScriptedRunner {
    fn output_key(bin: &str, args: &[&str], envs: &[(&str, Option<&str>)], cwd: &Path) -> String {
        let env_key = envs
            .iter()
            .map(|&(key, value)| {
                value.map_or_else(|| key.to_owned(), |assigned| format!("{key}={assigned}"))
            })
            .collect::<Vec<_>>()
            .join("\x1f");
        format!(
            "output\x1f{bin}\x1f{}\x1f{env_key}\x1f{}",
            cwd.display(),
            args.join("\x1f")
        )
    }

    fn status_key(
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        quiet: bool,
        cwd: &Path,
    ) -> String {
        let env_key = envs
            .iter()
            .map(|&(key, value)| {
                value.map_or_else(|| key.to_owned(), |assigned| format!("{key}={assigned}"))
            })
            .collect::<Vec<String>>()
            .join("\x1f");
        format!(
            "status\x1f{bin}\x1f{}\x1f{quiet}\x1f{env_key}\x1f{}",
            cwd.display(),
            args.join("\x1f")
        )
    }

    fn with_output(self, bin: &str, args: &[&str], cwd: &Path, stdout: &str) -> Self {
        self.with_output_status(bin, args, cwd, 0, stdout, "")
    }

    fn with_output_status(
        self,
        bin: &str,
        args: &[&str],
        cwd: &Path,
        code: i32,
        stdout: &str,
        stderr: &str,
    ) -> Self {
        let key = Self::output_key(bin, args, &[], cwd);
        self.outputs
            .borrow_mut()
            .entry(key)
            .or_default()
            .push_back(Output {
                status: exit_status(code),
                stdout: stdout.as_bytes().to_vec(),
                stderr: stderr.as_bytes().to_vec(),
            });
        self
    }

    pub(in crate::git_factor) fn with_status(
        self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        quiet: bool,
        cwd: &Path,
        code: i32,
    ) -> Self {
        let key = Self::status_key(bin, args, envs, quiet, cwd);
        self.statuses
            .borrow_mut()
            .entry(key)
            .or_default()
            .push_back(exit_status(code));
        self
    }
}

impl Runner for ScriptedRunner {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        let key = Self::output_key(bin, args, envs, cwd);
        if let Some(output) = self
            .outputs
            .borrow_mut()
            .get_mut(&key)
            .and_then(VecDeque::pop_front)
        {
            return Ok(output);
        }
        if envs.is_empty() && bin == "git" && args == ["rev-parse", "--git-dir"] {
            return Ok(Output {
                status: exit_status(0),
                stdout: b".git\n".to_vec(),
                stderr: Vec::new(),
            });
        }
        Err(io::Error::other(format!("unexpected output call: {key}")))
    }

    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        let key = Self::status_key(bin, args, envs, quiet, cwd);
        self.statuses
            .borrow_mut()
            .get_mut(&key)
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| io::Error::other(format!("unexpected status call: {key}")))
    }
}

#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "sibling canonical validation tests construct the shared root env fixture"
)]
pub(in crate::git_factor) struct TestEnv {
    pub(in crate::git_factor) cwd: PathBuf,
}

impl Env for TestEnv {
    fn current_dir(&self) -> io::Result<PathBuf> {
        Ok(self.cwd.clone())
    }

    fn current_exe(&self) -> io::Result<PathBuf> {
        Ok(self.cwd.join("git-factor"))
    }

    fn var_os(&self, _key: &str) -> Option<OsString> {
        None
    }
}

struct TraceLogEnv {
    cwd: PathBuf,
    trace_log: PathBuf,
}

impl Env for TraceLogEnv {
    fn current_dir(&self) -> io::Result<PathBuf> {
        Ok(self.cwd.clone())
    }

    fn current_exe(&self) -> io::Result<PathBuf> {
        Ok(self.cwd.join("git-factor"))
    }

    fn var_os(&self, key: &str) -> Option<OsString> {
        (key == TRACE_LOG_ENV).then(|| self.trace_log.clone().into_os_string())
    }
}

#[derive(Default)]
pub(in crate::git_factor) struct TestIo {
    stderr: Mutex<String>,
    stdout: Mutex<String>,
}

impl TestIo {
    fn stderr(&self) -> String {
        self.stderr.lock().or_abort("stderr lock").clone()
    }

    fn stdout(&self) -> String {
        self.stdout.lock().or_abort("stdout lock").clone()
    }
}

impl Io for TestIo {
    fn err(&self, text: &str) -> io::Result<()> {
        self.stderr
            .lock()
            .map_err(|_err| io::Error::other("stderr lock poisoned"))?
            .push_str(text);
        Ok(())
    }

    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(line)?;
        self.err("\n")
    }

    fn out(&self, text: &str) -> io::Result<()> {
        self.stdout
            .lock()
            .map_err(|_err| io::Error::other("stdout lock poisoned"))?
            .push_str(text);
        Ok(())
    }

    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }
}

struct FailingIo;

impl Io for FailingIo {
    fn err(&self, _text: &str) -> io::Result<()> {
        Err(io::Error::other("io fail"))
    }

    fn errln(&self, _line: &str) -> io::Result<()> {
        Err(io::Error::other("io fail"))
    }

    fn out(&self, _text: &str) -> io::Result<()> {
        Err(io::Error::other("io fail"))
    }

    fn outln(&self, _line: &str) -> io::Result<()> {
        Err(io::Error::other("io fail"))
    }
}

struct NthIoFailure {
    calls: Mutex<usize>,
    fail_at: usize,
}

impl NthIoFailure {
    fn maybe_fail(&self) -> io::Result<()> {
        let mut calls = self
            .calls
            .lock()
            .map_err(|error| io::Error::other(format!("io calls lock: {error}")))?;
        *calls = calls.checked_add(1).or_abort("counter should not overflow");
        if *calls == self.fail_at {
            Err(io::Error::other("io fail"))
        } else {
            Ok(())
        }
    }

    fn new(fail_at: usize) -> Self {
        Self {
            calls: Mutex::new(0),
            fail_at,
        }
    }
}

impl Io for NthIoFailure {
    fn err(&self, _text: &str) -> io::Result<()> {
        self.maybe_fail()
    }

    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(line)?;
        self.err("\n")
    }

    fn out(&self, _text: &str) -> io::Result<()> {
        self.maybe_fail()
    }

    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }
}

struct DefaultOutlnFailingIo;

impl Io for DefaultOutlnFailingIo {
    fn err(&self, _text: &str) -> io::Result<()> {
        Ok(())
    }

    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(line)?;
        self.err("\n")
    }

    fn out(&self, text: &str) -> io::Result<()> {
        if text == "io fail" {
            Err(io::Error::other("io fail"))
        } else {
            Ok(())
        }
    }

    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }
}

#[derive(Default)]
struct DefaultLineIo {
    stderr: Mutex<String>,
    stdout: Mutex<String>,
}

impl DefaultLineIo {
    fn stderr(&self) -> String {
        self.stderr.lock().or_abort("stderr lock").clone()
    }

    fn stdout(&self) -> String {
        self.stdout.lock().or_abort("stdout lock").clone()
    }
}

impl Io for DefaultLineIo {
    fn err(&self, text: &str) -> io::Result<()> {
        self.stderr
            .lock()
            .map_err(|_err| io::Error::other("stderr lock poisoned"))?
            .push_str(text);
        Ok(())
    }

    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(&format!("{line}\n"))
    }

    fn out(&self, text: &str) -> io::Result<()> {
        self.stdout
            .lock()
            .map_err(|_err| io::Error::other("stdout lock poisoned"))?
            .push_str(text);
        Ok(())
    }

    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(&format!("{line}\n"))
    }
}

struct DefaultErrlnFailingIo;

impl Io for DefaultErrlnFailingIo {
    fn err(&self, text: &str) -> io::Result<()> {
        if text == "io fail" {
            Err(io::Error::other("io fail"))
        } else {
            Ok(())
        }
    }

    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(line)?;
        self.err("\n")
    }

    fn out(&self, _text: &str) -> io::Result<()> {
        Ok(())
    }

    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }
}

#[derive(Default)]
struct FailingEnv {
    message: &'static str,
}

impl Env for FailingEnv {
    fn current_dir(&self) -> io::Result<PathBuf> {
        Err(io::Error::other(self.message))
    }

    fn current_exe(&self) -> io::Result<PathBuf> {
        Err(io::Error::other(self.message))
    }

    fn var_os(&self, _key: &str) -> Option<OsString> {
        None
    }
}

fn with_git_dir_outputs(mut runner: ScriptedRunner, repo: &Path, count: usize) -> ScriptedRunner {
    for _ in 0..count {
        runner = runner.with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    }
    runner
}

#[cfg(unix)]
fn exit_status(code: i32) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt as _;
    ExitStatus::from_raw(code)
}

#[test]
fn repo_status_stdout_returns_exact_porcelain_stdout() {
    let dir = TempDir::new().or_abort("tempdir");
    let env = TestEnv {
        cwd: dir.path().to_path_buf(),
    };
    let io = TestIo::default();
    let runner = ScriptedRunner::default().with_output_status(
        "git",
        &["status", "--porcelain=v1"],
        dir.path(),
        0,
        "M  src/git_factor.rs\n",
        "",
    );
    let ctx = Ctx {
        cwd: dir.path().to_path_buf(),
        env: &env,
        runner: &runner,
        io: &io,
        fs: &REAL_FS,
    };

    let stdout = repo_status_stdout(&ctx).or_abort("repo status should succeed");

    assert_eq!(stdout, "M  src/git_factor.rs\n");
}

#[test]
fn repo_status_stdout_rejects_nonempty_stderr() {
    let dir = TempDir::new().or_abort("tempdir");
    let env = TestEnv {
        cwd: dir.path().to_path_buf(),
    };
    let io = TestIo::default();
    let runner = ScriptedRunner::default().with_output_status(
        "git",
        &["status", "--porcelain=v1"],
        dir.path(),
        0,
        "",
        "warning: odd status output\n",
    );
    let ctx = Ctx {
        cwd: dir.path().to_path_buf(),
        env: &env,
        runner: &runner,
        io: &io,
        fs: &REAL_FS,
    };

    let err = repo_status_stdout(&ctx).err_or_abort("stderr should be rejected");

    assert_eq!(
        err.to_string(),
        "git command failed: git status --porcelain=v1 produced unexpected output (exit 0)\nSTDERR:\nwarning: odd status output"
    );
}

#[test]
fn repo_status_is_clean_rejects_each_changed_porcelain_class() {
    assert!(repo_status_is_clean(""));
    assert!(!repo_status_is_clean("M  src/git_factor.rs\n"));
    assert!(!repo_status_is_clean(" M src/git_factor.rs\n"));
    assert!(!repo_status_is_clean("?? scratch.txt\n"));
    assert!(!repo_status_is_clean("!! target/\n"));
}

#[test]
fn scripted_runner_status_includes_env_key() {
    let dir = TempDir::new().or_abort("tempdir");
    let runner = ScriptedRunner::default();

    let err = runner
        .status(
            "git",
            &["status"],
            &[("GIT_OPTIONAL_LOCKS", Some("0"))],
            false,
            dir.path(),
        )
        .err_or_abort("expected missing scripted status to error");

    assert!(
        err.to_string().contains("unexpected status call"),
        "unexpected error: {err}"
    );
}

#[test]
fn scripted_runner_missing_output_is_an_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let runner = ScriptedRunner::default();

    let err = runner
        .output("git", &["version"], &[], dir.path())
        .err_or_abort("expected missing scripted output to error");

    assert!(
        err.to_string().contains("unexpected output call"),
        "unexpected error: {err}"
    );
}

#[test]
fn scripted_runner_missing_status_is_an_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let runner = ScriptedRunner::default();

    let err = runner
        .status("git", &["status"], &[], false, dir.path())
        .err_or_abort("expected missing scripted status to error");

    assert!(
        err.to_string().contains("unexpected status call"),
        "unexpected error: {err}"
    );
}

#[test]
fn env_returns_configured_cwd_and_exe() {
    let dir = TempDir::new().or_abort("tempdir");
    let env = TestEnv {
        cwd: dir.path().to_path_buf(),
    };

    assert_eq!(env.current_dir().or_abort("cwd"), dir.path());
    assert_eq!(
        env.current_exe().or_abort("exe"),
        dir.path().join("git-factor")
    );
    assert_eq!(env.var_os("ANY"), None);
}

#[test]
fn real_env_delegates_to_std_env() {
    let cwd = env::current_dir().or_abort("cwd");
    assert_eq!(REAL_ENV.current_dir().or_abort("real cwd"), cwd);

    let exe = env::current_exe().or_abort("exe");
    assert_eq!(REAL_ENV.current_exe().or_abort("real exe"), exe);

    // Cargo sets this for tests; it avoids env mutation (tests run in parallel).
    assert!(REAL_ENV.var_os("CARGO_MANIFEST_DIR").is_some());
}

#[test]
fn failing_io_out_is_reachable_for_coverage() {
    let io = FailingIo;
    let err = io.out("io fail").err_or_abort("expected io failure");
    assert!(err.to_string().contains("io fail"), "err was: {err:?}");
}

#[test]
fn real_io_writes_to_stdout_and_stderr() {
    // Nextest captures test output; keep it minimal while exercising RealIo.
    REAL_IO.out("").or_abort("out");
    REAL_IO.err("").or_abort("err");
    REAL_IO.outln("").or_abort("outln");
    REAL_IO.errln("").or_abort("errln");
}

#[test]
fn failing_io_errln_is_reachable_for_coverage() {
    let io = FailingIo;
    let err = io.errln("io fail").err_or_abort("expected io failure");
    assert!(err.to_string().contains("io fail"), "err was: {err:?}");
}

#[test]
fn failing_io_err_is_reachable_for_coverage() {
    let io = FailingIo;
    let err = io.err("io fail").err_or_abort("expected io failure");
    assert!(err.to_string().contains("io fail"), "err was: {err:?}");
}

#[test]
fn nth_io_failure_errln_is_reachable_for_coverage() {
    let io = NthIoFailure::new(2);
    let err = io.errln("io fail").err_or_abort("expected io failure");
    assert!(err.to_string().contains("io fail"), "err was: {err:?}");
}

#[test]
fn nth_io_failure_outln_first_write_failure_is_reachable_for_coverage() {
    let io = NthIoFailure::new(1);
    let err = io.outln("io fail").err_or_abort("expected io failure");
    assert!(err.to_string().contains("io fail"), "err was: {err:?}");
}

#[test]
fn nth_io_failure_outln_second_write_failure_is_reachable_for_coverage() {
    let io = NthIoFailure::new(2);
    let err = io.outln("io fail").err_or_abort("expected io failure");
    assert!(err.to_string().contains("io fail"), "err was: {err:?}");
}

#[test]
fn nth_io_failure_errln_first_write_failure_is_reachable_for_coverage() {
    let io = NthIoFailure::new(1);
    let err = io.errln("io fail").err_or_abort("expected io failure");
    assert!(err.to_string().contains("io fail"), "err was: {err:?}");
}

#[test]
fn default_outln_error_path_is_reachable_for_coverage() {
    let io = DefaultOutlnFailingIo;
    let err = io.outln("io fail").err_or_abort("expected io failure");
    assert!(err.to_string().contains("io fail"), "err was: {err:?}");
    io.err("").or_abort("err ok");
}

#[test]
fn default_outln_success_path_is_reachable_for_coverage() {
    let io = DefaultOutlnFailingIo;
    io.outln("ok").or_abort("expected io success");
}

#[test]
fn run_for_rejects_finish_with_continue_usage() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let runner = ScriptedRunner::default();
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--finish"),
            OsString::from("--continue"),
        ],
    )
    .err_or_abort("expected usage error");
    assert_eq!(
        err.to_string(),
        "--finish cannot be combined with --continue, --exec, or COMMIT"
    );
}

#[test]
fn run_for_rejects_finish_with_exec_usage() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let runner = ScriptedRunner::default();
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--finish"),
            OsString::from("--exec"),
            OsString::from("true"),
        ],
    )
    .err_or_abort("expected usage error");
    assert_eq!(
        err.to_string(),
        "--finish cannot be combined with --continue, --exec, or COMMIT"
    );
}

#[test]
fn run_for_rejects_finish_with_commit_usage() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let runner = ScriptedRunner::default();
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--finish"),
            OsString::from("HEAD"),
        ],
    )
    .err_or_abort("expected usage error");
    assert_eq!(
        err.to_string(),
        "--finish cannot be combined with --continue, --exec, or COMMIT"
    );
}

#[test]
fn validate_not_merge_rejects_commit_query_launch_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let runner = ScriptedRunner::default();
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let sha = CommitSha::new("a".repeat(SHA_LEN)).or_abort("valid sha");

    let err = validate_not_merge(&ctx, &sha).err_or_abort("commit query failure must refuse");
    assert!(matches!(&err, FactorError::GitCommand(_)));
    assert_eq!(
        err.to_string(),
        format!(
            concat!(
                "git command failed: git cat-file: unexpected output call: ",
                "output\x1fgit\x1f{}\x1f\x1fcat-file\x1fcommit\x1f{sha}",
            ),
            repo.display(),
            sha = sha,
        )
    );
}

#[test]
fn validate_not_merge_rejects_nonzero_commit_query() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = CommitSha::new("a".repeat(SHA_LEN)).or_abort("valid sha");
    let runner = ScriptedRunner::default().with_output_status(
        "git",
        &["cat-file", "commit", sha.as_str()],
        repo,
        1,
        &format!("tree {}\n\nSuccess-looking object\n", "b".repeat(SHA_LEN)),
        "owned commit query rejected\n",
    );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = validate_not_merge(&ctx, &sha).err_or_abort("failed query must refuse");
    assert!(matches!(&err, FactorError::GitCommand(_)));
    assert_eq!(
        err.to_string(),
        "git command failed: owned commit query rejected"
    );
    assert!(runner.outputs.borrow().values().all(VecDeque::is_empty));
    assert!(runner.statuses.borrow().is_empty());
}

#[test]
fn validate_not_merge_errors_for_merge_commit() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = CommitSha::new("a".repeat(SHA_LEN)).or_abort("valid sha");
    let runner = ScriptedRunner::default().with_output(
        "git",
        &["cat-file", "commit", sha.as_str()],
        repo,
        &format!(
            concat!(
                "tree {}\nparent {}\nparent {}\n",
                "author Author <author@example.test> 1000000000 +0000\n",
                "committer Author <author@example.test> 1000000000 +0000\n",
                "\nMerge\n",
            ),
            "b".repeat(SHA_LEN),
            "c".repeat(SHA_LEN),
            "d".repeat(SHA_LEN),
        ),
    );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = validate_not_merge(&ctx, &sha).err_or_abort("merge commit should be rejected");
    assert!(
        matches!(&err, FactorError::MergeCommit(found) if *found == sha),
        "err was: {err:?}"
    );
}

#[test]
fn default_errln_error_path_is_reachable_for_coverage() {
    let io = DefaultErrlnFailingIo;
    let err = io.errln("io fail").err_or_abort("expected io failure");
    assert!(err.to_string().contains("io fail"), "err was: {err:?}");
    io.out("").or_abort("out ok");
}

#[test]
fn default_errln_success_path_is_reachable_for_coverage() {
    let io = DefaultErrlnFailingIo;
    io.errln("ok").or_abort("expected io success");
}

#[test]
fn io_default_outln_and_errln_append_newlines() {
    let io = DefaultLineIo::default();

    io.outln("hello").or_abort("outln ok");
    io.errln("world").or_abort("errln ok");

    assert_eq!(io.stdout(), "hello\n");
    assert_eq!(io.stderr(), "world\n");
}

#[test]
fn real_io_outln_and_errln_succeed() {
    REAL_IO.outln("").or_abort("real outln should succeed");
    REAL_IO.errln("").or_abort("real errln should succeed");
}

#[test]
fn io_default_out_errors_when_stdout_lock_is_poisoned() {
    let io = DefaultLineIo::default();
    thread::scope(|scope| {
        let handle = scope.spawn(|| {
            let _guard = io.stdout.lock().or_abort("stdout lock");
            resume_unwind(Box::new(String::from("poison stdout lock")));
        });
        drop(handle.join());
    });

    let err = io
        .out("hello")
        .err_or_abort("expected out to fail on poisoned lock");
    assert_eq!(err.to_string(), "stdout lock poisoned");
}

#[test]
fn io_default_err_errors_when_stderr_lock_is_poisoned() {
    let io = DefaultLineIo::default();
    thread::scope(|scope| {
        let handle = scope.spawn(|| {
            let _guard = io.stderr.lock().or_abort("stderr lock");
            resume_unwind(Box::new(String::from("poison stderr lock")));
        });
        drop(handle.join());
    });

    let err = io
        .err("hello")
        .err_or_abort("expected err to fail on poisoned lock");
    assert_eq!(err.to_string(), "stderr lock poisoned");
}

#[test]
fn io_default_outln_errors_when_stdout_lock_is_poisoned() {
    let io = DefaultLineIo::default();
    thread::scope(|scope| {
        let handle = scope.spawn(|| {
            let _guard = io.stdout.lock().or_abort("stdout lock");
            resume_unwind(Box::new(String::from("poison stdout lock")));
        });
        drop(handle.join());
    });

    let err = io
        .outln("hello")
        .err_or_abort("expected outln to fail on poisoned lock");
    assert_eq!(err.to_string(), "stdout lock poisoned");
}

#[test]
fn io_default_errln_errors_when_stderr_lock_is_poisoned() {
    let io = DefaultLineIo::default();
    thread::scope(|scope| {
        let handle = scope.spawn(|| {
            let _guard = io.stderr.lock().or_abort("stderr lock");
            resume_unwind(Box::new(String::from("poison stderr lock")));
        });
        drop(handle.join());
    });

    let err = io
        .errln("hello")
        .err_or_abort("expected errln to fail on poisoned lock");
    assert_eq!(err.to_string(), "stderr lock poisoned");
}

#[test]
fn io_out_errors_when_stdout_lock_is_poisoned() {
    let io = TestIo::default();
    thread::scope(|scope| {
        let handle = scope.spawn(|| {
            let _guard = io.stdout.lock().or_abort("stdout lock");
            resume_unwind(Box::new(String::from("poison stdout lock")));
        });
        drop(handle.join());
    });

    let err = io
        .out("hello")
        .err_or_abort("expected out to fail on poisoned lock");
    assert_eq!(err.to_string(), "stdout lock poisoned");
}

#[test]
fn io_outln_and_errln_append_newlines() {
    let io = TestIo::default();

    io.outln("hello").or_abort("outln ok");
    io.errln("world").or_abort("errln ok");

    assert_eq!(io.stdout(), "hello\n");
    assert_eq!(io.stderr(), "world\n");
}

#[test]
fn io_err_errors_when_stderr_lock_is_poisoned() {
    let io = TestIo::default();
    thread::scope(|scope| {
        let handle = scope.spawn(|| {
            let _guard = io.stderr.lock().or_abort("stderr lock");
            resume_unwind(Box::new(String::from("poison stderr lock")));
        });
        drop(handle.join());
    });

    let err = io
        .err("hello")
        .err_or_abort("expected err to fail on poisoned lock");
    assert_eq!(err.to_string(), "stderr lock poisoned");
}

#[test]
fn failing_env_returns_errors_and_no_vars() {
    let env = FailingEnv { message: "nope" };

    assert_eq!(
        env.current_dir().err_or_abort("cwd err").to_string(),
        "nope"
    );
    assert_eq!(
        env.current_exe().err_or_abort("exe err").to_string(),
        "nope"
    );
    assert_eq!(env.var_os("ANY"), None);
}

#[test]
fn resolve_commit_refs_errors_on_invalid_rev_list_range() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let runner = ScriptedRunner::default().with_output_status(
        "git",
        &["rev-list", "bad..range"],
        repo,
        128,
        &format!("{}\n", "a".repeat(SHA_LEN)),
        "owned range query rejected\n",
    );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let refs =
        NonEmpty::new(NonEmptyString::try_from("bad..range".to_owned()).or_abort("nonempty range"));

    let err = resolve_commit_refs(&ctx, &refs).err_or_abort("expected invalid range");
    assert_eq!(err.to_string(), "invalid commit: bad..range");
    assert!(runner.outputs.borrow().values().all(VecDeque::is_empty));
    assert!(runner.statuses.borrow().is_empty());
}

#[test]
fn resolve_commit_refs_ignores_invalid_rev_list_lines() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let valid_sha = "a".repeat(SHA_LEN);
    let runner = ScriptedRunner::default().with_output(
        "git",
        &["rev-list", "HEAD~1..HEAD"],
        repo,
        &format!("bad\n{valid_sha}\n"),
    );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let refs = NonEmpty::new(
        NonEmptyString::try_from("HEAD~1..HEAD".to_owned()).or_abort("range ref is non-empty"),
    );
    let commits = resolve_commit_refs(&ctx, &refs).or_abort("resolve_commit_refs should succeed");
    let collected: Vec<&str> = commits.iter().map(CommitSha::as_str).collect();
    assert_eq!(collected, vec![valid_sha.as_str()]);
}

#[expect(
    clippy::single_call_fn,
    reason = "native filesystem adapter checks remain grouped as one existing contract arrangement"
)]
fn assert_fs_adapter_basics<F>(repo: &Path, fs: &F)
where
    F: Fs,
{
    let mkdir = repo.join("mkdir");
    fs.create_dir_all(&mkdir).or_abort("mkdir");
    assert!(fs.is_dir(&mkdir));
    assert!(fs.exists(&mkdir));
    let write_read = repo.join("write-read.txt");
    fs.write_string(&write_read, "hello")
        .or_abort("write hello");
    let content = fs.read_to_string(&write_read).or_abort("read hello");
    assert_eq!(content, "hello");
    let canonical = fs.canonicalize(&mkdir).or_abort("canonicalize mkdir");
    assert!(canonical.exists());
    let rm_dir = repo.join("rm-dir");
    fs.create_dir_all(&rm_dir).or_abort("create rm dir");
    fs.remove_dir_all(&rm_dir).or_abort("remove rm dir");
    assert!(!fs.exists(&rm_dir));
}

#[test]
fn fs_adapter_basics_work_with_real_fs() {
    let dir = TempDir::new().or_abort("tempdir");
    assert_fs_adapter_basics(dir.path(), &REAL_FS);
}

#[test]
fn main_entry_with_prints_error_when_ctx_cannot_be_built() {
    let io = TestIo::default();
    let env = FailingEnv { message: "no cwd" };
    let ctx = build_ctx_from_cwd(env.current_dir());

    let code = main_entry_with_vec(&io, ctx, &[OsString::from("git-factor")]);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(
        io.stderr(),
        "git command failed: cannot resolve cwd: no cwd\n"
    );
    assert_eq!(io.stdout(), "");
}

#[test]
fn commit_sha_new_validates_length_and_hex() {
    assert!(
        CommitSha::new("a".repeat(SHA_LEN)).is_ok(),
        "40 hex should be ok"
    );
    assert!(
        CommitSha::new("a".repeat(39)).is_err(),
        "wrong length should fail"
    );
    assert!(
        CommitSha::new("g".repeat(SHA_LEN)).is_err(),
        "non-hex should fail"
    );
}

#[test]
fn run_with_args_maps_help_to_exit_ok() {
    let dir = TempDir::new().or_abort("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };
    let code = run_and_report_with_args_vec(
        &ctx,
        &[OsString::from("git-factor"), OsString::from("--help")],
    );
    assert_eq!(code, EXIT_OK);
    assert!(io.stdout().contains("WORKFLOW:"), "help should be printed");
    assert!(io.stderr().is_empty(), "help should not print to stderr");
}

#[test]
fn run_with_args_maps_version_to_exit_ok() {
    let dir = TempDir::new().or_abort("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };
    let code = run_and_report_with_args_vec(
        &ctx,
        &[OsString::from("git-factor"), OsString::from("--version")],
    );
    assert_eq!(code, EXIT_OK);
    assert!(
        io.stdout().contains(env!("CARGO_PKG_VERSION")),
        "version should be printed"
    );
    assert!(io.stderr().is_empty(), "version should not print to stderr");
}

#[test]
fn run_with_args_maps_short_version_to_exit_ok() {
    let dir = TempDir::new().or_abort("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };
    let code =
        run_and_report_with_args_vec(&ctx, &[OsString::from("git-factor"), OsString::from("-v")]);
    assert_eq!(code, EXIT_OK);
    assert!(
        io.stdout().contains(env!("CARGO_PKG_VERSION")),
        "version should be printed"
    );
    assert!(io.stderr().is_empty(), "version should not print to stderr");
}

#[test]
fn run_with_args_invalid_flag_writes_to_stderr_and_returns_usage() {
    let dir = TempDir::new().or_abort("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let code = run_and_report_with_args_vec(
        &ctx,
        &[
            OsString::from("git-factor"),
            OsString::from("--definitely-not-real"),
        ],
    );

    assert_eq!(code, EXIT_USAGE);
    assert!(
        io.stderr()
            .contains("unexpected argument '--definitely-not-real'"),
        "stderr was: {}",
        io.stderr()
    );
    assert!(io.stdout().is_empty(), "stdout should be empty");
}

#[test]
fn run_and_report_refuses_legacy_state_without_writing_error_log() {
    let directory = TempDir::new().or_abort("legacy session fixture");
    let repo = directory.path();
    let state = repo.join(".git/factor");
    fs::create_dir_all(&state).or_abort("legacy scratch");
    let commits = format!("{}\n", "a".repeat(SHA_LEN));
    fs::write(state.join("commits"), &commits).or_abort("legacy commits");
    fs::write(state.join("current_index"), "0\n").or_abort("legacy index");
    fs::write(repo.join("user-file"), "unrelated bytes\n").or_abort("user bytes");
    let runner = with_git_dir_outputs(
        ScriptedRunner::default().with_output("git", &["version"], repo, "git version 2.56.0\n"),
        repo,
        6,
    );
    let io = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let context = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &environment,
        fs: &REAL_FS,
    };

    let code = run_and_report_with_args_vec(
        &context,
        &[OsString::from("git-factor"), OsString::from("--abort")],
    );

    assert_eq!((code, io.stdout(), io.stderr()), (
        EXIT_SOFTWARE, String::new(),
        "git command failed: existing legacy session must be finished or aborted with its originating version\n".to_owned(),
    ));
    assert!(runner.outputs.borrow().values().all(VecDeque::is_empty));
    let mut names = fs::read_dir(&state)
        .or_abort("preserved legacy directory")
        .map(|entry| entry.or_abort("legacy entry").file_name())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        vec![OsString::from("commits"), OsString::from("current_index")]
    );
    assert_eq!(
        fs::read(state.join("commits")).or_abort("preserved commits"),
        commits.as_bytes()
    );
    assert_eq!(
        fs::read(state.join("current_index")).or_abort("preserved index"),
        b"0\n"
    );
    assert_eq!(
        fs::read(repo.join("user-file")).or_abort("preserved user bytes"),
        b"unrelated bytes\n"
    );
    assert!(!state.join("error.log").exists());
    assert!(!repo.join(".git/factor-journal.json").exists());
}

#[test]
fn write_error_log_uses_the_admitted_directory_when_snapshot_reports_another_directory() {
    let directory = TempDir::new().or_abort("diagnostic fixture");
    let repo = directory.path().join("repository");
    let admitted = repo.join(".git/factor");
    let foreign = directory.path().join("foreign-git");
    fs::create_dir_all(&admitted).or_abort("admitted scratch");
    fs::create_dir_all(foreign.join("factor")).or_abort("foreign scratch");
    fs::write(foreign.join("factor/user"), b"protected foreign bytes\n").or_abort("foreign bytes");
    let runner = ScriptedRunner::default()
        .with_output(
            "git",
            &["rev-parse", "--git-dir"],
            &repo,
            &format!("{}\n", foreign.display()),
        )
        .with_output("git", &["rev-parse", "--verify", "HEAD"], &repo, "")
        .with_output("git", &["rev-parse", "--verify", HEAD_TREEISH], &repo, "")
        .with_output("git", &["rev-parse", "--show-toplevel"], &repo, "")
        .with_output(
            "git",
            &[
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
            ],
            &repo,
            "",
        );
    let io = TestIo::default();
    let ctx = Ctx {
        cwd: repo,
        fs: &REAL_FS,
        env: &REAL_ENV,
        io: &io,
        runner: &runner,
    };
    let args = [OsString::from("git-factor"), OsString::from("--continue")];
    let error = FactorError::StateWrite(io::Error::other("diagnostic failure"));

    let result = write_error_log(&ctx, &StateDir::new(admitted.clone()), &args, &error);

    result.or_abort("diagnostic write must succeed");
    let log = fs::read_to_string(admitted.join(ERROR_LOG_FILE)).or_abort("admitted diagnostic");
    assert!(log.contains("error=failed to write state: diagnostic failure\n"));
    assert!(log.contains(&format!("git_dir={}\n", foreign.display())));
    assert!(!foreign.join("factor/error.log").exists());
    assert_eq!(
        fs::read(foreign.join("factor/user")).or_abort("foreign readback"),
        b"protected foreign bytes\n"
    );
    assert_eq!(io.stdout(), "");
    assert_eq!(io.stderr(), "");
}

#[expect(
    clippy::single_call_fn,
    reason = "the diagnostic chain fixture arranges the existing complete error-source observer"
)]
fn arranged_diagnostic_error() -> FactorError {
    #[derive(Debug)]
    struct InnerCause;

    impl fmt::Display for InnerCause {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("root cause")
        }
    }

    #[expect(
        clippy::missing_trait_methods,
        reason = "test helper only needs a leaf error source"
    )]
    impl Error for InnerCause {}

    #[derive(Debug)]
    struct OuterCause(InnerCause);

    impl fmt::Display for OuterCause {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("outer cause")
        }
    }

    #[expect(
        clippy::missing_trait_methods,
        reason = "test helper only needs to expose a single nested source"
    )]
    impl Error for OuterCause {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.0)
        }
    }

    FactorError::StateWrite(io::Error::other(OuterCause(InnerCause)))
}

#[test]
fn write_error_log_includes_trace_path_and_error_sources() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let sha = "a".repeat(SHA_LEN);
    let tree = "b".repeat(SHA_LEN);
    let journal = format!(
        r#"{{"branch":"refs/heads/main","checkpoint":"{sha}","final_tree":"{tree}","format":"checkpoint_v2","gates":[],"original_base":null,"original_tip":"{sha}","session":"{sha}","state":{{"phase":"selecting","lease":"{sha}","source":"{sha}","anchor":"{sha}","base":null,"head":"{sha}"}}}}"#
    );
    fs::write(git_dir.join("factor-journal.json"), journal).or_abort("recorded checkpoint journal");

    let trace_log = repo.join("tmp").join("factor-trace.jsonl");
    let runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 1)
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-parse", "--verify", HEAD_TREEISH],
            repo,
            &format!("{tree}\n"),
        )
        .with_output(
            "git",
            &["rev-parse", "--show-toplevel"],
            repo,
            &format!("{}\n", repo.display()),
        )
        .with_output(
            "git",
            &[
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
            ],
            repo,
            "M  staged.txt\n M unstaged.txt\n?? new.txt\n",
        );
    let io = TestIo::default();
    let env = TraceLogEnv {
        cwd: repo.to_path_buf(),
        trace_log: trace_log.clone(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let args = [OsString::from("git-factor"), OsString::from("--abort")];
    let error = arranged_diagnostic_error();

    write_error_log(&ctx, &StateDir::new(state_dir.clone()), &args, &error)
        .or_abort("write error log should succeed");

    let error_log =
        fs::read_to_string(state_dir.join(ERROR_LOG_FILE)).or_abort("error log should exist");
    assert!(error_log.contains(format!("trace_log={}", trace_log.display()).as_str()));
    assert!(error_log.contains("source_0=outer cause"));
    assert!(error_log.contains("source_1=root cause"));
    assert!(error_log.contains("factor_phase=selecting"));
    assert!(error_log.contains(format!("factor_checkpoint={sha}").as_str()));
    assert!(error_log.contains(format!("factor_final_tree={tree}").as_str()));
    assert!(error_log.contains(format!("factor_source={sha}").as_str()));
    assert!(error_log.contains("staged_paths=staged.txt"));
    assert!(error_log.contains("unstaged_paths=unstaged.txt"));
    assert!(error_log.contains("untracked_paths=new.txt"));
}

#[test]
fn write_error_log_includes_checkpoint_source_and_apply_state() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    let rebase_apply = git_dir.join(REBASE_APPLY_DIR);
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::create_dir_all(&rebase_apply).or_abort("create rebase-apply dir");

    let sha = "c".repeat(SHA_LEN);
    let tree = "d".repeat(SHA_LEN);
    let journal = format!(
        r#"{{"branch":"refs/heads/main","checkpoint":"{sha}","final_tree":"{tree}","format":"checkpoint_v2","gates":[],"original_base":null,"original_tip":"{sha}","session":"{sha}","state":{{"phase":"opening","lease":"{sha}","source":"{sha}","anchor":"{sha}","base":null,"head":"{sha}"}}}}"#
    );
    fs::write(git_dir.join("factor-journal.json"), journal).or_abort("recorded checkpoint journal");
    fs::write(rebase_apply.join("next"), "3\n").or_abort("write next");
    fs::write(rebase_apply.join("last"), "5\n").or_abort("write last");
    fs::write(rebase_apply.join("patch"), "patch body\n").or_abort("write patch");

    let runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 1)
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-parse", "--verify", HEAD_TREEISH],
            repo,
            &format!("{tree}\n"),
        )
        .with_output(
            "git",
            &["rev-parse", "--show-toplevel"],
            repo,
            &format!("{}\n", repo.display()),
        )
        .with_output(
            "git",
            &[
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
            ],
            repo,
            "",
        );
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };
    let args = [OsString::from("git-factor"), OsString::from("--finish")];
    let error = FactorError::StateWrite(io::Error::other("apply state failure"));

    write_error_log(&ctx, &StateDir::new(state_dir.clone()), &args, &error)
        .or_abort("write error log should succeed");

    let error_log =
        fs::read_to_string(state_dir.join(ERROR_LOG_FILE)).or_abort("error log should exist");
    assert!(error_log.contains("factor_phase=opening"));
    assert!(error_log.contains(format!("factor_source={sha}").as_str()));
    assert!(error_log.contains("rebase_state=rebase-apply"));
    assert!(error_log.contains("rebase_msgnum=3"));
    assert!(error_log.contains("rebase_end=5"));
    assert!(error_log.contains("rebase_todo_head=patch"));
}

#[test]
fn run_with_args_errors_when_exec_is_missing() {
    let dir = TempDir::new().or_abort("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };
    let code = run_and_report_with_args_vec(
        &ctx,
        &[OsString::from("git-factor"), OsString::from("HEAD")],
    );
    assert_eq!(code, EXIT_USAGE);
    let stderr = io.stderr();
    assert!(stderr.contains("--exec <COMMAND> is required when starting a factor session"));
}

#[test]
fn run_with_args_defaults_missing_commit_to_head() {
    let dir = TempDir::new().or_abort("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };
    let code = run_and_report_with_args_vec(
        &ctx,
        &[
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
        ],
    );
    assert_eq!(code, EXIT_DATAERR);
    let stderr = io.stderr();
    assert_eq!(stderr, "not a git repository\n");
}

#[test]
fn run_with_args_defaults_missing_commit_to_head_propagates_invalid_head() {
    let fixture = public_cli::PublicCli::inactive();
    let before = fixture.facts();
    let arguments = ["git-factor", "--exec", "true"].map(OsString::from);

    let error = run_with_args_vec(&fixture.context(), arguments.to_vec())
        .err_or_abort("unborn HEAD cannot be selected");

    assert!(
        matches!(&error, FactorError::InvalidCommit(commit) if commit == "HEAD"),
        "{error:?}"
    );
    assert_eq!(fixture.facts(), before);
}

#[test]
fn run_with_args_without_user_args_prints_help() {
    let dir = TempDir::new().or_abort("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };
    let code = run_and_report_with_args_vec(&ctx, &[OsString::from("git-factor")]);
    assert_eq!(code, EXIT_OK);
    assert!(io.stdout().contains("WORKFLOW:"), "help should be printed");
    assert!(io.stderr().is_empty(), "stderr should be empty");
}

#[test]
fn run_with_args_abort_delegates_to_abort_handler() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let runner = ScriptedRunner::default()
        .with_output("git", &["version"], repo, "git version 2.56.0\n")
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![OsString::from("git-factor"), OsString::from("--abort")],
    )
    .err_or_abort("abort should delegate to command handler");
    assert!(
        matches!(err, FactorError::NoActiveSession),
        "err was: {err:?}"
    );
}

#[test]
fn run_with_args_status_delegates_to_status_handler() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let runner = ScriptedRunner::default()
        .with_output("git", &["version"], repo, "git version 2.56.0\n")
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let code = run_and_report_with_args_vec(
        &ctx,
        &[OsString::from("git-factor"), OsString::from("--status")],
    );
    assert_eq!(code, EXIT_OK);
    assert_eq!(io.stdout(), "{\"operation\":\"status\",\"session\":null}\n");
    assert_eq!(io.stderr(), "");
}

#[test]
fn run_with_args_finish_delegates_to_finish_handler() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let runner = ScriptedRunner::default()
        .with_output("git", &["version"], repo, "git version 2.56.0\n")
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--finish"),
            OsString::from("--message"),
            OsString::from("test: final"),
        ],
    )
    .err_or_abort("finish should delegate to command handler");
    assert!(
        matches!(err, FactorError::NoActiveSession),
        "err was: {err:?}"
    );
}

#[test]
fn run_with_args_continue_delegates_to_continue_handler() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let runner = ScriptedRunner::default()
        .with_output("git", &["version"], repo, "git version 2.56.0\n")
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--continue"),
            OsString::from("--message"),
            OsString::from("test: next"),
        ],
    )
    .err_or_abort("continue should delegate to command handler");
    assert!(
        matches!(err, FactorError::NoActiveSession),
        "err was: {err:?}"
    );
}

#[test]
fn run_with_args_retry_delegates_to_retry_handler() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let runner = ScriptedRunner::default()
        .with_output("git", &["version"], repo, "git version 2.56.0\n")
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![OsString::from("git-factor"), OsString::from("--retry")],
    )
    .err_or_abort("retry should delegate to command handler");
    assert!(
        matches!(err, FactorError::NoActiveSession),
        "err was: {err:?}"
    );
}

#[test]
fn run_with_args_start_requires_repository_for_explicit_commit_ref() {
    let dir = TempDir::new().or_abort("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };
    let code = run_and_report_with_args_vec(
        &ctx,
        &[
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD~1"),
        ],
    );
    assert_eq!(code, EXIT_DATAERR);
    assert_eq!(io.stderr(), "not a git repository\n");
}

#[test]
fn real_ctx_returns_current_dir() {
    let cwd = env::current_dir().or_abort("current_dir");
    let ctx = build_ctx_from_cwd(REAL_ENV.current_dir()).or_abort("real context");
    assert_eq!(ctx.cwd, cwd);
}

#[test]
fn main_entry_is_callable() {
    let code = main_entry();
    assert!(
        (i32::from(u8::MIN)..=i32::from(u8::MAX)).contains(&code),
        "exit code should be in range"
    );
}

#[test]
fn run_with_args_vec_propagates_io_errors_for_parser_and_help_output() {
    let dir = TempDir::new().or_abort("tempdir");
    let io = FailingIo;
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let parse_stderr_err = run_with_args_vec(
        &ctx,
        vec![OsString::from("git-factor"), OsString::from("--not-real")],
    )
    .err_or_abort("expected parse stderr io failure");
    assert!(
        matches!(&parse_stderr_err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
        "err was: {parse_stderr_err:?}"
    );

    let parse_stdout_err = run_with_args_vec(
        &ctx,
        vec![OsString::from("git-factor"), OsString::from("--help")],
    )
    .err_or_abort("expected parse stdout io failure");
    assert!(
        matches!(&parse_stdout_err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
        "err was: {parse_stdout_err:?}"
    );

    let long_help_err = run_with_args_vec(&ctx, vec![OsString::from("git-factor")])
        .err_or_abort("expected long-help io failure");
    assert!(
        matches!(&long_help_err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
        "err was: {long_help_err:?}"
    );
}

#[test]
fn io_default_line_helpers_report_second_write_failures() {
    #[derive(Default)]
    struct SecondWriteFailIo {
        err_calls: Mutex<usize>,
        out_calls: Mutex<usize>,
    }

    impl Io for SecondWriteFailIo {
        fn err(&self, _text: &str) -> io::Result<()> {
            let mut err_calls = self
                .err_calls
                .lock()
                .map_err(|error| io::Error::other(format!("err_calls lock: {error}")))?;
            *err_calls = err_calls
                .checked_add(1)
                .or_abort("counter should not overflow");
            if *err_calls == 2 {
                return Err(io::Error::other("second err write failed"));
            }
            drop(err_calls);
            Ok(())
        }

        fn errln(&self, line: &str) -> io::Result<()> {
            self.err(line)?;
            self.err("\n")
        }

        fn out(&self, _text: &str) -> io::Result<()> {
            let mut out_calls = self
                .out_calls
                .lock()
                .map_err(|error| io::Error::other(format!("out_calls lock: {error}")))?;
            *out_calls = out_calls
                .checked_add(1)
                .or_abort("counter should not overflow");
            if *out_calls == 2 {
                return Err(io::Error::other("second out write failed"));
            }
            drop(out_calls);
            Ok(())
        }

        fn outln(&self, line: &str) -> io::Result<()> {
            self.out(line)?;
            self.out("\n")
        }
    }

    let io = SecondWriteFailIo::default();

    let out_err = io
        .outln("line")
        .err_or_abort("expected second stdout write to fail");
    assert!(
        out_err.to_string().contains("second out write failed"),
        "error was: {out_err}"
    );

    let err_err = io
        .errln("line")
        .err_or_abort("expected second stderr write to fail");
    assert!(
        err_err.to_string().contains("second err write failed"),
        "error was: {err_err}"
    );
}
