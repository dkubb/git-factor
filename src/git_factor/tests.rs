use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;

use super::*;
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt as _;

#[cfg(unix)]
struct NonUtf8Fs;

#[cfg(unix)]
impl Fs for NonUtf8Fs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.create_dir_all(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_file(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        REAL_FS.read_to_string(path)
    }

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        REAL_FS.write_string(path, content)
    }

    fn canonicalize(&self, _path: &Path) -> io::Result<PathBuf> {
        let mut bytes = b"/tmp/".to_vec();
        bytes.push(0xff);
        bytes.extend_from_slice(b"/bin/git-factor");
        Ok(PathBuf::from(OsString::from_vec(bytes)))
    }

    fn is_dir(&self, path: &Path) -> bool {
        REAL_FS.is_dir(path)
    }

    fn exists(&self, path: &Path) -> bool {
        REAL_FS.exists(path)
    }
}

struct FailingRequiresRebaseWriteFs;

impl Fs for FailingRequiresRebaseWriteFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.create_dir_all(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_file(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        REAL_FS.read_to_string(path)
    }

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        if path
            .file_name()
            .is_some_and(|name| name == "requires_rebase")
        {
            return Err(io::Error::other("requires_rebase write failed"));
        }
        REAL_FS.write_string(path, content)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        REAL_FS.canonicalize(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        REAL_FS.is_dir(path)
    }

    fn exists(&self, path: &Path) -> bool {
        REAL_FS.exists(path)
    }
}

struct FailingIsRootWriteFs;

impl Fs for FailingIsRootWriteFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.create_dir_all(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_file(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        REAL_FS.read_to_string(path)
    }

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        if path.file_name().is_some_and(|name| name == "is_root") {
            return Err(io::Error::other("is_root write failed"));
        }
        REAL_FS.write_string(path, content)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        REAL_FS.canonicalize(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        REAL_FS.is_dir(path)
    }

    fn exists(&self, path: &Path) -> bool {
        REAL_FS.exists(path)
    }
}

struct FailingWriteForFileFs {
    file_name: &'static str,
    message: &'static str,
}

impl Fs for FailingWriteForFileFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.create_dir_all(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_file(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        REAL_FS.read_to_string(path)
    }

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        if path.file_name().is_some_and(|name| name == self.file_name) {
            return Err(io::Error::other(self.message));
        }
        REAL_FS.write_string(path, content)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        REAL_FS.canonicalize(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        REAL_FS.is_dir(path)
    }

    fn exists(&self, path: &Path) -> bool {
        REAL_FS.exists(path)
    }
}

struct NthReadFailureFs {
    file_name: &'static str,
    fail_at: usize,
    message: &'static str,
    reads: Mutex<usize>,
}

impl NthReadFailureFs {
    fn new(file_name: &'static str, fail_at: usize, message: &'static str) -> Self {
        Self {
            file_name,
            fail_at,
            message,
            reads: Mutex::new(0),
        }
    }
}

impl Fs for NthReadFailureFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.create_dir_all(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_file(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        if path.file_name().is_some_and(|name| name == self.file_name) {
            let mut reads = self.reads.lock().expect("nth-read lock");
            *reads = reads.saturating_add(1);
            if *reads == self.fail_at {
                return Err(io::Error::other(self.message));
            }
        }
        REAL_FS.read_to_string(path)
    }

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        REAL_FS.write_string(path, content)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        REAL_FS.canonicalize(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        REAL_FS.is_dir(path)
    }

    fn exists(&self, path: &Path) -> bool {
        REAL_FS.exists(path)
    }
}

fn ctx_for(path: &Path) -> Ctx<'static> {
    Ctx {
        runner: &REAL_RUNNER,
        cwd: path.to_path_buf(),
        io: &REAL_IO,
        env: &REAL_ENV,
        fs: &REAL_FS,
    }
}

fn is_forced_runner_failure(err: &FactorError) -> bool {
    matches!(
        err,
        FactorError::GitDir(msg) | FactorError::GitCommand(msg)
            if msg.contains("forced runner failure")
    )
}

#[cfg(unix)]
fn exit_status(code: i32) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt as _;
    ExitStatus::from_raw(code)
}

#[derive(Clone, Default)]
struct ScriptedRunner {
    outputs: HashMap<String, Output>,
    statuses: HashMap<String, ExitStatus>,
}

impl ScriptedRunner {
    fn output_key(bin: &str, args: &[&str], cwd: &Path) -> String {
        format!(
            "output\x1f{bin}\x1f{}\x1f{}",
            cwd.display(),
            args.join("\x1f")
        )
    }

    fn status_key(
        bin: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        quiet: bool,
        cwd: &Path,
    ) -> String {
        let env_key = envs
            .iter()
            .map(|&(k, v)| format!("{k}={v}"))
            .collect::<Vec<String>>()
            .join("\x1f");
        format!(
            "status\x1f{bin}\x1f{}\x1f{quiet}\x1f{env_key}\x1f{}",
            cwd.display(),
            args.join("\x1f")
        )
    }

    fn with_output(mut self, bin: &str, args: &[&str], cwd: &Path, stdout: &str) -> Self {
        let key = Self::output_key(bin, args, cwd);
        self.outputs.insert(
            key,
            Output {
                status: exit_status(0),
                stdout: stdout.as_bytes().to_vec(),
                stderr: Vec::new(),
            },
        );
        self
    }

    fn with_status(
        mut self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        quiet: bool,
        cwd: &Path,
        code: i32,
    ) -> Self {
        let key = Self::status_key(bin, args, envs, quiet, cwd);
        self.statuses.insert(key, exit_status(code));
        self
    }
}

impl Runner for ScriptedRunner {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        let key = Self::output_key(bin, args, cwd);
        self.outputs
            .get(&key)
            .cloned()
            .ok_or_else(|| io::Error::other(format!("unexpected output call: {key}")))
    }

    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        let key = Self::status_key(bin, args, envs, quiet, cwd);
        self.statuses
            .get(&key)
            .copied()
            .ok_or_else(|| io::Error::other(format!("unexpected status call: {key}")))
    }
}

struct NthRunnerFailure {
    inner: ScriptedRunner,
    fail_at: usize,
    calls: Mutex<usize>,
}

impl NthRunnerFailure {
    fn new(inner: ScriptedRunner, fail_at: usize) -> Self {
        Self {
            inner,
            fail_at,
            calls: Mutex::new(0),
        }
    }

    fn should_fail(&self) -> bool {
        let mut calls = self.calls.lock().expect("runner calls lock");
        *calls = calls.saturating_add(1);
        *calls == self.fail_at
    }
}

impl Runner for NthRunnerFailure {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        if self.should_fail() {
            return Err(io::Error::other("forced runner failure"));
        }
        self.inner.output(bin, args, cwd)
    }

    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        if self.should_fail() {
            return Err(io::Error::other("forced runner failure"));
        }
        self.inner.status(bin, args, envs, quiet, cwd)
    }
}

#[test]
fn scripted_runner_missing_output_is_an_error() {
    let dir = TempDir::new().expect("tempdir");
    let runner = ScriptedRunner::default();

    let err = runner
        .output("git", &["version"], dir.path())
        .expect_err("expected missing scripted output to error");

    assert!(
        err.to_string().contains("unexpected output call"),
        "unexpected error: {err}"
    );
}

#[test]
fn scripted_runner_missing_status_is_an_error() {
    let dir = TempDir::new().expect("tempdir");
    let runner = ScriptedRunner::default();

    let err = runner
        .status("git", &["status"], &[], false, dir.path())
        .expect_err("expected missing scripted status to error");

    assert!(
        err.to_string().contains("unexpected status call"),
        "unexpected error: {err}"
    );
}

#[test]
fn scripted_runner_status_includes_env_key() {
    let dir = TempDir::new().expect("tempdir");
    let runner = ScriptedRunner::default();

    let err = runner
        .status(
            "git",
            &["status"],
            &[("GIT_OPTIONAL_LOCKS", "0")],
            false,
            dir.path(),
        )
        .expect_err("expected missing scripted status to error");

    assert!(
        err.to_string().contains("unexpected status call"),
        "unexpected error: {err}"
    );
}

#[test]
fn git_commit_preserving_metadata_propagates_git_output_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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

    let commit = CommitSha::new("a".repeat(40)).expect("commit sha");
    let msg = NonEmptyString::try_from("feat: msg".to_owned()).expect("msg");
    let messages = NonEmpty::new(msg);

    let err = git_commit_preserving_metadata(&ctx, &commit, &messages, false)
        .expect_err("expected git output error");

    assert!(
        err.to_string().contains("unexpected output call"),
        "unexpected error: {err}"
    );
}

#[test]
#[should_panic(expected = "author date")]
fn git_commit_preserving_metadata_panics_on_truncated_format() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                &sha,
            ],
            repo,
            "Alice\0alice@example.com",
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

    let commit = CommitSha::new(sha).expect("commit sha");
    let msg = NonEmptyString::try_from("feat: msg".to_owned()).expect("msg");
    let messages = NonEmpty::new(msg);

    // Current code panics on truncated output; after fix this will return Err
    drop(git_commit_preserving_metadata(
        &ctx, &commit, &messages, false,
    ));
}

struct TestEnv {
    cwd: PathBuf,
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

struct ClaudeCodeEnv {
    cwd: PathBuf,
}

impl Env for ClaudeCodeEnv {
    fn current_dir(&self) -> io::Result<PathBuf> {
        Ok(self.cwd.clone())
    }

    fn current_exe(&self) -> io::Result<PathBuf> {
        Ok(self.cwd.join("git-factor"))
    }

    fn var_os(&self, key: &str) -> Option<OsString> {
        (key == "CLAUDECODE").then(|| OsString::from("1"))
    }
}

#[test]
fn env_returns_configured_cwd_and_exe() {
    let dir = TempDir::new().expect("tempdir");
    let env = TestEnv {
        cwd: dir.path().to_path_buf(),
    };

    assert_eq!(env.current_dir().expect("cwd"), dir.path());
    assert_eq!(
        env.current_exe().expect("exe"),
        dir.path().join("git-factor")
    );
    assert_eq!(env.var_os("ANY"), None);
}

#[test]
fn real_env_delegates_to_std_env() {
    let cwd = env::current_dir().expect("cwd");
    assert_eq!(REAL_ENV.current_dir().expect("real cwd"), cwd);

    let exe = env::current_exe().expect("exe");
    assert_eq!(REAL_ENV.current_exe().expect("real exe"), exe);

    // Cargo sets this for tests; it avoids env mutation (tests run in parallel).
    assert!(REAL_ENV.var_os("CARGO_MANIFEST_DIR").is_some());
}

#[test]
fn real_io_writes_to_stdout_and_stderr() {
    // Nextest captures test output; keep it minimal while exercising RealIo.
    REAL_IO.out("").expect("out");
    REAL_IO.err("").expect("err");
    REAL_IO.outln("").expect("outln");
    REAL_IO.errln("").expect("errln");
}

#[derive(Default)]
struct TestIo {
    stdout: Mutex<String>,
    stderr: Mutex<String>,
}

impl TestIo {
    fn stdout(&self) -> String {
        self.stdout.lock().expect("stdout lock").clone()
    }

    fn stderr(&self) -> String {
        self.stderr.lock().expect("stderr lock").clone()
    }
}

impl Io for TestIo {
    fn out(&self, text: &str) -> io::Result<()> {
        self.stdout
            .lock()
            .map_err(|_err| io::Error::other("stdout lock poisoned"))?
            .push_str(text);
        Ok(())
    }

    fn err(&self, text: &str) -> io::Result<()> {
        self.stderr
            .lock()
            .map_err(|_err| io::Error::other("stderr lock poisoned"))?
            .push_str(text);
        Ok(())
    }

    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }

    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(line)?;
        self.err("\n")
    }
}

struct FailingIo;

impl Io for FailingIo {
    fn out(&self, _text: &str) -> io::Result<()> {
        Err(io::Error::other("io fail"))
    }

    fn err(&self, _text: &str) -> io::Result<()> {
        Err(io::Error::other("io fail"))
    }

    fn outln(&self, _line: &str) -> io::Result<()> {
        Err(io::Error::other("io fail"))
    }

    fn errln(&self, _line: &str) -> io::Result<()> {
        Err(io::Error::other("io fail"))
    }
}

struct NthIoFailure {
    fail_at: usize,
    calls: Mutex<usize>,
}

impl NthIoFailure {
    fn new(fail_at: usize) -> Self {
        Self {
            fail_at,
            calls: Mutex::new(0),
        }
    }

    fn maybe_fail(&self) -> io::Result<()> {
        let mut calls = self.calls.lock().expect("io calls lock");
        *calls = calls.saturating_add(1);
        if *calls == self.fail_at {
            Err(io::Error::other("io fail"))
        } else {
            Ok(())
        }
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Test helper uses Io default line methods to exercise out/err call sites"
)]
impl Io for NthIoFailure {
    fn out(&self, _text: &str) -> io::Result<()> {
        self.maybe_fail()
    }

    fn err(&self, _text: &str) -> io::Result<()> {
        self.maybe_fail()
    }
}

#[test]
fn failing_io_out_is_reachable_for_coverage() {
    let io = FailingIo;
    let err = io.out("io fail").expect_err("expected io failure");
    assert!(err.to_string().contains("io fail"), "err was: {err:?}");
}

#[test]
fn failing_io_err_is_reachable_for_coverage() {
    let io = FailingIo;
    let err = io.err("io fail").expect_err("expected io failure");
    assert!(err.to_string().contains("io fail"), "err was: {err:?}");
}

#[test]
fn failing_io_errln_is_reachable_for_coverage() {
    let io = FailingIo;
    let err = io.errln("io fail").expect_err("expected io failure");
    assert!(err.to_string().contains("io fail"), "err was: {err:?}");
}

#[test]
fn nth_io_failure_errln_is_reachable_for_coverage() {
    let io = NthIoFailure::new(2);
    let err = io.errln("io fail").expect_err("expected io failure");
    assert!(err.to_string().contains("io fail"), "err was: {err:?}");
}

struct DefaultOutlnFailingIo;

#[expect(
    clippy::missing_trait_methods,
    reason = "Test helper intentionally relies on Io default outln to cover the default implementation"
)]
impl Io for DefaultOutlnFailingIo {
    fn out(&self, _text: &str) -> io::Result<()> {
        Err(io::Error::other("io fail"))
    }

    fn err(&self, _text: &str) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn default_outln_error_path_is_reachable_for_coverage() {
    let io = DefaultOutlnFailingIo;
    let err = io.outln("io fail").expect_err("expected io failure");
    assert!(err.to_string().contains("io fail"), "err was: {err:?}");
    io.err("").expect("err ok");
}

struct DefaultErrlnFailingIo;

#[expect(
    clippy::missing_trait_methods,
    reason = "Test helper intentionally relies on Io default errln to cover the default implementation"
)]
impl Io for DefaultErrlnFailingIo {
    fn out(&self, _text: &str) -> io::Result<()> {
        Ok(())
    }

    fn err(&self, _text: &str) -> io::Result<()> {
        Err(io::Error::other("io fail"))
    }
}

#[test]
fn default_errln_error_path_is_reachable_for_coverage() {
    let io = DefaultErrlnFailingIo;
    let err = io.errln("io fail").expect_err("expected io failure");
    assert!(err.to_string().contains("io fail"), "err was: {err:?}");
    io.out("").expect("out ok");
}

#[derive(Default)]
struct DefaultLineIo {
    stdout: Mutex<String>,
    stderr: Mutex<String>,
}

impl DefaultLineIo {
    fn stdout(&self) -> String {
        self.stdout.lock().expect("stdout lock").clone()
    }

    fn stderr(&self) -> String {
        self.stderr.lock().expect("stderr lock").clone()
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Test helper intentionally relies on Io default outln/errln to cover default implementations"
)]
impl Io for DefaultLineIo {
    fn out(&self, text: &str) -> io::Result<()> {
        self.stdout
            .lock()
            .map_err(|_err| io::Error::other("stdout lock poisoned"))?
            .push_str(text);
        Ok(())
    }

    fn err(&self, text: &str) -> io::Result<()> {
        self.stderr
            .lock()
            .map_err(|_err| io::Error::other("stderr lock poisoned"))?
            .push_str(text);
        Ok(())
    }
}

#[test]
fn io_default_outln_and_errln_append_newlines() {
    let io = DefaultLineIo::default();

    io.outln("hello").expect("outln ok");
    io.errln("world").expect("errln ok");

    assert_eq!(io.stdout(), "hello\n");
    assert_eq!(io.stderr(), "world\n");
}

#[test]
#[expect(
    clippy::panic,
    reason = "Poisoning a mutex requires panicking while holding the lock"
)]
fn io_default_out_errors_when_stdout_lock_is_poisoned() {
    let io = DefaultLineIo::default();
    thread::scope(|scope| {
        let handle = scope.spawn(|| {
            let _guard = io.stdout.lock().expect("stdout lock");
            panic!("poison stdout lock");
        });
        drop(handle.join());
    });

    let err = io
        .out("hello")
        .expect_err("expected out to fail on poisoned lock");
    assert_eq!(err.to_string(), "stdout lock poisoned");
}

#[test]
#[expect(
    clippy::panic,
    reason = "Poisoning a mutex requires panicking while holding the lock"
)]
fn io_default_err_errors_when_stderr_lock_is_poisoned() {
    let io = DefaultLineIo::default();
    thread::scope(|scope| {
        let handle = scope.spawn(|| {
            let _guard = io.stderr.lock().expect("stderr lock");
            panic!("poison stderr lock");
        });
        drop(handle.join());
    });

    let err = io
        .err("hello")
        .expect_err("expected err to fail on poisoned lock");
    assert_eq!(err.to_string(), "stderr lock poisoned");
}

#[test]
fn io_outln_and_errln_append_newlines() {
    let io = TestIo::default();

    io.outln("hello").expect("outln ok");
    io.errln("world").expect("errln ok");

    assert_eq!(io.stdout(), "hello\n");
    assert_eq!(io.stderr(), "world\n");
}

#[test]
#[expect(
    clippy::panic,
    reason = "Poisoning a mutex requires panicking while holding the lock"
)]
fn io_out_errors_when_stdout_lock_is_poisoned() {
    let io = TestIo::default();
    thread::scope(|scope| {
        let handle = scope.spawn(|| {
            let _guard = io.stdout.lock().expect("stdout lock");
            panic!("poison stdout lock");
        });
        drop(handle.join());
    });

    let err = io
        .out("hello")
        .expect_err("expected out to fail on poisoned lock");
    assert_eq!(err.to_string(), "stdout lock poisoned");
}

#[test]
#[expect(
    clippy::panic,
    reason = "Poisoning a mutex requires panicking while holding the lock"
)]
fn io_err_errors_when_stderr_lock_is_poisoned() {
    let io = TestIo::default();
    thread::scope(|scope| {
        let handle = scope.spawn(|| {
            let _guard = io.stderr.lock().expect("stderr lock");
            panic!("poison stderr lock");
        });
        drop(handle.join());
    });

    let err = io
        .err("hello")
        .expect_err("expected err to fail on poisoned lock");
    assert_eq!(err.to_string(), "stderr lock poisoned");
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

struct ExeFailingEnv {
    cwd: PathBuf,
}

impl Env for ExeFailingEnv {
    fn current_dir(&self) -> io::Result<PathBuf> {
        Ok(self.cwd.clone())
    }

    fn current_exe(&self) -> io::Result<PathBuf> {
        Err(io::Error::other("no exe"))
    }

    fn var_os(&self, _key: &str) -> Option<OsString> {
        None
    }
}

struct RootExeEnv {
    cwd: PathBuf,
}

impl Env for RootExeEnv {
    fn current_dir(&self) -> io::Result<PathBuf> {
        Ok(self.cwd.clone())
    }

    fn current_exe(&self) -> io::Result<PathBuf> {
        Ok(PathBuf::from("/"))
    }

    fn var_os(&self, _key: &str) -> Option<OsString> {
        None
    }
}

#[test]
fn failing_env_returns_errors_and_no_vars() {
    let env = FailingEnv { message: "nope" };

    assert_eq!(env.current_dir().expect_err("cwd err").to_string(), "nope");
    assert_eq!(env.current_exe().expect_err("exe err").to_string(), "nope");
    assert_eq!(env.var_os("ANY"), None);
}

#[test]
fn editor_path_errors_when_current_exe_fails() {
    let dir = TempDir::new().expect("tempdir");
    let env = ExeFailingEnv {
        cwd: dir.path().to_path_buf(),
    };
    let io = TestIo::default();
    let ctx = ctx_from_parts(&env, &REAL_RUNNER, &io, &REAL_FS).expect("ctx ok");

    let err = editor_path(&ctx).expect_err("expected editor_path to error");

    assert_eq!(env.var_os("ANY"), None);
    assert_eq!(
        err.to_string(),
        "git command failed: cannot resolve current exe: no exe"
    );
}

#[test]
fn editor_path_errors_when_exe_cannot_be_canonicalized() {
    let dir = TempDir::new().expect("tempdir");
    let env = TestEnv {
        cwd: dir.path().to_path_buf(),
    };
    let io = TestIo::default();
    let ctx = ctx_from_parts(&env, &REAL_RUNNER, &io, &REAL_FS).expect("ctx ok");

    let err = editor_path(&ctx).expect_err("expected editor_path to error");

    assert!(
        err.to_string()
            .starts_with("git command failed: cannot canonicalize exe: "),
        "unexpected error: {err}"
    );
}

#[test]
fn editor_path_errors_when_executable_has_no_parent_dir() {
    let dir = TempDir::new().expect("tempdir");
    let env = RootExeEnv {
        cwd: dir.path().to_path_buf(),
    };
    let io = TestIo::default();
    let ctx = ctx_from_parts(&env, &REAL_RUNNER, &io, &REAL_FS).expect("ctx ok");

    let err = editor_path(&ctx).expect_err("expected editor_path to error");

    assert_eq!(env.var_os("ANY"), None);
    assert_eq!(
        err.to_string(),
        "git command failed: executable has no parent directory"
    );
}

#[cfg(unix)]
#[test]
fn editor_path_errors_when_path_is_not_valid_utf8() {
    let dir = TempDir::new().expect("tempdir");
    let env = TestEnv {
        cwd: dir.path().to_path_buf(),
    };
    let io = TestIo::default();
    let fs = NonUtf8Fs;
    let ctx = ctx_from_parts(&env, &REAL_RUNNER, &io, &fs).expect("ctx ok");

    let mkdir = dir.path().join("mkdir");
    fs.create_dir_all(&mkdir).expect("mkdir ok");
    assert!(mkdir.is_dir());

    let rm_dir = dir.path().join("rm_dir");
    fs.create_dir_all(&rm_dir).expect("rm_dir create");
    fs.remove_dir_all(&rm_dir).expect("rm_dir remove");
    assert!(!rm_dir.exists());

    let rm_file = dir.path().join("rm_file");
    fs::write(&rm_file, "x").expect("rm_file write");
    fs.remove_file(&rm_file).expect("rm_file remove");
    assert!(!rm_file.exists());

    let read_file = dir.path().join("read_to_string");
    fs::write(&read_file, "hello").expect("read_file write");
    let content = fs.read_to_string(&read_file).expect("read_to_string ok");
    assert_eq!(content, "hello");

    let write_file = dir.path().join("write_string");
    fs.write_string(&write_file, "world")
        .expect("write_string ok");
    let written = fs.read_to_string(&write_file).expect("read back ok");
    assert_eq!(written, "world");

    let is_dir = fs.is_dir(&mkdir);
    assert!(is_dir);

    let exists = fs.exists(&mkdir);
    assert!(exists);

    let err = editor_path(&ctx).expect_err("expected editor_path to error");
    assert_eq!(
        err.to_string(),
        "git command failed: editor path is not valid UTF-8"
    );
}

#[test]
fn resolve_commit_refs_errors_on_invalid_rev_list_range() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("bad..range"),
        ],
    )
    .expect_err("expected invalid commit");
    assert_eq!(err.to_string(), "invalid commit: bad..range");
}

#[test]
fn resolve_commit_refs_ignores_invalid_rev_list_lines() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let valid_sha = "a".repeat(40);
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
        NonEmptyString::try_from("HEAD~1..HEAD".to_owned()).expect("range ref is non-empty"),
    );
    let commits = resolve_commit_refs(&ctx, &refs).expect("resolve_commit_refs should succeed");
    let collected: Vec<&str> = commits.iter().map(CommitSha::as_str).collect();
    assert_eq!(collected, vec![valid_sha.as_str()]);
}

#[test]
fn cmd_start_errors_when_no_commits_remain_after_sorting() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let sha_a = "a".repeat(40);
    let sha_b = "b".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha_a}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha_a],
            repo,
            &format!("{sha_b}\n"),
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

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .expect_err("expected sorting to error");
    assert_eq!(
        err.to_string(),
        "git command failed: no commits after sorting"
    );
}

#[test]
fn print_session_started_single_commit_without_untracked_or_claude_hints() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let runner = ScriptedRunner::default()
        .with_output(
            "git",
            &["diff", "--stat"],
            repo,
            "a.txt | 1 +\n1 file changed, 1 insertion(+)\n",
        )
        .with_output(
            "git",
            &["ls-files", "--others", "--exclude-standard"],
            repo,
            "",
        )
        .with_output(
            "git",
            &["rev-parse", "--show-toplevel"],
            repo,
            &format!("{}\n", repo.display()),
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
    let commit = CommitSha::new("a".repeat(40)).expect("sha");
    let commits = NonEmpty::new(commit);
    let short = NonEmptyString::try_from("aaaaaaa".to_owned()).expect("short sha");

    print_session_started(&ctx, &commits, &short, "subject").expect("print should succeed");

    let expected = concat!(
        "FACTOR: Split session started for aaaaaaa.\n",
        "ORIGINAL MESSAGE: subject\n",
        "UNSTAGED:\n",
        "  a.txt | 1 +\n",
        "  1 file changed, 1 insertion(+)\n",
        "\n",
        "NEXT: Stage changes for the first atomic commit, then run:\n",
        "  git factor --continue --message \"type: description\"\n",
        "\n",
        "Run git factor --help for the full workflow guide.\n",
        "\n",
        "HINTS:\n",
        "  - Find the ONE smallest addition nothing depends on\n",
        "  - Target 15-30 lines (50 max)\n",
        "  - Message: single concrete action, no \"and\"/\"or\"\n",
        "  - Verify: git log --oneline | wc -l\n",
        "  - NEVER use git commit. ONLY use git factor --continue.\n",
        "  REMAINING: 1 file changed, 1 insertion(+)\n",
        "  RECOVERY: git factor --abort\n"
    );
    assert_eq!(io.stdout(), expected);
    assert!(io.stderr().is_empty());
}

#[test]
fn print_session_started_multi_commit_with_untracked_and_claude_hints() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let reference_dir = repo.join("references");
    fs::create_dir_all(&reference_dir).expect("create references dir");
    let rust_ref = reference_dir.join("rust.md");
    fs::write(&rust_ref, "# rust\n").expect("write rust reference");

    let runner = ScriptedRunner::default()
        .with_output(
            "git",
            &["diff", "--stat"],
            repo,
            "a.txt | 1 +\n1 file changed, 1 insertion(+)\n",
        )
        .with_output(
            "git",
            &["ls-files", "--others", "--exclude-standard"],
            repo,
            "tmp.txt\n",
        )
        .with_output(
            "git",
            &["rev-parse", "--show-toplevel"],
            repo,
            &format!("{}\n", repo.display()),
        );
    let io = TestIo::default();
    let env = ClaudeCodeEnv {
        cwd: repo.to_path_buf(),
    };
    assert_eq!(env.current_dir().expect("cwd"), repo);
    assert_eq!(env.current_exe().expect("exe"), repo.join("git-factor"));
    assert_eq!(env.var_os("CLAUDECODE"), Some(OsString::from("1")));
    assert_eq!(env.var_os("ANY"), None);
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let commit_a = CommitSha::new("a".repeat(40)).expect("sha a");
    let commit_b = CommitSha::new("b".repeat(40)).expect("sha b");
    let commits = NonEmpty {
        head: commit_a,
        tail: vec![commit_b],
    };
    let short = NonEmptyString::try_from("aaaaaaa".to_owned()).expect("short sha");

    print_session_started(&ctx, &commits, &short, "subject").expect("print should succeed");

    let expected = format!(
        concat!(
            "FACTOR: Split session started for 2 commits (first: aaaaaaa).\n",
            "ORIGINAL MESSAGE: subject\n",
            "UNSTAGED:\n",
            "  a.txt | 1 +\n",
            "  1 file changed, 1 insertion(+)\n",
            "UNTRACKED:\n",
            "  tmp.txt\n",
            "\n",
            "NEXT: Stage changes for the first atomic commit, then run:\n",
            "  git factor --continue --message \"type: description\"\n",
            "\n",
            "Run git factor --help for the full workflow guide.\n",
            "\n",
            "HINTS:\n",
            "  - Find the ONE smallest addition nothing depends on\n",
            "  - Target 15-30 lines (50 max)\n",
            "  - Message: single concrete action, no \"and\"/\"or\"\n",
            "  - Verify: git log --oneline | wc -l\n",
            "  - NEVER use git commit. ONLY use git factor --continue.\n",
            "  REMAINING: 1 file changed, 1 insertion(+)\n",
            "  REFERENCE: {}\n",
            "  RECOVERY: git factor --abort\n",
            "<claude>\n",
            "- If context is above 50%, pause and ask the user to /compact.\n",
            "- Do NOT stop early. Keep committing until \"Complete\".\n",
            "- Do NOT use git commit directly. ONLY use git-factor --continue.\n",
            "- Each commit MUST pass the exec gate. No shortcuts.\n",
            "</claude>\n"
        ),
        rust_ref.display()
    );
    assert_eq!(io.stdout(), expected);
    assert!(io.stderr().is_empty());
}

#[test]
fn cmd_start_errors_when_merge_base_spawn_fails() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha],
            repo,
            &format!("{sha}\n"),
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

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .expect_err("expected merge-base spawn to fail");

    assert!(
        err.to_string().contains("merge-base"),
        "unexpected error: {err}"
    );
}

#[test]
fn cmd_start_errors_when_short_sha_is_empty() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha],
            repo,
            &format!("{sha}\n"),
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_output("git", &["rev-parse", "--short", &sha], repo, "\n");
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
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .expect_err("expected short SHA to be empty");

    assert_eq!(err.to_string(), "git command failed: empty short SHA");
}

#[test]
fn cmd_start_propagates_rev_parse_short_sha_output_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha],
            repo,
            &format!("{sha}\n"),
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^2")],
            &[],
            true,
            repo,
            1,
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

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .expect_err("expected rev-parse --short to fail");

    assert!(
        err.to_string().contains("git rev-parse:")
            && err.to_string().contains("unexpected output call"),
        "unexpected error: {err}"
    );
}

#[test]
fn cmd_start_propagates_status_error_when_start_sequence_fails() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    fs::write(repo.join("git-factor"), "").expect("write fake exe");

    let sha = "a".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha],
            repo,
            &format!("{sha}\n"),
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &sha],
            repo,
            "msg\n",
        )
        .with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", "true"],
            &[],
            true,
            repo,
            0,
        )
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n")
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
            &[],
            true,
            repo,
            0,
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

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .expect_err("expected startup status lookup to fail");

    assert!(
        matches!(
            &err,
            FactorError::GitCommand(msg)
                if msg.contains("git reset:") && msg.contains("unexpected status call")
        ),
        "err was: {err:?}"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "Full cmd_start scripted runner setup for rebase-status failure path"
)]
fn cmd_start_propagates_rebase_status_error_in_multi_commit_session() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    fs::write(repo.join("git-factor"), "").expect("write fake exe");

    let sha_a = "a".repeat(40);
    let sha_b = "b".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", &sha_a],
            repo,
            &format!("{sha_a}\n"),
        )
        .with_output(
            "git",
            &["rev-parse", "--verify", &sha_b],
            repo,
            &format!("{sha_b}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha_a, &sha_b],
            repo,
            &format!("{sha_a}\n{sha_b}\n"),
        )
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha_b}\n"),
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha_a, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha_a}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha_b, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha_b}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_output("git", &["rev-parse", "--short", &sha_a], repo, "aaaaaaa\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &sha_a],
            repo,
            "msg\n",
        )
        .with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", "true"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha_a}^")],
            &[],
            true,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "--short", &sha_a], repo, "aaaaaaa\n")
        .with_output("git", &["rev-parse", "--short", &sha_b], repo, "bbbbbbb\n");

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
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from(&sha_a),
            OsString::from(&sha_b),
        ],
    )
    .expect_err("expected rebase status call to fail");

    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git rebase:") && msg.contains("unexpected status call")),
        "err was: {err:?}"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "Full cmd_start scripted runner setup with failing state write path"
)]
fn cmd_start_propagates_requires_rebase_state_write_failure() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let fs = FailingRequiresRebaseWriteFs;

    let mkdir = repo.join("mkdir");
    fs.create_dir_all(&mkdir).expect("mkdir");
    assert!(fs.is_dir(&mkdir));
    assert!(fs.exists(&mkdir));

    let write_read = repo.join("write-read.txt");
    fs.write_string(&write_read, "hello").expect("write hello");
    let content = fs.read_to_string(&write_read).expect("read hello");
    assert_eq!(content, "hello");

    let canonical = fs.canonicalize(&mkdir).expect("canonicalize mkdir");
    assert!(canonical.exists());

    let rm_file = repo.join("rm-file.txt");
    fs.write_string(&rm_file, "x").expect("write rm file");
    fs.remove_file(&rm_file).expect("remove rm file");
    assert!(!fs.exists(&rm_file));

    let rm_dir = repo.join("rm-dir");
    fs.create_dir_all(&rm_dir).expect("create rm dir");
    fs.remove_dir_all(&rm_dir).expect("remove rm dir");
    assert!(!fs.exists(&rm_dir));

    let sha = "a".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha],
            repo,
            &format!("{sha}\n"),
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &sha],
            repo,
            "msg\n",
        )
        .with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", "true"],
            &[],
            true,
            repo,
            0,
        )
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
            &[],
            true,
            repo,
            0,
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
        fs: &fs,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .expect_err("expected requires_rebase write failure");

    assert_eq!(
        err.to_string(),
        "failed to write state: requires_rebase write failed"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "Full cmd_start scripted runner setup with failing state write path"
)]
fn cmd_start_propagates_is_root_state_write_failure() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let fs = FailingIsRootWriteFs;

    let mkdir = repo.join("mkdir");
    fs.create_dir_all(&mkdir).expect("mkdir");
    assert!(fs.is_dir(&mkdir));
    assert!(fs.exists(&mkdir));

    let write_read = repo.join("write-read.txt");
    fs.write_string(&write_read, "hello").expect("write hello");
    let content = fs.read_to_string(&write_read).expect("read hello");
    assert_eq!(content, "hello");

    let canonical = fs.canonicalize(&mkdir).expect("canonicalize mkdir");
    assert!(canonical.exists());

    let rm_file = repo.join("rm-file.txt");
    fs.write_string(&rm_file, "x").expect("write rm file");
    fs.remove_file(&rm_file).expect("remove rm file");
    assert!(!fs.exists(&rm_file));

    let rm_dir = repo.join("rm-dir");
    fs.create_dir_all(&rm_dir).expect("create rm dir");
    fs.remove_dir_all(&rm_dir).expect("remove rm dir");
    assert!(!fs.exists(&rm_dir));

    let sha = "a".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha],
            repo,
            &format!("{sha}\n"),
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &sha],
            repo,
            "msg\n",
        )
        .with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", "true"],
            &[],
            true,
            repo,
            0,
        )
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
            &[],
            true,
            repo,
            0,
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
        fs: &fs,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .expect_err("expected is_root write failure");

    assert_eq!(
        err.to_string(),
        "failed to write state: is_root write failed"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "Full cmd_start scripted runner setup; splitting further would reduce readability"
)]
fn cmd_start_range_ref_inserts_shas_and_propagates_io_error_on_multi_commit_banner() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    fs::write(repo.join("git-factor"), "").expect("write fake exe");

    let sha_a = "a".repeat(40);
    let sha_b = "b".repeat(40);

    // Match the production `editor_path()` behavior, which canonicalizes the exe path.
    let exe = fs::canonicalize(repo.join("git-factor")).expect("canonicalize exe");
    let editor = exe
        .parent()
        .expect("exe parent")
        .join("git-sequence-editor");
    let seq_editor = format!(
        "{} {} {} {} {}",
        shell_quote(editor.to_string_lossy().as_ref()),
        shell_quote("--edit"),
        shell_quote("aaaaaaa"),
        shell_quote("--edit"),
        shell_quote("bbbbbbb"),
    );

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha_b}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "a..b"],
            repo,
            &format!("{sha_a}\n{sha_b}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha_a, &sha_b],
            repo,
            &format!("{sha_a}\n{sha_b}\n"),
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha_a, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha_b, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha_a}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha_b}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_output("git", &["rev-parse", "--short", &sha_a], repo, "aaaaaaa\n")
        .with_output("git", &["rev-parse", "--short", &sha_b], repo, "bbbbbbb\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &sha_a],
            repo,
            "msg\n",
        )
        .with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", "true"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha_a}^")],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &[
                "rebase",
                "--empty",
                "drop",
                "--interactive",
                "--no-autosquash",
                "--no-autostash",
                "--no-rebase-merges",
                "--no-stat",
                "--quiet",
                "--reschedule-failed-exec",
                "--exec",
                "true",
                &format!("{sha_a}^"),
            ],
            &[
                ("GIT_EDITOR", "false"),
                ("GIT_SEQUENCE_EDITOR", &seq_editor),
            ],
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n")
        .with_status("git", &["reset", "--quiet", "HEAD~1"], &[], false, repo, 0)
        .with_output("git", &["diff", "--stat"], repo, "")
        .with_output(
            "git",
            &["ls-files", "--others", "--exclude-standard"],
            repo,
            "",
        );

    let io = FailingIo;
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
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("a..b"),
        ],
    )
    .expect_err("expected io failure");

    assert!(
        matches!(&err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
        "err was: {err:?}"
    );
}

#[test]
fn main_entry_with_prints_error_when_ctx_cannot_be_built() {
    let io = TestIo::default();
    let env = FailingEnv { message: "no cwd" };
    let ctx = ctx_from_parts(&env, &REAL_RUNNER, &io, &REAL_FS);

    let code = main_entry_with_vec(&io, ctx, vec![OsString::from("git-factor")]);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(
        io.stderr(),
        "git command failed: cannot resolve cwd: no cwd\n"
    );
    assert_eq!(io.stdout(), "");
}

#[test]
fn cmd_abort_reports_rebase_hint_when_rebase_still_active() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");

    let sha = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["reset", "--hard", "--quiet", &sha],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
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

    let code = cmd_abort_in(&ctx).expect("abort should succeed");

    assert_eq!(code, EXIT_OK);
    assert_eq!(
        io.stdout(),
        concat!(
            "FACTOR: Session aborted for current commit step.\n",
            "FACTOR: Rebase still active. To abort full rebase, run: git rebase --abort\n"
        )
    );
    assert!(io.stderr().is_empty(), "stderr should be empty");
    assert!(
        !state_dir.exists(),
        "factor state dir should be removed after abort"
    );
}

#[test]
fn cmd_status_reports_no_active_session_message() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(&git_dir).expect("create git dir");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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

    let code = cmd_status_in(&ctx).expect("status should succeed");

    assert_eq!(code, EXIT_OK);
    assert_eq!(io.stdout(), "FACTOR: No active session.\n");
    assert!(io.stderr().is_empty(), "stderr should be empty");
}

#[test]
fn cmd_status_reports_active_session_fields() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");

    let sha = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");
    fs::write(state_dir.join("split_count"), "2\n").expect("write split count");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires rebase");
    fs::write(state_dir.join("is_root"), "true\n").expect("write is root");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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

    let code = cmd_status_in(&ctx).expect("status should succeed");

    assert_eq!(code, EXIT_OK);
    assert_eq!(
        io.stdout(),
        format!(
            concat!(
                "FACTOR: Active session.\n",
                "CURRENT_COMMIT: {}\n",
                "CURRENT_INDEX: 0\n",
                "SPLIT_COUNT: 2\n",
                "REQUIRES_REBASE: false\n",
                "REBASE_IN_PROGRESS: true\n",
                "IS_ROOT: true\n"
            ),
            sha
        )
    );
    assert!(io.stderr().is_empty(), "stderr should be empty");
}

#[test]
fn advance_to_next_commit_prints_untracked_changes_when_present() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n{}\n", "a".repeat(40), "b".repeat(40)),
    )
    .expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "3\n").expect("write split_count");

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n")
        .with_status("git", &["reset", "--quiet", "HEAD~1"], &[], false, repo, 0)
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &"b".repeat(40)],
            repo,
            "original message\n",
        )
        .with_output(
            "git",
            &["diff", "--stat"],
            repo,
            "file.txt | 1 +\n1 file changed, 1 insertion(+)\n",
        )
        .with_output(
            "git",
            &["ls-files", "--others", "--exclude-standard"],
            repo,
            "newfile.txt\n",
        )
        .with_output(
            "git",
            &["rev-parse", "--short", &"b".repeat(40)],
            repo,
            "bbbbbbb\n",
        )
        .with_output(
            "git",
            &["rev-parse", "--show-toplevel"],
            repo,
            repo.to_string_lossy().as_ref(),
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

    let has_next = advance_to_next_commit_in(&ctx, &state_dir).expect("advance ok");
    assert!(has_next, "expected another commit to be ready");

    assert_eq!(
        io.stdout(),
        concat!(
            "FACTOR: Previous commit split into 3 commits.\n",
            "FACTOR: Now splitting bbbbbbb.\n",
            "ORIGINAL MESSAGE: original message\n",
            "UNSTAGED:\n",
            "  file.txt | 1 +\n",
            "  1 file changed, 1 insertion(+)\n",
            "UNTRACKED:\n",
            "  newfile.txt\n",
            "\n",
            "NEXT: Stage changes for the next commit, then run:\n",
            "  git factor --continue --message \"type: description\"\n",
            "\n",
            "HINTS:\n",
            "  - Find the ONE smallest addition nothing depends on\n",
            "  - Target 15-30 lines (50 max)\n",
            "  - Message: single concrete action, no \"and\"/\"or\"\n",
            "  - Verify: git log --oneline | wc -l\n",
            "  - NEVER use git commit. ONLY use git factor --continue.\n",
            "  REMAINING: 1 file changed, 1 insertion(+)\n",
            "  RECOVERY: git factor --abort\n"
        )
    );
    assert_eq!(io.stderr(), "");
}

#[test]
fn advance_to_next_commit_propagates_io_error_when_outln_fails_mid_rebase() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n{}\n", "a".repeat(40), "b".repeat(40)),
    )
    .expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "3\n").expect("write split_count");

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n")
        .with_status("git", &["reset", "--quiet", "HEAD~1"], &[], false, repo, 0)
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &"b".repeat(40)],
            repo,
            "original message\n",
        )
        .with_output("git", &["diff", "--stat"], repo, "")
        .with_output(
            "git",
            &["ls-files", "--others", "--exclude-standard"],
            repo,
            "",
        )
        .with_output(
            "git",
            &["rev-parse", "--short", &"b".repeat(40)],
            repo,
            "bbbbbbb\n",
        );

    let io = FailingIo;
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

    let err = advance_to_next_commit_in(&ctx, &state_dir).expect_err("expected io failure");
    assert!(
        matches!(&err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
        "err was: {err:?}"
    );
}

#[test]
fn advance_to_next_commit_propagates_io_error_when_outln_fails_after_rebase_finishes() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("split_count"), "3\n").expect("write split_count");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");

    let io = FailingIo;
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

    let err = advance_to_next_commit_in(&ctx, &state_dir).expect_err("expected io failure");
    assert!(
        matches!(&err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
        "err was: {err:?}"
    );
}

#[test]
fn advance_to_next_commit_errors_on_current_index_overflow() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("current_index"), format!("{}\n", usize::MAX))
        .expect("write current_index");

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n")
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

    let err = advance_to_next_commit_in(&ctx, &state_dir).expect_err("expected overflow");
    assert_eq!(
        err.to_string(),
        "git command failed: current_index overflow"
    );
}

#[test]
fn advance_to_next_commit_errors_when_rebase_is_required_but_not_in_progress() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(&git_dir).expect("create git dir");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");

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

    let err = advance_to_next_commit_in(&ctx, &state_dir).expect_err("expected no rebase error");
    assert_eq!(err.to_string(), "git command failed: no rebase in progress");
}

#[test]
fn advance_to_next_commit_omits_untracked_section_when_empty() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n{}\n", "a".repeat(40), "b".repeat(40)),
    )
    .expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "1\n").expect("write split_count");

    let runner = ScriptedRunner::default()
        .with_status(
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n")
        .with_status("git", &["reset", "--quiet", "HEAD~1"], &[], false, repo, 0)
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &"b".repeat(40)],
            repo,
            "msg\n",
        )
        .with_output(
            "git",
            &["diff", "--stat"],
            repo,
            "file.txt | 1 +\n1 file changed, 1 insertion(+)\n",
        )
        .with_output(
            "git",
            &["ls-files", "--others", "--exclude-standard"],
            repo,
            "",
        )
        .with_output(
            "git",
            &["rev-parse", "--short", &"b".repeat(40)],
            repo,
            "bbbbbbb\n",
        )
        .with_output(
            "git",
            &["rev-parse", "--show-toplevel"],
            repo,
            repo.to_string_lossy().as_ref(),
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

    let has_next = advance_to_next_commit_in(&ctx, &state_dir).expect("advance ok");
    assert!(has_next, "expected another commit to be ready");

    assert_eq!(
        io.stdout(),
        concat!(
            "FACTOR: Previous commit split into 1 commits.\n",
            "FACTOR: Now splitting bbbbbbb.\n",
            "ORIGINAL MESSAGE: msg\n",
            "UNSTAGED:\n",
            "  file.txt | 1 +\n",
            "  1 file changed, 1 insertion(+)\n",
            "\n",
            "NEXT: Stage changes for the next commit, then run:\n",
            "  git factor --continue --message \"type: description\"\n",
            "\n",
            "HINTS:\n",
            "  - Find the ONE smallest addition nothing depends on\n",
            "  - Target 15-30 lines (50 max)\n",
            "  - Message: single concrete action, no \"and\"/\"or\"\n",
            "  - Verify: git log --oneline | wc -l\n",
            "  - NEVER use git commit. ONLY use git factor --continue.\n",
            "  REMAINING: 1 file changed, 1 insertion(+)\n",
            "  RECOVERY: git factor --abort\n"
        )
    );
    assert_eq!(io.stderr(), "");
}

#[test]
fn rehydrate_pool_preserving_index_succeeds_when_cherry_pick_succeeds() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();

    let runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, "deadbeef\n")
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(40),
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 0)
        .with_status("git", &["read-tree", "deadbeef"], &[], false, repo, 0);

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

    let commit = CommitSha::new("a".repeat(40)).expect("sha");
    let result = rehydrate_pool_preserving_index(&ctx, &commit);

    assert!(result.is_ok(), "rehydrate should succeed");
    assert_eq!(io.stdout(), "");
    assert_eq!(io.stderr(), "");
}

#[test]
fn rehydrate_pool_preserving_index_propagates_cherry_pick_status_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();

    let runner = ScriptedRunner::default().with_output("git", &["write-tree"], repo, "deadbeef\n");

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

    let commit = CommitSha::new("a".repeat(40)).expect("sha");
    let err = rehydrate_pool_preserving_index(&ctx, &commit).expect_err("expected error");

    assert!(
        matches!(
            &err,
            FactorError::GitCommand(msg)
                if msg.contains("git cherry-pick:") && msg.contains("unexpected status call")
        ),
        "err was: {err:?}"
    );
}

#[test]
fn rehydrate_pool_preserving_index_reports_conflicts_when_unmerged_paths_exist() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();

    let runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, "deadbeef\n")
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(40),
            ],
            &[],
            false,
            repo,
            1,
        )
        .with_output(
            "git",
            &["diff", "--name-only", "--diff-filter=U"],
            repo,
            "conflict.txt\n",
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

    let commit = CommitSha::new("a".repeat(40)).expect("sha");
    let err = rehydrate_pool_preserving_index(&ctx, &commit).expect_err("expected conflict error");

    assert_eq!(
        err.to_string(),
        "git command failed: rehydrate cherry-pick left conflicts:\nconflict.txt"
    );
}

#[test]
fn rehydrate_pool_preserving_index_reports_quit_failure_after_cherry_pick_failure() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();

    let runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, "deadbeef\n")
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(40),
            ],
            &[],
            false,
            repo,
            1,
        )
        .with_output("git", &["diff", "--name-only", "--diff-filter=U"], repo, "")
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 128);

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

    let commit = CommitSha::new("a".repeat(40)).expect("sha");
    let err = rehydrate_pool_preserving_index(&ctx, &commit).expect_err("expected quit error");

    assert!(
        matches!(err, FactorError::GitCommand(ref msg) if msg.contains("git cherry-pick --quit failed (exit ")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn rehydrate_pool_preserving_index_reports_read_tree_failure() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();

    let runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, "deadbeef\n")
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(40),
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 0)
        .with_status("git", &["read-tree", "deadbeef"], &[], false, repo, 2);

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

    let commit = CommitSha::new("a".repeat(40)).expect("sha");
    let err = rehydrate_pool_preserving_index(&ctx, &commit).expect_err("expected read-tree error");

    assert!(
        matches!(err, FactorError::GitCommand(ref msg) if msg.contains("git read-tree failed (exit ")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn rehydrate_pool_preserving_index_reports_write_tree_output_error() {
    let dir = TempDir::new().expect("tempdir");
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

    let commit = CommitSha::new("a".repeat(40)).expect("sha");
    let err =
        rehydrate_pool_preserving_index(&ctx, &commit).expect_err("expected write-tree error");

    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git write-tree:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn rehydrate_pool_preserving_index_propagates_unmerged_query_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();

    let runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, "deadbeef\n")
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(40),
            ],
            &[],
            false,
            repo,
            1,
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

    let commit = CommitSha::new("a".repeat(40)).expect("sha");
    let err =
        rehydrate_pool_preserving_index(&ctx, &commit).expect_err("expected unmerged query error");

    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git diff:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn increment_split_count_in_state_propagates_state_write_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).expect("create state dir");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");

    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let fs = FailingWriteForFileFs {
        file_name: "split_count",
        message: "split_count write failed",
    };
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &fs,
    };

    let err =
        increment_split_count_in_state(&ctx, &state_dir).expect_err("expected state write error");
    assert!(
        matches!(&err, FactorError::StateWrite(inner) if inner.to_string().contains("split_count write failed")),
        "err was: {err:?}"
    );
}

#[test]
fn capture_expected_tree_in_state_propagates_git_output_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).expect("create state dir");

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

    let err =
        capture_expected_tree_in_state(&ctx, &state_dir).expect_err("expected git output error");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git rev-parse:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn capture_expected_tree_in_state_propagates_state_write_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).expect("create state dir");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "tree\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let fs = FailingWriteForFileFs {
        file_name: "expected_tree",
        message: "expected_tree write failed",
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &fs,
    };

    let err =
        capture_expected_tree_in_state(&ctx, &state_dir).expect_err("expected state write error");
    assert!(
        matches!(&err, FactorError::StateWrite(inner) if inner.to_string().contains("expected_tree write failed")),
        "err was: {err:?}"
    );
}

#[test]
fn write_state_pairs_propagates_first_state_write_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).expect("create state dir");

    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let fs = FailingWriteForFileFs {
        file_name: "split_count",
        message: "split_count write failed",
    };
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &fs,
    };

    let err = write_state_pairs(
        &ctx,
        &state_dir,
        &[
            ("current_index", "0"),
            ("split_count", "1"),
            ("exec", "true"),
        ],
    )
    .expect_err("expected state write error");
    assert!(
        matches!(&err, FactorError::StateWrite(inner) if inner.to_string().contains("split_count write failed")),
        "err was: {err:?}"
    );
}

#[test]
fn commit_sha_new_validates_length_and_hex() {
    assert!(
        CommitSha::new("a".repeat(40)).is_ok(),
        "40 hex should be ok"
    );
    assert!(
        CommitSha::new("a".repeat(39)).is_err(),
        "wrong length should fail"
    );
    assert!(
        CommitSha::new("g".repeat(40)).is_err(),
        "non-hex should fail"
    );
}

#[test]
fn build_rebase_args_uses_parent_for_non_root_and_root_flag_for_root() {
    let exec = NonEmpty::new(NonEmptyString::try_from("true".to_owned()).expect("non-empty"));

    let non_root = build_rebase_args(&exec, "abc1234^", false);
    assert_eq!(
        non_root,
        vec![
            "rebase",
            "--empty",
            "drop",
            "--interactive",
            "--no-autosquash",
            "--no-autostash",
            "--no-rebase-merges",
            "--no-stat",
            "--quiet",
            "--reschedule-failed-exec",
            "--exec",
            "true",
            "abc1234^",
        ]
    );

    let root = build_rebase_args(&exec, "abc1234^", true);
    assert_eq!(
        root,
        vec![
            "rebase",
            "--empty",
            "drop",
            "--interactive",
            "--no-autosquash",
            "--no-autostash",
            "--no-rebase-merges",
            "--no-stat",
            "--quiet",
            "--reschedule-failed-exec",
            "--exec",
            "true",
            "--root",
        ]
    );
}

#[test]
fn remove_empty_root_is_noop_when_root_is_not_empty() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let ctx = ctx_for(repo);

    let status = Command::new("git")
        .args(["init"])
        .current_dir(repo)
        .status()
        .expect("git init");
    assert!(status.success(), "git init should succeed");

    drop(
        Command::new("git")
            .args(["config", "user.name", "Git Factor Tests"])
            .current_dir(repo)
            .status(),
    );
    drop(
        Command::new("git")
            .args(["config", "user.email", "git-factor-tests@example.invalid"])
            .current_dir(repo)
            .status(),
    );

    fs::write(repo.join("file.txt"), "one\n").expect("write file");
    assert!(
        Command::new("git")
            .args(["add", "file.txt"])
            .current_dir(repo)
            .status()
            .expect("git add")
            .success()
    );
    assert!(
        Command::new("git")
            .args(["commit", "--message", "chore: base"])
            .current_dir(repo)
            .status()
            .expect("git commit")
            .success()
    );

    let result = remove_empty_root_in(&ctx);

    assert!(result.is_ok(), "should be a no-op when root is not empty");
}

#[test]
fn run_with_args_maps_help_to_exit_ok() {
    let dir = TempDir::new().expect("tempdir");
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
        vec![OsString::from("git-factor"), OsString::from("--help")],
    );
    assert_eq!(code, EXIT_OK);
    assert!(io.stdout().contains("WORKFLOW:"), "help should be printed");
    assert!(io.stderr().is_empty(), "help should not print to stderr");
}

#[test]
fn run_with_args_invalid_flag_writes_to_stderr_and_returns_usage() {
    let dir = TempDir::new().expect("tempdir");
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
        vec![
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
fn run_with_args_rejects_abort_when_combined_with_status() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--abort"),
            OsString::from("--status"),
        ],
    )
    .expect_err("abort/status should be rejected");

    assert_eq!(
        err.to_string(),
        "--abort cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_abort_when_combined_with_continue() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--abort"),
            OsString::from("--continue"),
        ],
    )
    .expect_err("abort/continue should be rejected");

    assert_eq!(
        err.to_string(),
        "--abort cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_abort_when_combined_with_finish() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--abort"),
            OsString::from("--finish"),
        ],
    )
    .expect_err("abort/finish should be rejected");

    assert_eq!(
        err.to_string(),
        "--abort cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_abort_when_combined_with_exec() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--abort"),
            OsString::from("--exec"),
            OsString::from("true"),
        ],
    )
    .expect_err("abort/exec should be rejected");

    assert_eq!(
        err.to_string(),
        "--abort cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_abort_when_combined_with_commit() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--abort"),
            OsString::from("HEAD"),
        ],
    )
    .expect_err("abort/commit should be rejected");

    assert_eq!(
        err.to_string(),
        "--abort cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_status_when_combined_with_message() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--status"),
            OsString::from("--message"),
            OsString::from("msg"),
        ],
    )
    .expect_err("status/message should be rejected");

    assert_eq!(
        err.to_string(),
        "--status cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_status_when_combined_with_continue() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--status"),
            OsString::from("--continue"),
        ],
    )
    .expect_err("status/continue should be rejected");

    assert_eq!(
        err.to_string(),
        "--status cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_status_when_combined_with_finish() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--status"),
            OsString::from("--finish"),
        ],
    )
    .expect_err("status/finish should be rejected");

    assert_eq!(
        err.to_string(),
        "--status cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_status_when_combined_with_exec() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--status"),
            OsString::from("--exec"),
            OsString::from("true"),
        ],
    )
    .expect_err("status/exec should be rejected");

    assert_eq!(
        err.to_string(),
        "--status cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_status_when_combined_with_commit() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--status"),
            OsString::from("HEAD"),
        ],
    )
    .expect_err("status/commit should be rejected");

    assert_eq!(
        err.to_string(),
        "--status cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_finish_when_combined_with_continue() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
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
    .expect_err("finish/continue should be rejected");

    assert_eq!(
        err.to_string(),
        "--finish cannot be combined with --continue, --exec, or COMMIT"
    );
}

#[test]
fn run_with_args_rejects_finish_when_combined_with_exec() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
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
    .expect_err("finish/exec should be rejected");

    assert_eq!(
        err.to_string(),
        "--finish cannot be combined with --continue, --exec, or COMMIT"
    );
}

#[test]
fn run_with_args_rejects_finish_when_combined_with_commit() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
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
    .expect_err("finish/commit should be rejected");

    assert_eq!(
        err.to_string(),
        "--finish cannot be combined with --continue, --exec, or COMMIT"
    );
}

#[test]
fn run_with_args_rejects_continue_when_combined_with_exec() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--continue"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("--message"),
            OsString::from("msg"),
        ],
    )
    .expect_err("continue/exec should be rejected");

    assert_eq!(
        err.to_string(),
        "--continue cannot be combined with --exec or COMMIT"
    );
}

#[test]
fn run_with_args_rejects_continue_when_combined_with_commit() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--continue"),
            OsString::from("--message"),
            OsString::from("msg"),
            OsString::from("HEAD"),
        ],
    )
    .expect_err("continue/commit should be rejected");

    assert_eq!(
        err.to_string(),
        "--continue cannot be combined with --exec or COMMIT"
    );
}

#[test]
fn run_with_args_rejects_continue_without_message() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![OsString::from("git-factor"), OsString::from("--continue")],
    )
    .expect_err("continue without message should be rejected");

    assert_eq!(err.to_string(), "--continue requires --message <MSG>");
}

#[test]
fn run_with_args_rejects_message_when_not_continuing_or_finishing() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("--message"),
            OsString::from("msg"),
        ],
    )
    .expect_err("message without continue/finish should be rejected");

    assert_eq!(
        err.to_string(),
        "--message can only be used with --continue or --finish"
    );
}

#[test]
fn run_with_args_errors_when_exec_is_missing() {
    let dir = TempDir::new().expect("tempdir");
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
        vec![OsString::from("git-factor"), OsString::from("HEAD")],
    );
    assert_eq!(code, EXIT_USAGE);
    let stderr = io.stderr();
    assert!(stderr.contains("--exec <COMMAND> is required"));
}

#[test]
fn run_with_args_defaults_missing_commit_to_head() {
    let dir = TempDir::new().expect("tempdir");
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
        vec![
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
fn run_with_args_without_user_args_prints_help() {
    let dir = TempDir::new().expect("tempdir");
    let io = TestIo::default();
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };
    let code = run_and_report_with_args_vec(&ctx, vec![OsString::from("git-factor")]);
    assert_eq!(code, EXIT_OK);
    assert!(io.stdout().contains("WORKFLOW:"), "help should be printed");
    assert!(io.stderr().is_empty(), "stderr should be empty");
}

#[test]
fn run_with_args_abort_delegates_to_abort_handler() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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
    .expect_err("abort should delegate to command handler");
    assert!(
        matches!(err, FactorError::NoActiveSession),
        "err was: {err:?}"
    );
}

#[test]
fn run_with_args_status_delegates_to_status_handler() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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
        vec![OsString::from("git-factor"), OsString::from("--status")],
    );
    assert_eq!(code, EXIT_OK);
    assert_eq!(io.stdout(), "FACTOR: No active session.\n");
}

#[test]
fn run_with_args_finish_delegates_to_finish_handler() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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
    .expect_err("finish should delegate to command handler");
    assert!(
        matches!(err, FactorError::NoActiveSession),
        "err was: {err:?}"
    );
}

#[test]
fn run_with_args_continue_delegates_to_continue_handler() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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
    .expect_err("continue should delegate to command handler");
    assert!(
        matches!(err, FactorError::NoActiveSession),
        "err was: {err:?}"
    );
}

#[test]
fn run_with_args_start_accepts_explicit_commit_ref() {
    let dir = TempDir::new().expect("tempdir");
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
        vec![
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
    let cwd = env::current_dir().expect("current_dir");
    let ctx = real_ctx().expect("real_ctx");
    assert_eq!(ctx.cwd, cwd);
}

#[test]
fn main_entry_is_callable() {
    let code = main_entry();
    assert!((0..=255).contains(&code), "exit code should be in range");
}

#[test]
fn validate_exec_syntax_reports_spawn_failure_as_git_command() {
    let dir = TempDir::new().expect("tempdir");
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

    let err = validate_exec_syntax(&ctx, "echo hi").expect_err("expected spawn failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("bash syntax check:") && msg.contains("unexpected status call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_continue_errors_on_split_count_overflow() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), format!("{}\n", u32::MAX)).expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");

    let original = "a".repeat(40);
    let commit_meta = "a\0b\0c\0d\0e\0f";
    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = NonEmpty::new(message);

    let envs = [
        ("GIT_AUTHOR_NAME", "a"),
        ("GIT_AUTHOR_EMAIL", "b"),
        ("GIT_AUTHOR_DATE", "c"),
        ("GIT_COMMITTER_NAME", "d"),
        ("GIT_COMMITTER_EMAIL", "e"),
        ("GIT_COMMITTER_DATE", "f"),
    ];

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["checkout-index", "--all", "--force", "--quiet"],
            &[],
            false,
            repo,
            0,
        )
        .with_output(
            "git",
            &["diff", "--diff-filter=D", "--name-only", "--staged"],
            repo,
            "",
        )
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original.as_str(),
            ],
            repo,
            commit_meta,
        )
        .with_status(
            "git",
            &["commit", "--quiet", "--message", "test: message"],
            &envs,
            false,
            repo,
            0,
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

    let err = cmd_continue_in(&ctx, &messages).expect_err("expected overflow");
    assert!(
        matches!(&err, FactorError::GitCommand(err_msg) if err_msg == "split_count overflow"),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_continue_propagates_restore_status_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");

    let original = "a".repeat(40);
    let commit_meta = "a\0b\0c\0d\0e\0f";
    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = NonEmpty::new(message);

    let envs = [
        ("GIT_AUTHOR_NAME", "a"),
        ("GIT_AUTHOR_EMAIL", "b"),
        ("GIT_AUTHOR_DATE", "c"),
        ("GIT_COMMITTER_NAME", "d"),
        ("GIT_COMMITTER_EMAIL", "e"),
        ("GIT_COMMITTER_DATE", "f"),
    ];

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["checkout-index", "--all", "--force", "--quiet"],
            &[],
            false,
            repo,
            0,
        )
        .with_output(
            "git",
            &["diff", "--diff-filter=D", "--name-only", "--staged"],
            repo,
            "",
        )
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original.as_str(),
            ],
            repo,
            commit_meta,
        )
        .with_status(
            "git",
            &["commit", "--quiet", "--message", "test: message"],
            &envs,
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n")
        .with_output(
            "git",
            &["rev-parse", &format!("{original}^{{tree}}")],
            repo,
            "original_tree\n",
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

    let err = cmd_continue_in(&ctx, &messages).expect_err("expected restore status error");
    assert!(
        matches!(
            &err,
            FactorError::GitCommand(msg)
                if msg.contains("git restore:") && msg.contains("unexpected status call")
        ),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_errors_on_split_count_overflow() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), format!("{}\n", u32::MAX)).expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");

    let original = "a".repeat(40);
    let commit_meta = "a\0b\0c\0d\0e\0f";

    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = [message];

    let envs = [
        ("GIT_AUTHOR_NAME", "a"),
        ("GIT_AUTHOR_EMAIL", "b"),
        ("GIT_AUTHOR_DATE", "c"),
        ("GIT_COMMITTER_NAME", "d"),
        ("GIT_COMMITTER_EMAIL", "e"),
        ("GIT_COMMITTER_DATE", "f"),
    ];

    let expected_tree_output = "tree\n";
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", &format!("{original}^{{tree}}")],
            repo,
            expected_tree_output,
        )
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_output("git", &["write-tree"], repo, expected_tree_output)
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original.as_str(),
            ],
            repo,
            commit_meta,
        )
        .with_status(
            "git",
            &["commit", "--quiet", "--message", "test: message"],
            &envs,
            false,
            repo,
            0,
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

    let err = cmd_finish_in(&ctx, &messages).expect_err("expected overflow");
    assert!(
        matches!(&err, FactorError::GitCommand(err_msg) if err_msg == "split_count overflow"),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_propagates_restore_status_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");

    let original = "a".repeat(40);
    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = [message];

    let expected_tree_output = "tree\n";
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", &format!("{original}^{{tree}}")],
            repo,
            expected_tree_output,
        )
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
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

    let err = cmd_finish_in(&ctx, &messages).expect_err("expected restore status error");
    assert!(
        matches!(
            &err,
            FactorError::GitCommand(msg)
                if msg.contains("git restore:") && msg.contains("unexpected status call")
        ),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_propagates_git_commit_preserving_metadata_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");

    let original = "a".repeat(40);
    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = [message];

    let expected_tree_output = "tree\n";
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", &format!("{original}^{{tree}}")],
            repo,
            expected_tree_output,
        )
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_output("git", &["write-tree"], repo, expected_tree_output)
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_status("bash", &["-c", "true"], &[], false, repo, 0);

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

    let err = cmd_finish_in(&ctx, &messages).expect_err("expected metadata lookup to fail");
    assert!(
        matches!(
            &err,
            FactorError::GitCommand(msg)
                if msg.contains("git show:") && msg.contains("unexpected output call")
        ),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_abort_errors_when_no_active_session() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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

    let err = cmd_abort_in(&ctx).expect_err("expected no active session");
    assert!(
        matches!(err, FactorError::NoActiveSession),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_continue_errors_when_no_staged_changes() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");

    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = NonEmpty::new(message);

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 0);
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

    let err = cmd_continue_in(&ctx, &messages).expect_err("expected no staged changes");
    assert!(
        matches!(err, FactorError::NoStagedChanges),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_errors_when_no_active_session() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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

    let err = cmd_finish_in(&ctx, &[]).expect_err("expected no active session");
    assert!(
        matches!(err, FactorError::NoActiveSession),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_errors_when_factor_session_is_already_active() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    fs::create_dir_all(repo.join(".git").join("factor")).expect("create active factor dir");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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

    let exec = NonEmpty::new(NonEmptyString::try_from("true".to_owned()).expect("exec"));
    let commits = NonEmpty::new(NonEmptyString::try_from("HEAD".to_owned()).expect("commit"));

    let err = cmd_start_in(&ctx, &exec, &commits).expect_err("expected active session error");
    assert!(
        matches!(err, FactorError::ActiveSession),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_errors_when_rebase_is_already_active() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    fs::create_dir_all(repo.join(".git").join("rebase-merge")).expect("create rebase-merge");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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

    let exec = NonEmpty::new(NonEmptyString::try_from("true".to_owned()).expect("exec"));
    let commits = NonEmpty::new(NonEmptyString::try_from("HEAD".to_owned()).expect("commit"));

    let err = cmd_start_in(&ctx, &exec, &commits).expect_err("expected active rebase error");
    assert!(matches!(err, FactorError::ActiveRebase), "err was: {err:?}");
}

#[test]
fn expected_tree_for_current_step_prefers_state_file() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("expected_tree"), "tree-from-state\n").expect("write expected_tree");

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
    let original = CommitSha::new("a".repeat(40)).expect("sha");

    let tree = expected_tree_for_current_step(&ctx, &state_dir, &original)
        .expect("expected tree from state");
    assert_eq!(tree, "tree-from-state");
}

#[test]
fn expected_tree_for_current_step_falls_back_to_original_commit_tree() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");

    let original = "b".repeat(40);
    let runner = ScriptedRunner::default().with_output(
        "git",
        &["rev-parse", &format!("{original}^{{tree}}")],
        repo,
        "tree-from-original\n",
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
    let original = CommitSha::new(original).expect("sha");

    let tree = expected_tree_for_current_step(&ctx, &state_dir, &original)
        .expect("expected tree fallback");
    assert_eq!(tree, "tree-from-original");
}

#[test]
fn git_commit_preserving_metadata_supports_allow_empty() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let original = "a".repeat(40);
    let commit_meta = "a\0b\0c\0d\0e\0f";
    let message = NonEmptyString::try_from("feat: message".to_owned()).expect("non-empty");
    let messages = NonEmpty::new(message);

    let envs = [
        ("GIT_AUTHOR_NAME", "a"),
        ("GIT_AUTHOR_EMAIL", "b"),
        ("GIT_AUTHOR_DATE", "c"),
        ("GIT_COMMITTER_NAME", "d"),
        ("GIT_COMMITTER_EMAIL", "e"),
        ("GIT_COMMITTER_DATE", "f"),
    ];
    let runner = ScriptedRunner::default()
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original.as_str(),
            ],
            repo,
            commit_meta,
        )
        .with_status(
            "git",
            &[
                "commit",
                "--quiet",
                "--allow-empty",
                "--message",
                "feat: message",
            ],
            &envs,
            false,
            repo,
            0,
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
    let commit = CommitSha::new(original).expect("sha");

    git_commit_preserving_metadata(&ctx, &commit, &messages, true).expect("allow-empty commit");
}

#[test]
fn git_commit_preserving_metadata_errors_on_nonzero_commit_status() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let original = "a".repeat(40);
    let commit_meta = "a\0b\0c\0d\0e\0f";
    let message = NonEmptyString::try_from("feat: message".to_owned()).expect("non-empty");
    let messages = NonEmpty::new(message);

    let envs = [
        ("GIT_AUTHOR_NAME", "a"),
        ("GIT_AUTHOR_EMAIL", "b"),
        ("GIT_AUTHOR_DATE", "c"),
        ("GIT_COMMITTER_NAME", "d"),
        ("GIT_COMMITTER_EMAIL", "e"),
        ("GIT_COMMITTER_DATE", "f"),
    ];
    let runner = ScriptedRunner::default()
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original.as_str(),
            ],
            repo,
            commit_meta,
        )
        .with_status(
            "git",
            &["commit", "--quiet", "--message", "feat: message"],
            &envs,
            false,
            repo,
            1,
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
    let commit = CommitSha::new(original).expect("sha");

    let err = git_commit_preserving_metadata(&ctx, &commit, &messages, false)
        .expect_err("expected nonzero commit status");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.starts_with("git commit failed (exit ")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_continue_reports_tree_mismatch_after_restore() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");

    let original = "a".repeat(40);
    let commit_meta = "a\0b\0c\0d\0e\0f";
    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = NonEmpty::new(message);

    let envs = [
        ("GIT_AUTHOR_NAME", "a"),
        ("GIT_AUTHOR_EMAIL", "b"),
        ("GIT_AUTHOR_DATE", "c"),
        ("GIT_COMMITTER_NAME", "d"),
        ("GIT_COMMITTER_EMAIL", "e"),
        ("GIT_COMMITTER_DATE", "f"),
    ];

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["checkout-index", "--all", "--force", "--quiet"],
            &[],
            false,
            repo,
            0,
        )
        .with_output(
            "git",
            &["diff", "--diff-filter=D", "--name-only", "--staged"],
            repo,
            "",
        )
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original.as_str(),
            ],
            repo,
            commit_meta,
        )
        .with_status(
            "git",
            &["commit", "--quiet", "--message", "test: message"],
            &envs,
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n")
        .with_output(
            "git",
            &["rev-parse", &format!("{original}^{{tree}}")],
            repo,
            "expected_tree\n",
        )
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_output("git", &["write-tree"], repo, "restored_tree\n");
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

    let err = cmd_continue_in(&ctx, &messages).expect_err("expected tree mismatch");
    assert!(
        matches!(
            &err,
            FactorError::TreeHashMismatch { actual, expected }
                if actual == "restored_tree" && expected == "expected_tree"
        ),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_abort_io_failures_cover_output_paths() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");

    let sha = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");

    let base_runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["reset", "--hard", "--quiet", &sha],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        );
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    for fail_at in 1..=4 {
        fs::create_dir_all(&state_dir).expect("recreate factor dir");
        fs::write(state_dir.join("commits"), format!("{sha}\n")).expect("rewrite commits");
        fs::write(state_dir.join("current_index"), "0\n").expect("rewrite current index");

        let runner = NthRunnerFailure::new(base_runner.clone(), usize::MAX);
        let io = NthIoFailure::new(fail_at);
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        let err = cmd_abort_in(&ctx).expect_err("expected io failure");
        assert!(
            matches!(&err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
            "err was: {err:?}"
        );
    }
}

#[test]
fn cmd_status_io_failures_cover_active_session_output_paths() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");

    let sha = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");
    fs::write(state_dir.join("split_count"), "2\n").expect("write split count");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires rebase");
    fs::write(state_dir.join("is_root"), "true\n").expect("write is root");

    let base_runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    for fail_at in 1..=14 {
        let runner = NthRunnerFailure::new(base_runner.clone(), usize::MAX);
        let io = NthIoFailure::new(fail_at);
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        let err = cmd_status_in(&ctx).expect_err("expected io failure");
        assert!(
            matches!(&err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
            "err was: {err:?}"
        );
    }
}

#[test]
fn advance_to_next_commit_runner_failures_cover_command_error_paths() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n{}\n", "a".repeat(40), "b".repeat(40)),
    )
    .expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "3\n").expect("write split_count");

    let base_runner = ScriptedRunner::default()
        .with_status(
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n")
        .with_status("git", &["reset", "--quiet", "HEAD~1"], &[], false, repo, 0)
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &"b".repeat(40)],
            repo,
            "original message\n",
        )
        .with_output(
            "git",
            &["diff", "--stat"],
            repo,
            "file.txt | 1 +\n1 file changed, 1 insertion(+)\n",
        )
        .with_output(
            "git",
            &["ls-files", "--others", "--exclude-standard"],
            repo,
            "newfile.txt\n",
        )
        .with_output(
            "git",
            &["rev-parse", "--short", &"b".repeat(40)],
            repo,
            "bbbbbbb\n",
        )
        .with_output(
            "git",
            &["rev-parse", "--show-toplevel"],
            repo,
            repo.to_string_lossy().as_ref(),
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    for fail_at in [2_usize, 4, 5, 6, 7, 8, 9, 10, 11] {
        fs::write(state_dir.join("current_index"), "0\n").expect("reset current_index");
        fs::write(state_dir.join("split_count"), "3\n").expect("reset split_count");

        let runner = NthRunnerFailure::new(base_runner.clone(), fail_at);
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };
        let err = advance_to_next_commit_in(&ctx, &state_dir)
            .expect_err("expected forced runner failure");
        assert!(is_forced_runner_failure(&err), "err was: {err:?}");
    }
}

#[test]
fn advance_to_next_commit_io_failures_cover_output_paths() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n{}\n", "a".repeat(40), "b".repeat(40)),
    )
    .expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "3\n").expect("write split_count");

    let base_runner = ScriptedRunner::default()
        .with_status(
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n")
        .with_status("git", &["reset", "--quiet", "HEAD~1"], &[], false, repo, 0)
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &"b".repeat(40)],
            repo,
            "original message\n",
        )
        .with_output(
            "git",
            &["diff", "--stat"],
            repo,
            "file.txt | 1 +\n1 file changed, 1 insertion(+)\n",
        )
        .with_output(
            "git",
            &["ls-files", "--others", "--exclude-standard"],
            repo,
            "newfile.txt\n",
        )
        .with_output(
            "git",
            &["rev-parse", "--short", &"b".repeat(40)],
            repo,
            "bbbbbbb\n",
        )
        .with_output(
            "git",
            &["rev-parse", "--show-toplevel"],
            repo,
            repo.to_string_lossy().as_ref(),
        );
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let mut io_failures = 0_u32;
    for fail_at in 1..=40 {
        fs::write(state_dir.join("current_index"), "0\n").expect("reset current_index");
        fs::write(state_dir.join("split_count"), "3\n").expect("reset split_count");

        let runner = NthRunnerFailure::new(base_runner.clone(), usize::MAX);
        let io = NthIoFailure::new(fail_at);
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        match advance_to_next_commit_in(&ctx, &state_dir) {
            Err(FactorError::Io(inner)) => {
                io_failures = io_failures.saturating_add(1);
                assert!(inner.to_string().contains("io fail"), "inner was: {inner}");
            }
            Ok(has_next) => {
                assert!(has_next, "expected to stay in rebase for this fixture");
            }
            Err(err) => panic!("unexpected error: {err:?}"),
        }
    }
    assert!(io_failures > 0, "expected at least one io failure");
}

#[test]
fn cmd_continue_runner_failures_cover_command_error_paths() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), "expected_tree\n").expect("write expected_tree");

    let original = "a".repeat(40);
    let commit_meta = "a\0b\0c\0d\0e\0f";
    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = NonEmpty::new(message);

    let envs = [
        ("GIT_AUTHOR_NAME", "a"),
        ("GIT_AUTHOR_EMAIL", "b"),
        ("GIT_AUTHOR_DATE", "c"),
        ("GIT_COMMITTER_NAME", "d"),
        ("GIT_COMMITTER_EMAIL", "e"),
        ("GIT_COMMITTER_DATE", "f"),
    ];

    let base_runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["checkout-index", "--all", "--force", "--quiet"],
            &[],
            false,
            repo,
            0,
        )
        .with_output(
            "git",
            &["diff", "--diff-filter=D", "--name-only", "--staged"],
            repo,
            "",
        )
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original.as_str(),
            ],
            repo,
            commit_meta,
        )
        .with_status(
            "git",
            &["commit", "--quiet", "--message", "test: message"],
            &envs,
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n")
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_output("git", &["write-tree"], repo, "expected_tree\n")
        .with_status("git", &["reset", "--quiet"], &[], false, repo, 0)
        .with_output("git", &["diff", "--stat"], repo, "file.txt | 1 +\n")
        .with_output(
            "git",
            &["ls-files", "--others", "--exclude-standard"],
            repo,
            "newfile.txt\n",
        )
        .with_output(
            "git",
            &["rev-parse", "--show-toplevel"],
            repo,
            repo.to_string_lossy().as_ref(),
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    for fail_at in 4..=16 {
        fs::write(state_dir.join("split_count"), "0\n").expect("reset split_count");
        let runner = NthRunnerFailure::new(base_runner.clone(), fail_at);
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        let err = cmd_continue_in(&ctx, &messages).expect_err("expected forced runner failure");
        assert!(is_forced_runner_failure(&err), "err was: {err:?}");
    }
}

#[test]
fn cmd_continue_io_failures_cover_exec_gate_failure_output_paths() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "false\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");

    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = NonEmpty::new(message);

    let base_runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["checkout-index", "--all", "--force", "--quiet"],
            &[],
            false,
            repo,
            0,
        )
        .with_output(
            "git",
            &["diff", "--diff-filter=D", "--name-only", "--staged"],
            repo,
            "",
        )
        .with_status("bash", &["-c", "false"], &[], false, repo, 1)
        .with_output("git", &["write-tree"], repo, "deadbeef\n")
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(40),
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 0)
        .with_status("git", &["read-tree", "deadbeef"], &[], false, repo, 0);
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let mut io_failures = 0_u32;
    for fail_at in 1..=20 {
        let runner = NthRunnerFailure::new(base_runner.clone(), usize::MAX);
        let io = NthIoFailure::new(fail_at);
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        match cmd_continue_in(&ctx, &messages) {
            Err(FactorError::Io(inner)) => {
                io_failures = io_failures.saturating_add(1);
                assert!(inner.to_string().contains("io fail"), "inner was: {inner}");
            }
            Err(FactorError::ExecFailed { .. }) => {}
            Err(err) => panic!("unexpected error: {err:?}"),
            Ok(code) => panic!("unexpected success code {code}"),
        }
    }
    assert!(io_failures > 0, "expected at least one io failure");
}

#[test]
fn cmd_continue_io_failures_cover_remaining_output_paths() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), "expected_tree\n").expect("write expected_tree");

    let original = "a".repeat(40);
    let commit_meta = "a\0b\0c\0d\0e\0f";
    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = NonEmpty::new(message);

    let envs = [
        ("GIT_AUTHOR_NAME", "a"),
        ("GIT_AUTHOR_EMAIL", "b"),
        ("GIT_AUTHOR_DATE", "c"),
        ("GIT_COMMITTER_NAME", "d"),
        ("GIT_COMMITTER_EMAIL", "e"),
        ("GIT_COMMITTER_DATE", "f"),
    ];

    let base_runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["checkout-index", "--all", "--force", "--quiet"],
            &[],
            false,
            repo,
            0,
        )
        .with_output(
            "git",
            &["diff", "--diff-filter=D", "--name-only", "--staged"],
            repo,
            "",
        )
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original.as_str(),
            ],
            repo,
            commit_meta,
        )
        .with_status(
            "git",
            &["commit", "--quiet", "--message", "test: message"],
            &envs,
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n")
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_output("git", &["write-tree"], repo, "expected_tree\n")
        .with_status("git", &["reset", "--quiet"], &[], false, repo, 0)
        .with_output("git", &["diff", "--stat"], repo, "file.txt | 1 +\n")
        .with_output(
            "git",
            &["ls-files", "--others", "--exclude-standard"],
            repo,
            "newfile.txt\n",
        )
        .with_output(
            "git",
            &["rev-parse", "--show-toplevel"],
            repo,
            repo.to_string_lossy().as_ref(),
        );
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let mut io_failures = 0_u32;
    for fail_at in 1..=40 {
        let runner = NthRunnerFailure::new(base_runner.clone(), usize::MAX);
        let io = NthIoFailure::new(fail_at);
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        match cmd_continue_in(&ctx, &messages) {
            Err(FactorError::Io(inner)) => {
                io_failures = io_failures.saturating_add(1);
                assert!(inner.to_string().contains("io fail"), "inner was: {inner}");
            }
            Ok(code) => assert_eq!(code, EXIT_OK),
            Err(err) => panic!("unexpected error: {err:?}"),
        }
    }
    assert!(io_failures > 0, "expected at least one io failure");
}

#[test]
fn cmd_finish_runner_failures_cover_command_error_paths() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), "expected_tree\n").expect("write expected_tree");

    let original = "a".repeat(40);
    let commit_meta = "a\0b\0c\0d\0e\0f";
    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = [message];

    let envs = [
        ("GIT_AUTHOR_NAME", "a"),
        ("GIT_AUTHOR_EMAIL", "b"),
        ("GIT_AUTHOR_DATE", "c"),
        ("GIT_COMMITTER_NAME", "d"),
        ("GIT_COMMITTER_EMAIL", "e"),
        ("GIT_COMMITTER_DATE", "f"),
    ];

    let base_runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_output("git", &["write-tree"], repo, "expected_tree\n")
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original.as_str(),
            ],
            repo,
            commit_meta,
        )
        .with_status(
            "git",
            &["commit", "--quiet", "--message", "test: message"],
            &envs,
            false,
            repo,
            0,
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    for fail_at in 3..=10 {
        fs::write(state_dir.join("split_count"), "0\n").expect("reset split_count");
        let runner = NthRunnerFailure::new(base_runner.clone(), fail_at);
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        let err = cmd_finish_in(&ctx, &messages).expect_err("expected forced runner failure");
        assert!(is_forced_runner_failure(&err), "err was: {err:?}");
    }
}

#[test]
fn cmd_finish_io_failures_cover_exec_gate_failure_output_paths() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "false\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), "expected_tree\n").expect("write expected_tree");

    let original = "a".repeat(40);
    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = [message];

    let base_runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_output("git", &["write-tree"], repo, "expected_tree\n")
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_status("bash", &["-c", "false"], &[], false, repo, 1);
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let mut io_failures = 0_u32;
    for fail_at in 1..=20 {
        let runner = NthRunnerFailure::new(base_runner.clone(), usize::MAX);
        let io = NthIoFailure::new(fail_at);
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        match cmd_finish_in(&ctx, &messages) {
            Err(FactorError::Io(inner)) => {
                io_failures = io_failures.saturating_add(1);
                assert!(inner.to_string().contains("io fail"), "inner was: {inner}");
            }
            Err(FactorError::ExecFailed { .. }) => {}
            Err(err) => panic!("unexpected error: {err:?}"),
            Ok(code) => panic!("unexpected success code {code}"),
        }
    }
    assert!(io_failures > 0, "expected at least one io failure");
}

#[test]
fn cmd_abort_runner_failures_cover_internal_question_mark_paths() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");

    let sha = "a".repeat(40);
    let base_runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["reset", "--hard", "--quiet", &sha],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    for fail_at in [2_usize, 3, 4] {
        fs::create_dir_all(&state_dir).expect("recreate factor dir");
        fs::write(state_dir.join("commits"), format!("{sha}\n")).expect("write commits");
        fs::write(state_dir.join("current_index"), "0\n").expect("write current index");

        let runner = NthRunnerFailure::new(base_runner.clone(), fail_at);
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        let err = cmd_abort_in(&ctx).expect_err("expected forced runner failure");
        assert!(is_forced_runner_failure(&err), "err was: {err:?}");
    }
}

#[test]
fn cmd_abort_errors_when_current_commit_state_is_missing() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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

    let err = cmd_abort_in(&ctx).expect_err("expected missing state file to fail");
    assert!(
        matches!(&err, FactorError::StateRead(inner) if inner.kind() == io::ErrorKind::NotFound),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_abort_propagates_start_head_state_read_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::create_dir_all(state_dir.join("start_head")).expect("create invalid start_head");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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

    let err = cmd_abort_in(&ctx).expect_err("expected start_head read failure");
    assert!(
        matches!(&err, FactorError::StateRead(inner) if inner.kind() != io::ErrorKind::NotFound),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_abort_propagates_requires_rebase_state_read_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::create_dir_all(state_dir.join("requires_rebase")).expect("create invalid requires_rebase");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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

    let err = cmd_abort_in(&ctx).expect_err("expected requires_rebase read failure");
    assert!(
        matches!(&err, FactorError::StateRead(inner) if inner.kind() != io::ErrorKind::NotFound),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_abort_propagates_started_rebase_state_read_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::create_dir_all(state_dir.join("started_rebase")).expect("create invalid started_rebase");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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

    let err = cmd_abort_in(&ctx).expect_err("expected started_rebase read failure");
    assert!(
        matches!(&err, FactorError::StateRead(inner) if inner.kind() != io::ErrorKind::NotFound),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_status_io_failure_on_no_active_session_covers_outln_error_path() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = FailingIo;
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

    let err = cmd_status_in(&ctx).expect_err("expected io failure");
    assert!(
        matches!(&err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_status_state_failures_cover_internal_question_mark_paths() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");
    fs::write(state_dir.join("split_count"), "1\n").expect("write split count");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("is_root"), "false\n").expect("write is_root");

    let base_runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let runner = NthRunnerFailure::new(base_runner.clone(), 2);
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let err = cmd_status_in(&ctx).expect_err("expected factor_dir_in failure");
    assert!(
        matches!(&err, FactorError::GitDir(msg) if msg.contains("forced runner failure")),
        "err was: {err:?}"
    );

    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("reset commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("reset current index");
    fs::write(state_dir.join("split_count"), "1\n").expect("reset split count");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("reset requires_rebase");
    fs::write(state_dir.join("is_root"), "false\n").expect("reset is_root");
    fs::remove_file(state_dir.join("commits")).expect("remove commits");
    let ctx = Ctx {
        runner: &base_runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let err = cmd_status_in(&ctx).expect_err("expected missing commits state");
    assert!(
        matches!(&err, FactorError::StateRead(inner) if inner.kind() == io::ErrorKind::NotFound),
        "err was: {err:?}"
    );

    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("restore commits");
    fs::write(state_dir.join("current_index"), "not-a-number\n").expect("corrupt current_index");
    let err = cmd_status_in(&ctx).expect_err("expected invalid current_index");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("current_index")),
        "err was: {err:?}"
    );

    fs::write(state_dir.join("current_index"), "0\n").expect("restore current_index");
    fs::write(state_dir.join("split_count"), "not-a-number\n").expect("corrupt split_count");
    let err = cmd_status_in(&ctx).expect_err("expected invalid split_count");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("split_count")),
        "err was: {err:?}"
    );

    fs::write(state_dir.join("split_count"), "1\n").expect("restore split_count");
    fs::write(state_dir.join("requires_rebase"), "not-bool\n").expect("corrupt requires_rebase");
    let err = cmd_status_in(&ctx).expect_err("expected invalid requires_rebase");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("requires_rebase")),
        "err was: {err:?}"
    );

    fs::write(state_dir.join("requires_rebase"), "false\n").expect("restore requires_rebase");
    fs::write(state_dir.join("is_root"), "not-bool\n").expect("corrupt is_root");
    let err = cmd_status_in(&ctx).expect_err("expected invalid is_root");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("is_root")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_continue_precondition_failures_cover_internal_question_mark_paths() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    let original = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{original}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");

    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = NonEmpty::new(message);
    let base_runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1);
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let runner = NthRunnerFailure::new(base_runner.clone(), 2);
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let err = cmd_continue_in(&ctx, &messages).expect_err("expected factor_dir_in failure");
    assert!(
        matches!(&err, FactorError::GitDir(msg) if msg.contains("forced runner failure")),
        "err was: {err:?}"
    );

    let ctx = Ctx {
        runner: &base_runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    fs::remove_file(state_dir.join("exec")).expect("remove exec");
    let err = cmd_continue_in(&ctx, &messages).expect_err("expected missing exec state");
    assert!(
        matches!(&err, FactorError::StateRead(inner) if inner.kind() == io::ErrorKind::NotFound),
        "err was: {err:?}"
    );

    fs::write(state_dir.join("exec"), "true\n").expect("restore exec");
    let runner = NthRunnerFailure::new(base_runner.clone(), 3);
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let err = cmd_continue_in(&ctx, &messages).expect_err("expected staged diff status failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("forced runner failure")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_precondition_failures_cover_internal_question_mark_paths() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    let original = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{original}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("expected_tree"), "tree\n").expect("write expected_tree");

    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = [message];
    let base_runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let runner = NthRunnerFailure::new(base_runner.clone(), 2);
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let err = cmd_finish_in(&ctx, &messages).expect_err("expected factor_dir_in failure");
    assert!(
        matches!(&err, FactorError::GitDir(msg) if msg.contains("forced runner failure")),
        "err was: {err:?}"
    );

    let ctx = Ctx {
        runner: &base_runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    fs::remove_file(state_dir.join("commits")).expect("remove commits");
    let err = cmd_finish_in(&ctx, &messages).expect_err("expected missing commits state");
    assert!(
        matches!(&err, FactorError::StateRead(inner) if inner.kind() == io::ErrorKind::NotFound),
        "err was: {err:?}"
    );

    fs::write(state_dir.join("commits"), format!("{original}\n")).expect("restore commits");
    fs::remove_file(state_dir.join("exec")).expect("remove exec");
    let err = cmd_finish_in(&ctx, &messages).expect_err("expected missing exec state");
    assert!(
        matches!(&err, FactorError::StateRead(inner) if inner.kind() == io::ErrorKind::NotFound),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_abort_omits_rebase_hint_when_rebase_is_not_active() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");

    let sha = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["reset", "--hard", "--quiet", &sha],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
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

    let code = cmd_abort_in(&ctx).expect("abort should succeed");
    assert_eq!(code, EXIT_OK);
    assert_eq!(
        io.stdout(),
        "FACTOR: Session aborted for current commit step.\n"
    );
    assert!(io.stderr().is_empty(), "stderr should be empty");
}

#[test]
fn cmd_continue_errors_when_rebase_is_required_but_not_active() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "true\n").expect("write requires_rebase");

    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = NonEmpty::new(message);
    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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

    let err = cmd_continue_in(&ctx, &messages).expect_err("expected no rebase error");
    assert_eq!(err.to_string(), "git command failed: no rebase in progress");
}

#[test]
fn cmd_finish_errors_when_rebase_is_required_but_not_active() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("commits"), format!("{}\n", "a".repeat(40))).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "true\n").expect("write requires_rebase");

    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = [message];
    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
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

    let err = cmd_finish_in(&ctx, &messages).expect_err("expected no rebase error");
    assert_eq!(err.to_string(), "git command failed: no rebase in progress");
}

#[test]
fn cmd_continue_converged_tree_completes_session_without_remainder() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    let original = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{original}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), "same_tree\n").expect("write expected_tree");

    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = NonEmpty::new(message);

    let commit_meta = "a\0b\0c\0d\0e\0f";
    let envs = [
        ("GIT_AUTHOR_NAME", "a"),
        ("GIT_AUTHOR_EMAIL", "b"),
        ("GIT_AUTHOR_DATE", "c"),
        ("GIT_COMMITTER_NAME", "d"),
        ("GIT_COMMITTER_EMAIL", "e"),
        ("GIT_COMMITTER_DATE", "f"),
    ];
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["checkout-index", "--all", "--force", "--quiet"],
            &[],
            false,
            repo,
            0,
        )
        .with_output(
            "git",
            &["diff", "--diff-filter=D", "--name-only", "--staged"],
            repo,
            "",
        )
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original.as_str(),
            ],
            repo,
            commit_meta,
        )
        .with_status(
            "git",
            &["commit", "--quiet", "--message", "test: message"],
            &envs,
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "same_tree\n");
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

    let code = cmd_continue_in(&ctx, &messages).expect("continue should succeed");
    assert_eq!(code, EXIT_OK);
    assert_eq!(
        io.stdout(),
        "FACTOR: Complete. Final commit split into 1 commits.\n"
    );
    assert!(io.stderr().is_empty(), "stderr should be empty");
    assert!(
        !state_dir.exists(),
        "factor state dir should be removed after completion"
    );
}

#[test]
fn advance_to_next_commit_finishes_root_session_and_runs_empty_root_cleanup() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    fs::write(repo.join("git-factor"), "").expect("write fake exe");

    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("split_count"), "2\n").expect("write split_count");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("is_root"), "true\n").expect("write is_root");

    let root = "a".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-list", "--max-parents=0", "HEAD"],
            repo,
            &format!("{root}\n"),
        )
        .with_output("git", &["ls-tree", &root], repo, "")
        .with_output("git", &["rev-parse", "--short", &root], repo, "aaaaaaa\n");
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

    let has_next = advance_to_next_commit_in(&ctx, &state_dir).expect("advance should succeed");
    assert!(!has_next, "session should be complete");
    assert_eq!(
        io.stdout(),
        "FACTOR: Complete. Final commit split into 2 commits.\n"
    );
    assert!(io.stderr().is_empty(), "stderr should be empty");
    assert!(
        !state_dir.exists(),
        "factor state dir should be removed after completion"
    );
}

#[test]
fn cmd_finish_rehydrates_remaining_changes_with_restore() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    let original = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{original}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), "expected_tree\n").expect("write expected_tree");

    let message = NonEmptyString::try_from("test: finish".to_owned()).expect("non-empty");
    let messages = [message];
    let commit_meta = "A U Thor\0author@example.com\01700000000 +0000\0C O M Mitter\0committer@example.com\01700000001 +0000";
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_output("git", &["write-tree"], repo, "expected_tree\n")
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original.as_str(),
            ],
            repo,
            commit_meta,
        )
        .with_status(
            "git",
            &["commit", "--quiet", "--message", "test: finish"],
            &[
                ("GIT_AUTHOR_NAME", "A U Thor"),
                ("GIT_AUTHOR_EMAIL", "author@example.com"),
                ("GIT_AUTHOR_DATE", "1700000000 +0000"),
                ("GIT_COMMITTER_NAME", "C O M Mitter"),
                ("GIT_COMMITTER_EMAIL", "committer@example.com"),
                ("GIT_COMMITTER_DATE", "1700000001 +0000"),
            ],
            false,
            repo,
            0,
        )
        .with_output(
            "git",
            &["rev-list", "--max-parents=0", "HEAD"],
            repo,
            &format!("{original}\n"),
        )
        .with_output("git", &["ls-tree", original.as_str()], repo, "not-empty\n");
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

    let code = cmd_finish_in(&ctx, &messages).expect("finish should succeed");
    assert_eq!(code, EXIT_OK);
}

#[test]
fn cmd_finish_reports_restore_failure() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    let original = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{original}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), "expected_tree\n").expect("write expected_tree");

    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = [message];
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            &[],
            false,
            repo,
            1,
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

    let err = cmd_finish_in(&ctx, &messages).expect_err("expected restore failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git restore failed (exit ")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_propagates_restore_nonzero_exit() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    let original = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{original}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), "expected_tree\n").expect("write expected_tree");

    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = [message];
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            &[],
            false,
            repo,
            1,
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

    let err = cmd_finish_in(&ctx, &messages).expect_err("expected restore failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git restore failed (exit ")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_errors_when_original_message_is_empty_without_messages() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    let original = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{original}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), "expected_tree\n").expect("write expected_tree");

    let messages: [NonEmptyString; 0] = [];
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", original.as_str()],
            repo,
            "\n",
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

    let err = cmd_finish_in(&ctx, &messages).expect_err("expected empty message error");
    assert_eq!(
        err.to_string(),
        "git command failed: original commit has empty message"
    );
}

#[test]
fn cmd_finish_reports_tree_mismatch_when_no_messages_are_provided() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    let original = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{original}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), "expected_tree\n").expect("write expected_tree");

    let messages: [NonEmptyString; 0] = [];
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", original.as_str()],
            repo,
            "original message\n",
        )
        .with_output("git", &["write-tree"], repo, "actual_tree\n");
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

    let err = cmd_finish_in(&ctx, &messages).expect_err("expected tree mismatch");
    assert!(
        matches!(
            &err,
            FactorError::TreeHashMismatch { actual, expected }
                if actual == "actual_tree" && expected == "expected_tree"
        ),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_single_head_session_exec_gate_failure_returns_exec_failed() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha],
            repo,
            &format!("{sha}\n"),
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &sha],
            repo,
            "subject\n",
        )
        .with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", "true"],
            &[],
            true,
            repo,
            0,
        )
        .with_status("bash", &["-c", "true"], &[], false, repo, 1);
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
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .expect_err("expected exec gate failure");
    assert!(
        matches!(
            &err,
            FactorError::ExecFailed { code, command } if *code != 0 && command == "true"
        ),
        "err was: {err:?}"
    );
}

#[test]
fn run_with_args_vec_propagates_io_errors_for_parser_and_help_output() {
    let dir = TempDir::new().expect("tempdir");
    let io = FailingIo;
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: dir.path().to_path_buf(),
        io: &io,
        env: &REAL_ENV,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![OsString::from("git-factor"), OsString::from("--not-real")],
    )
    .expect_err("expected parse stderr io failure");
    assert!(
        matches!(&err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
        "err was: {err:?}"
    );

    let err = run_with_args_vec(
        &ctx,
        vec![OsString::from("git-factor"), OsString::from("--help")],
    )
    .expect_err("expected parse stdout io failure");
    assert!(
        matches!(&err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
        "err was: {err:?}"
    );

    let err = run_with_args_vec(&ctx, vec![OsString::from("git-factor")])
        .expect_err("expected long-help io failure");
    assert!(
        matches!(&err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
        "err was: {err:?}"
    );
}

#[test]
fn increment_split_count_in_state_reports_invalid_split_count_value() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).expect("create state dir");
    fs::write(state_dir.join("split_count"), "not-a-number\n").expect("write split_count");

    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &REAL_RUNNER,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = increment_split_count_in_state(&ctx, &state_dir)
        .expect_err("invalid split_count should fail");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("split_count")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_status_propagates_second_current_index_read_failure() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");

    let sha = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");
    fs::write(state_dir.join("split_count"), "1\n").expect("write split_count");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("is_root"), "false\n").expect("write is_root");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let fs = NthReadFailureFs::new("current_index", 2, "second current_index read failed");
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &fs,
    };

    let err = cmd_status_in(&ctx).expect_err("expected second current_index read to fail");
    assert!(
        matches!(&err, FactorError::StateRead(inner) if inner.to_string().contains("second current_index read failed")),
        "err was: {err:?}"
    );
}

#[test]
fn advance_to_next_commit_reports_invalid_split_count_before_rebase_step() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("split_count"), "not-a-number\n").expect("write split_count");

    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &ScriptedRunner::default(),
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err =
        advance_to_next_commit_in(&ctx, &state_dir).expect_err("expected invalid split_count");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("split_count")),
        "err was: {err:?}"
    );
}

#[test]
fn advance_to_next_commit_reports_invalid_requires_rebase_before_rebase_step() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("requires_rebase"), "not-bool\n").expect("write requires_rebase");

    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &ScriptedRunner::default(),
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err =
        advance_to_next_commit_in(&ctx, &state_dir).expect_err("expected invalid requires_rebase");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("requires_rebase")),
        "err was: {err:?}"
    );
}

#[test]
fn advance_to_next_commit_reports_invalid_current_index_at_edit_stop() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("split_count"), "1\n").expect("write split_count");
    fs::write(state_dir.join("current_index"), "not-a-number\n").expect("write current_index");

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n");
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

    let err =
        advance_to_next_commit_in(&ctx, &state_dir).expect_err("expected invalid current_index");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("current_index")),
        "err was: {err:?}"
    );
}

#[test]
fn advance_to_next_commit_propagates_state_write_failure_for_current_index_update() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("split_count"), "1\n").expect("write split_count");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let fs = FailingWriteForFileFs {
        file_name: "current_index",
        message: "current_index write failed",
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &fs,
    };

    let err = advance_to_next_commit_in(&ctx, &state_dir)
        .expect_err("expected current_index write failure");
    assert!(
        matches!(&err, FactorError::StateWrite(inner) if inner.to_string().contains("current_index write failed")),
        "err was: {err:?}"
    );
}

#[test]
fn advance_to_next_commit_propagates_current_commit_lookup_failure_after_reset() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(git_dir.join("rebase-merge")).expect("create rebase-merge");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n{}\n", "a".repeat(40), "b".repeat(40)),
    )
    .expect("write commits");
    fs::write(state_dir.join("split_count"), "1\n").expect("write split_count");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current_index");

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n")
        .with_status("git", &["reset", "--quiet", "HEAD~1"], &[], false, repo, 0);
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let fs = NthReadFailureFs::new("current_index", 2, "current_index read failed after reset");
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &fs,
    };

    let err =
        advance_to_next_commit_in(&ctx, &state_dir).expect_err("expected current_commit failure");
    assert!(
        matches!(&err, FactorError::StateRead(inner) if inner.to_string().contains("after reset")),
        "err was: {err:?}"
    );
}

#[test]
fn advance_to_next_commit_propagates_empty_root_cleanup_failure() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    fs::write(state_dir.join("split_count"), "2\n").expect("write split_count");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("is_root"), "true\n").expect("write is_root");

    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &ScriptedRunner::default(),
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = advance_to_next_commit_in(&ctx, &state_dir)
        .expect_err("expected remove_empty_root failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("rev-list") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn rehydrate_pool_preserving_index_propagates_quit_status_io_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let base_runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, "deadbeef\n")
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(40),
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 0)
        .with_status("git", &["read-tree", "deadbeef"], &[], false, repo, 0);
    let runner = NthRunnerFailure::new(base_runner, 3);
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

    let commit = CommitSha::new("a".repeat(40)).expect("sha");
    let err = rehydrate_pool_preserving_index(&ctx, &commit).expect_err("expected quit io error");
    assert!(is_forced_runner_failure(&err), "err was: {err:?}");
}

#[test]
fn rehydrate_pool_preserving_index_propagates_read_tree_status_io_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let base_runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, "deadbeef\n")
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(40),
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 0)
        .with_status("git", &["read-tree", "deadbeef"], &[], false, repo, 0);
    let runner = NthRunnerFailure::new(base_runner, 4);
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

    let commit = CommitSha::new("a".repeat(40)).expect("sha");
    let err =
        rehydrate_pool_preserving_index(&ctx, &commit).expect_err("expected read-tree io error");
    assert!(is_forced_runner_failure(&err), "err was: {err:?}");
}

#[test]
fn cmd_continue_propagates_expected_tree_fallback_lookup_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    let original = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{original}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");

    let commit_meta = "a\0b\0c\0d\0e\0f";
    let envs = [
        ("GIT_AUTHOR_NAME", "a"),
        ("GIT_AUTHOR_EMAIL", "b"),
        ("GIT_AUTHOR_DATE", "c"),
        ("GIT_COMMITTER_NAME", "d"),
        ("GIT_COMMITTER_EMAIL", "e"),
        ("GIT_COMMITTER_DATE", "f"),
    ];
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["checkout-index", "--all", "--force", "--quiet"],
            &[],
            false,
            repo,
            0,
        )
        .with_output(
            "git",
            &["diff", "--diff-filter=D", "--name-only", "--staged"],
            repo,
            "",
        )
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original.as_str(),
            ],
            repo,
            commit_meta,
        )
        .with_status(
            "git",
            &["commit", "--quiet", "--message", "test: message"],
            &envs,
            false,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n");
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
    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = NonEmpty::new(message);

    let err = cmd_continue_in(&ctx, &messages).expect_err("expected expected-tree lookup failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("rev-parse") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_propagates_restore_status_error_when_runner_errors() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    let original = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{original}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), "expected_tree\n").expect("write expected_tree");

    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = [message];
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            &[],
            false,
            repo,
            1,
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

    let err = cmd_finish_in(&ctx, &messages).expect_err("expected restore status error");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git restore failed (exit ")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_without_messages_propagates_original_message_lookup_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    let original = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{original}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), "expected_tree\n").expect("write expected_tree");

    let messages: [NonEmptyString; 0] = [];
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            &[],
            false,
            repo,
            0,
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

    let err = cmd_finish_in(&ctx, &messages).expect_err("expected original message lookup failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git show:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_propagates_advance_error_after_successful_commit() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).expect("create factor dir");
    let original = "a".repeat(40);
    fs::write(state_dir.join("commits"), format!("{original}\n")).expect("write commits");
    fs::write(state_dir.join("current_index"), "0\n").expect("write current index");
    fs::write(state_dir.join("split_count"), "0\n").expect("write split_count");
    fs::write(state_dir.join("exec"), "true\n").expect("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").expect("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), "expected_tree\n").expect("write expected_tree");
    fs::write(state_dir.join("is_root"), "true\n").expect("write is_root");

    let commit_meta = "a\0b\0c\0d\0e\0f";
    let envs = [
        ("GIT_AUTHOR_NAME", "a"),
        ("GIT_AUTHOR_EMAIL", "b"),
        ("GIT_AUTHOR_DATE", "c"),
        ("GIT_COMMITTER_NAME", "d"),
        ("GIT_COMMITTER_EMAIL", "e"),
        ("GIT_COMMITTER_DATE", "f"),
    ];
    let message = NonEmptyString::try_from("test: message".to_owned()).expect("non-empty");
    let messages = [message];
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["checkout", "--quiet", "--", "."],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_output("git", &["write-tree"], repo, "expected_tree\n")
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original.as_str(),
            ],
            repo,
            commit_meta,
        )
        .with_status(
            "git",
            &["commit", "--quiet", "--message", "test: message"],
            &envs,
            false,
            repo,
            0,
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

    let err = cmd_finish_in(&ctx, &messages).expect_err("expected advance failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("rev-list") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_propagates_head_lookup_error_after_sorting() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", &sha],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha],
            repo,
            &format!("{sha}\n"),
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

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from(&sha),
        ],
    )
    .expect_err("expected HEAD lookup failure");
    assert!(
        matches!(&err, FactorError::InvalidCommit(commit) if commit == "HEAD"),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_single_head_session_propagates_exec_status_io_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha],
            repo,
            &format!("{sha}\n"),
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &sha],
            repo,
            "subject\n",
        )
        .with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", "true"],
            &[],
            true,
            repo,
            0,
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

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .expect_err("expected exec status lookup failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("unexpected status call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_propagates_commits_state_write_failure() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(40);
    let fs = FailingWriteForFileFs {
        file_name: "commits",
        message: "commits write failed",
    };
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha],
            repo,
            &format!("{sha}\n"),
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &sha],
            repo,
            "subject\n",
        )
        .with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", "true"],
            &[],
            true,
            repo,
            0,
        )
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
            &[],
            true,
            repo,
            0,
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
        fs: &fs,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .expect_err("expected commits state write failure");
    assert!(
        matches!(&err, FactorError::StateWrite(inner) if inner.to_string().contains("commits write failed")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_propagates_editor_path_error_in_multi_commit_session() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let sha_a = "a".repeat(40);
    let sha_b = "b".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", &sha_a],
            repo,
            &format!("{sha_a}\n"),
        )
        .with_output(
            "git",
            &["rev-parse", "--verify", &sha_b],
            repo,
            &format!("{sha_b}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha_a, &sha_b],
            repo,
            &format!("{sha_a}\n{sha_b}\n"),
        )
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha_b}\n"),
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha_a, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha_b, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha_a}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha_b}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_output("git", &["rev-parse", "--short", &sha_a], repo, "aaaaaaa\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &sha_a],
            repo,
            "subject\n",
        )
        .with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", "true"],
            &[],
            true,
            repo,
            0,
        );
    let io = TestIo::default();
    let env = ExeFailingEnv {
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
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from(&sha_a),
            OsString::from(&sha_b),
        ],
    )
    .expect_err("expected editor path error");
    assert_eq!(
        err.to_string(),
        "git command failed: cannot resolve current exe: no exe"
    );
}

#[test]
fn cmd_start_propagates_short_sha_lookup_error_for_sequence_editor() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    fs::write(repo.join("git-factor"), "").expect("write fake exe");
    let sha_a = "a".repeat(40);
    let sha_b = "b".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", &sha_a],
            repo,
            &format!("{sha_a}\n"),
        )
        .with_output(
            "git",
            &["rev-parse", "--verify", &sha_b],
            repo,
            &format!("{sha_b}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha_a, &sha_b],
            repo,
            &format!("{sha_a}\n{sha_b}\n"),
        )
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha_b}\n"),
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha_a, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha_b, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha_a}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha_b}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_output("git", &["rev-parse", "--short", &sha_a], repo, "aaaaaaa\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &sha_a],
            repo,
            "subject\n",
        )
        .with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", "true"],
            &[],
            true,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "--short", &sha_a], repo, "aaaaaaa\n");
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
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from(&sha_a),
            OsString::from(&sha_b),
        ],
    )
    .expect_err("expected short SHA lookup failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git rev-parse:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_propagates_expected_tree_capture_error_after_state_write() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha],
            repo,
            &format!("{sha}\n"),
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &sha],
            repo,
            "subject\n",
        )
        .with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", "true"],
            &[],
            true,
            repo,
            0,
        )
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
            &[],
            true,
            repo,
            0,
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

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .expect_err("expected expected-tree capture error");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git rev-parse:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_root_session_propagates_mixed_reset_error() {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(40);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha],
            repo,
            &format!("{sha}\n"),
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &sha],
            repo,
            "subject\n",
        )
        .with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", "true"],
            &[],
            true,
            repo,
            0,
        )
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
            &[],
            true,
            repo,
            1,
        )
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, "head_tree\n");
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
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .expect_err("expected mixed-reset failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git commit-tree:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}
