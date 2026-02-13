use std::collections::HashMap;
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

fn ctx_for(path: &Path) -> Ctx<'static> {
    Ctx {
        runner: &REAL_RUNNER,
        cwd: path.to_path_buf(),
        io: &REAL_IO,
        env: &REAL_ENV,
        fs: &REAL_FS,
    }
}

#[cfg(unix)]
fn exit_status(code: i32) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt as _;
    ExitStatus::from_raw(code)
}

#[derive(Default)]
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
            &["rev-parse", "--verify", "--quiet", &format!("{sha}^2")],
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
            &["rev-parse", "--verify", "--quiet", &format!("{sha}^2")],
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
            &["rev-parse", "--verify", "--quiet", &format!("{sha}^2")],
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
            &["rev-parse", "--verify", "--quiet", &format!("{sha}^")],
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
            &["rev-parse", "--verify", "--quiet", &format!("{sha_a}^2")],
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
            &["rev-parse", "--verify", "--quiet", &format!("{sha_b}^2")],
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
            &["rev-parse", "--verify", "--quiet", &format!("{sha_a}^")],
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
            &["rev-parse", "--verify", "--quiet", &format!("{sha}^2")],
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
            &["rev-parse", "--verify", "--quiet", &format!("{sha}^")],
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
            &["rev-parse", "--verify", "--quiet", &format!("{sha_a}^2")],
            &[],
            true,
            repo,
            1,
        )
        .with_status(
            "git",
            &["rev-parse", "--verify", "--quiet", &format!("{sha_b}^2")],
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
            &["rev-parse", "--verify", "--quiet", &format!("{sha_a}^")],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &[
                "rebase",
                "--no-autosquash",
                "--no-autostash",
                "--no-rebase-merges",
                "--no-stat",
                "--reschedule-failed-exec",
                "--interactive",
                "--exec",
                "true",
                &format!("{sha_a}^"),
            ],
            &[("GIT_SEQUENCE_EDITOR", &seq_editor)],
            false,
            repo,
            0,
        )
        .with_status("git", &["reset", "--mixed", "HEAD~1"], &[], false, repo, 0)
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
        .with_status("git", &["rebase", "--continue"], &[], false, repo, 0)
        .with_status("git", &["reset", "--mixed", "HEAD~1"], &[], false, repo, 0)
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
        .with_status("git", &["rebase", "--continue"], &[], false, repo, 0)
        .with_status("git", &["reset", "--mixed", "HEAD~1"], &[], false, repo, 0)
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
        .with_status("git", &["rebase", "--continue"], &[], false, repo, 0)
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
        .with_status("git", &["rebase", "--continue"], &[], false, repo, 0)
        .with_status("git", &["reset", "--mixed", "HEAD~1"], &[], false, repo, 0)
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
        .with_status("git", &["diff", "--cached", "--quiet"], &[], false, repo, 1)
        .with_status("git", &["checkout", "--", "."], &[], false, repo, 0)
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
            &["checkout-index", "--all", "--force"],
            &[],
            false,
            repo,
            0,
        )
        .with_output(
            "git",
            &["diff", "--cached", "--name-only", "--diff-filter=D"],
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
            &["commit", "--message", "test: message"],
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
        .with_status("git", &["diff", "--cached", "--quiet"], &[], false, repo, 1)
        .with_status("git", &["checkout", "--", "."], &[], false, repo, 0)
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
            &["checkout-index", "--all", "--force"],
            &[],
            false,
            repo,
            0,
        )
        .with_output(
            "git",
            &["diff", "--cached", "--name-only", "--diff-filter=D"],
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
            &["commit", "--message", "test: message"],
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
        .with_status("git", &["checkout", "--", "."], &[], false, repo, 0)
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
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                original.as_str(),
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 0)
        .with_output("git", &["write-tree"], repo, expected_tree_output)
        .with_status("git", &["diff", "--cached", "--quiet"], &[], false, repo, 1)
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
            &["commit", "--message", "test: message"],
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
fn cmd_finish_propagates_final_cherry_pick_status_error() {
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
        .with_status("git", &["checkout", "--", "."], &[], false, repo, 0)
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

    let err = cmd_finish_in(&ctx, &messages).expect_err("expected cherry-pick status error");
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
        .with_status("git", &["checkout", "--", "."], &[], false, repo, 0)
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
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                original.as_str(),
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 0)
        .with_output("git", &["write-tree"], repo, expected_tree_output)
        .with_status("git", &["diff", "--cached", "--quiet"], &[], false, repo, 1)
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
