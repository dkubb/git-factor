use alloc::collections::VecDeque;
use core::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::panic::resume_unwind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;

use super::*;
use crate::git_factor::validation::validate_not_merge;
use tempfile::TempDir;

/// Generates an `Fs` trait method that delegates to `REAL_FS`.
macro_rules! fs_delegate {
    (canonicalize) => {
        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            REAL_FS.canonicalize(path)
        }
    };
    (create_dir_all) => {
        fn create_dir_all(&self, path: &Path) -> io::Result<()> {
            REAL_FS.create_dir_all(path)
        }
    };
    (exists) => {
        fn exists(&self, path: &Path) -> bool {
            REAL_FS.exists(path)
        }
    };
    (is_dir) => {
        fn is_dir(&self, path: &Path) -> bool {
            REAL_FS.is_dir(path)
        }
    };
    (read_to_string) => {
        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            REAL_FS.read_to_string(path)
        }
    };
    (remove_dir_all) => {
        fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
            REAL_FS.remove_dir_all(path)
        }
    };
    (remove_file) => {
        fn remove_file(&self, path: &Path) -> io::Result<()> {
            REAL_FS.remove_file(path)
        }
    };
    (write_string) => {
        fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
            REAL_FS.write_string(path, content)
        }
    };
}

const TEST_COMMIT_META: &str = "a\0b\0c\0d\0e\0f";
const TEST_COMMIT_ENVS: [(&str, &str); 6] = [
    ("GIT_AUTHOR_NAME", "a"),
    ("GIT_AUTHOR_EMAIL", "b"),
    ("GIT_AUTHOR_DATE", "c"),
    ("GIT_COMMITTER_NAME", "d"),
    ("GIT_COMMITTER_EMAIL", "e"),
    ("GIT_COMMITTER_DATE", "f"),
];
const SHA_LEN: usize = COMMIT_SHA_HEX_LEN;

/// Valid 40-char hex tree hash for the "expected" / "same" tree in tests.
const TREE_EXPECTED: &str = "dddddddddddddddddddddddddddddddddddddddd";

/// The same tree hash with trailing newline (matching git output format).
const TREE_EXPECTED_NL: &str = "dddddddddddddddddddddddddddddddddddddddd\n";

/// Valid 40-char hex tree hash for a "different" / "actual" / "restored" tree.
const TREE_DIFFERENT: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

/// The different tree hash with trailing newline.
const TREE_DIFFERENT_NL: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee\n";

/// Valid 40-char hex tree hash for rehydrate test scenarios.
const TREE_REHYDRATE: &str = "ffffffffffffffffffffffffffffffffffffffff";

/// The rehydrate tree hash with trailing newline.
const TREE_REHYDRATE_NL: &str = "ffffffffffffffffffffffffffffffffffffffff\n";

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt as _;

#[cfg(unix)]
struct NonUtf8Fs;

#[cfg(unix)]
impl Fs for NonUtf8Fs {
    fn canonicalize(&self, _path: &Path) -> io::Result<PathBuf> {
        let mut bytes = b"/tmp/".to_vec();
        bytes.push(0xff);
        bytes.extend_from_slice(b"/bin/git-factor");
        Ok(PathBuf::from(OsString::from_vec(bytes)))
    }

    fs_delegate!(create_dir_all);
    fs_delegate!(exists);
    fs_delegate!(is_dir);
    fs_delegate!(read_to_string);
    fs_delegate!(remove_dir_all);
    fs_delegate!(remove_file);
    fs_delegate!(write_string);
}

struct FailingRequiresRebaseWriteFs;

impl Fs for FailingRequiresRebaseWriteFs {
    fs_delegate!(canonicalize);
    fs_delegate!(create_dir_all);
    fs_delegate!(exists);
    fs_delegate!(is_dir);
    fs_delegate!(read_to_string);
    fs_delegate!(remove_dir_all);
    fs_delegate!(remove_file);

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        if path
            .file_name()
            .is_some_and(|name| name == "requires_rebase")
        {
            return Err(io::Error::other("requires_rebase write failed"));
        }
        REAL_FS.write_string(path, content)
    }
}

struct FailingIsRootWriteFs;

impl Fs for FailingIsRootWriteFs {
    fs_delegate!(canonicalize);
    fs_delegate!(create_dir_all);
    fs_delegate!(exists);
    fs_delegate!(is_dir);
    fs_delegate!(read_to_string);
    fs_delegate!(remove_dir_all);
    fs_delegate!(remove_file);

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        if path.file_name().is_some_and(|name| name == "is_root") {
            return Err(io::Error::other("is_root write failed"));
        }
        REAL_FS.write_string(path, content)
    }
}

struct FailingWriteForFileFs {
    file_name: &'static str,
    message: &'static str,
}

impl Fs for FailingWriteForFileFs {
    fs_delegate!(canonicalize);
    fs_delegate!(create_dir_all);
    fs_delegate!(exists);
    fs_delegate!(is_dir);
    fs_delegate!(read_to_string);
    fs_delegate!(remove_dir_all);
    fs_delegate!(remove_file);

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        if path.file_name().is_some_and(|name| name == self.file_name) {
            return Err(io::Error::other(self.message));
        }
        REAL_FS.write_string(path, content)
    }
}

struct CorruptSplitCountWriteFs;

impl Fs for CorruptSplitCountWriteFs {
    fs_delegate!(canonicalize);
    fs_delegate!(create_dir_all);
    fs_delegate!(exists);
    fs_delegate!(is_dir);
    fs_delegate!(read_to_string);
    fs_delegate!(remove_dir_all);
    fs_delegate!(remove_file);

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        if path.file_name().is_some_and(|name| name == "split_count") {
            return REAL_FS.write_string(path, "not-a-number\n");
        }
        REAL_FS.write_string(path, content)
    }
}

struct NthReadFailureFs {
    fail_at: usize,
    file_name: &'static str,
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
    fs_delegate!(canonicalize);
    fs_delegate!(create_dir_all);
    fs_delegate!(exists);
    fs_delegate!(is_dir);

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        if path.file_name().is_some_and(|name| name == self.file_name) {
            let mut reads = self
                .reads
                .lock()
                .map_err(|error| io::Error::other(format!("nth-read lock: {error}")))?;
            *reads = reads.checked_add(1).or_abort("counter should not overflow");
            if *reads == self.fail_at {
                return Err(io::Error::other(self.message));
            }
        }
        REAL_FS.read_to_string(path)
    }

    fs_delegate!(remove_dir_all);
    fs_delegate!(remove_file);
    fs_delegate!(write_string);
}

#[derive(Clone, Default)]
struct ScriptedRunner {
    outputs: RefCell<HashMap<String, VecDeque<Output>>>,
    statuses: RefCell<HashMap<String, VecDeque<ExitStatus>>>,
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
            .map(|&(key, value)| format!("{key}={value}"))
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
        let key = Self::output_key(bin, args, cwd);
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

    fn with_status(
        self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, &str)],
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
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        let key = Self::output_key(bin, args, cwd);
        if let Some(output) = self
            .outputs
            .borrow_mut()
            .get_mut(&key)
            .and_then(VecDeque::pop_front)
        {
            return Ok(output);
        }
        if bin == "git" && args == ["rev-parse", "--git-dir"] {
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
        envs: &[(&str, &str)],
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

struct NthRunnerFailure {
    calls: Mutex<usize>,
    fail_at: usize,
    inner: ScriptedRunner,
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
        let mut calls = self.calls.lock().or_abort("runner calls lock");
        *calls = calls.checked_add(1).or_abort("counter should not overflow");
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

struct RebaseContinueCompletesRunner {
    completed: Mutex<bool>,
    inner: ScriptedRunner,
    rebase_dir: PathBuf,
}

impl RebaseContinueCompletesRunner {
    fn new(inner: ScriptedRunner, rebase_dir: PathBuf) -> Self {
        Self {
            inner,
            rebase_dir,
            completed: Mutex::new(false),
        }
    }
}

impl Runner for RebaseContinueCompletesRunner {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
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
        if bin == "git" && args == ["rebase", "--continue"] {
            let mut completed = self.completed.lock().map_err(|error| {
                io::Error::other(format!(
                    "rebase completion lock should not be poisoned: {error}"
                ))
            })?;
            if !*completed {
                drop(fs::remove_dir_all(&self.rebase_dir));
                *completed = true;
            }
        }
        self.inner.status(bin, args, envs, quiet, cwd)
    }
}

struct RebaseStartPausesRunner {
    inner: ScriptedRunner,
    rebase_dir: PathBuf,
}

impl RebaseStartPausesRunner {
    fn new(inner: ScriptedRunner, rebase_dir: PathBuf) -> Self {
        Self { inner, rebase_dir }
    }
}

impl Runner for RebaseStartPausesRunner {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
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
        let status = self.inner.status(bin, args, envs, quiet, cwd)?;
        if bin == "git"
            && args.first() == Some(&"rebase")
            && args.contains(&"--interactive")
            && status.success()
        {
            fs::create_dir_all(&self.rebase_dir).or_abort("create rebase-merge");
        }
        Ok(status)
    }
}

struct BashStatusFailureRunner {
    inner: ScriptedRunner,
}

impl Runner for BashStatusFailureRunner {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
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
        if bin == "bash" && args == ["-c", "true"] {
            return Err(io::Error::other("forced bash status failure"));
        }
        self.inner.status(bin, args, envs, quiet, cwd)
    }
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

#[derive(Default)]
struct TestIo {
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

struct MatchingOutlnFailureIo<'line> {
    fail_on: &'line str,
}

impl Io for MatchingOutlnFailureIo<'_> {
    fn err(&self, _text: &str) -> io::Result<()> {
        Ok(())
    }

    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(line)?;
        self.err("\n")
    }

    fn out(&self, text: &str) -> io::Result<()> {
        if text == self.fail_on {
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

struct FailingRemoveDirAllFs;

impl Fs for FailingRemoveDirAllFs {
    fs_delegate!(canonicalize);
    fs_delegate!(create_dir_all);
    fs_delegate!(exists);
    fs_delegate!(is_dir);
    fs_delegate!(read_to_string);

    fn remove_dir_all(&self, _path: &Path) -> io::Result<()> {
        Err(io::Error::other("injected remove_dir_all failure"))
    }

    fs_delegate!(remove_file);
    fs_delegate!(write_string);
}

struct StickyStatePathFs;

impl Fs for StickyStatePathFs {
    fs_delegate!(canonicalize);
    fs_delegate!(create_dir_all);

    fn exists(&self, _path: &Path) -> bool {
        true
    }

    fn is_dir(&self, _path: &Path) -> bool {
        true
    }

    fs_delegate!(read_to_string);

    fn remove_dir_all(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }

    fs_delegate!(remove_file);
    fs_delegate!(write_string);
}

fn with_git_dir_outputs(mut runner: ScriptedRunner, repo: &Path, count: usize) -> ScriptedRunner {
    for _ in 0..count {
        runner = runner.with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    }
    runner
}

fn with_start_gate_result(
    runner: ScriptedRunner,
    repo: &Path,
    command: &str,
    code: i32,
    stdout: &str,
    stderr: &str,
) -> ScriptedRunner {
    let runner_with_gate = runner
        .with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", command],
            &[],
            true,
            repo,
            0,
        )
        .with_output_status("bash", &["-c", command], repo, code, stdout, stderr);
    if code == i32::default() {
        return runner_with_gate.with_output("git", &["status", "--porcelain=v1"], repo, "");
    }
    runner_with_gate
}

fn start_single_head_resolution_runner(repo: &Path, sha: &str) -> ScriptedRunner {
    ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output("git", &["status", "--porcelain=v1"], repo, "")
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", sha],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
}

fn start_single_head_validation_runner(repo: &Path, sha: &str) -> ScriptedRunner {
    start_single_head_resolution_runner(repo, sha)
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", sha, "HEAD"],
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
}

fn is_forced_runner_failure(err: &FactorError) -> bool {
    matches!(
        err,
        FactorError::GitDir(msg) | FactorError::GitCommand(msg)
            if msg.contains("forced runner failure")
    )
}

fn test_messages() -> NonEmpty<NonEmptyString> {
    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
    NonEmpty::new(message)
}

fn setup_factor_state(
    repo: &Path,
    original: &str,
    split_count: &str,
    requires_rebase: Option<&str>,
    expected_tree: Option<&str>,
) -> PathBuf {
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("split_count"), split_count).or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    if let Some(value) = requires_rebase {
        fs::write(state_dir.join("requires_rebase"), value).or_abort("write requires_rebase");
    }
    if let Some(value) = expected_tree {
        fs::write(state_dir.join("expected_tree"), value).or_abort("write expected_tree");
    }
    state_dir
}

fn continue_runner_with_commit(repo: &Path, original: &str, deleted_paths: &str) -> ScriptedRunner {
    with_git_dir_outputs(ScriptedRunner::default(), repo, 6)
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", original, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{original}^2")],
            &[],
            true,
            repo,
            1,
        )
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
            deleted_paths,
        )
        .with_output("git", &["status", "--porcelain=v1"], repo, "M  file.txt\n")
        .with_status("bash", &["-c", "true"], &[], false, repo, 0)
        .with_output("git", &["status", "--porcelain=v1"], repo, "M  file.txt\n")
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original,
            ],
            repo,
            TEST_COMMIT_META,
        )
        .with_status(
            "git",
            &["commit", "--quiet", "--message", "test: message"],
            &TEST_COMMIT_ENVS,
            false,
            repo,
            0,
        )
        .with_output("git", &["status", "--porcelain=v1"], repo, "")
}

fn continue_runner_with_remaining_output(
    repo: &Path,
    original: &str,
    diff_stat: &str,
    untracked: &str,
) -> ScriptedRunner {
    let repo_top = repo.to_string_lossy().into_owned();
    continue_runner_with_commit(repo, original, "")
        .with_output(
            "git",
            &["rev-parse", "HEAD^{tree}"],
            repo,
            &format!("{}\n", "c".repeat(SHA_LEN)),
        )
        .with_status(
            "git",
            &[
                "restore",
                "--source",
                original,
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
        .with_output("git", &["write-tree"], repo, TREE_EXPECTED_NL)
        .with_status("git", &["reset", "--quiet"], &[], false, repo, 0)
        .with_output("git", &["diff", "--stat"], repo, diff_stat)
        .with_output(
            "git",
            &["ls-files", "--others", "--exclude-standard"],
            repo,
            untracked,
        )
        .with_output("git", &["rev-parse", "--show-toplevel"], repo, &repo_top)
        .with_output("git", &["write-tree"], repo, TREE_REHYDRATE_NL)
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                original,
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 0)
        .with_status("git", &["read-tree", TREE_EXPECTED], &[], false, repo, 0)
        .with_status("git", &["read-tree", TREE_REHYDRATE], &[], false, repo, 0)
}

fn finish_advance_runner(repo: &Path, original: &str, next: &str) -> ScriptedRunner {
    with_git_dir_outputs(ScriptedRunner::default(), repo, 6)
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
                original,
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
        .with_output("git", &["write-tree"], repo, TREE_EXPECTED_NL)
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original,
            ],
            repo,
            TEST_COMMIT_META,
        )
        .with_status(
            "git",
            &["commit", "--quiet", "--message", "test: message"],
            &TEST_COMMIT_ENVS,
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        )
        .with_output("git", &["status", "--porcelain=v1"], repo, "")
        .with_status("git", &["reset", "--quiet", "HEAD~1"], &[], false, repo, 0)
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", next],
            repo,
            "next subject\n",
        )
        .with_output("git", &["rev-parse", "--short", next], repo, "bbbbbbb\n")
        .with_output(
            "git",
            &["diff", "--stat"],
            repo,
            "file.txt | 1 +\n1 file changed, 1 insertion(+)\n",
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
            &["rev-parse", "--show-toplevel"],
            repo,
            &format!("{}\n", repo.display()),
        )
}

fn start_multi_commit_runner_base(repo: &Path, sha_a: &str, sha_b: &str) -> ScriptedRunner {
    ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output("git", &["status", "--porcelain=v1"], repo, "")
        .with_output(
            "git",
            &["rev-parse", "--verify", sha_a],
            repo,
            &format!("{sha_a}\n"),
        )
        .with_output(
            "git",
            &["rev-parse", "--verify", sha_b],
            repo,
            &format!("{sha_b}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", sha_a, sha_b],
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
            &["merge-base", "--is-ancestor", sha_a, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", sha_b, "HEAD"],
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
        .with_output("git", &["rev-parse", "--short", sha_a], repo, "aaaaaaa\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", sha_a],
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
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha_a}^")],
            &[],
            true,
            repo,
            0,
        )
}

fn start_rebase_failure_runner(
    repo: &Path,
    sha_a: &str,
    sha_b: &str,
    seq_editor: &str,
) -> ScriptedRunner {
    start_multi_commit_runner_base(repo, sha_a, sha_b)
        .with_output("git", &["rev-parse", "--short", sha_a], repo, "aaaaaaa\n")
        .with_output("git", &["rev-parse", "--short", sha_b], repo, "bbbbbbb\n")
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
                &format!("{sha_a}^"),
            ],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", seq_editor)],
            false,
            repo,
            1,
        )
}

fn start_single_head_runner(repo: &Path, sha: &str, tree: &str, diff_stat: &str) -> ScriptedRunner {
    with_start_gate_result(
        start_single_head_validation_runner(repo, sha)
            .with_output("git", &["rev-parse", "--short", sha], repo, "aaaaaaa\n")
            .with_output(
                "git",
                &["show", "--format=%B", "--no-patch", sha],
                repo,
                "subject\n",
            ),
        repo,
        "true",
        0,
        "",
        "",
    )
    .with_status(
        "git",
        &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
        &[],
        true,
        repo,
        0,
    )
    .with_output(
        "git",
        &["rev-parse", concat!("HEAD^", "{", "tree", "}")],
        repo,
        &format!("{tree}\n"),
    )
    .with_status("git", &["reset", "--quiet", "HEAD~1"], &[], false, repo, 0)
    .with_output("git", &["diff", "--stat"], repo, diff_stat)
    .with_output(
        "git",
        &["ls-files", "--others", "--exclude-standard"],
        repo,
        "",
    )
    .with_output("git", &["diff", "--stat"], repo, diff_stat)
    .with_output(
        "git",
        &["rev-parse", "--show-toplevel"],
        repo,
        &format!("{}\n", repo.display()),
    )
}

fn start_single_head_root_runner(
    repo: &Path,
    sha: &str,
    synthetic_root: &str,
    tree: &str,
    diff_stat: &str,
    reset_code: i32,
) -> ScriptedRunner {
    with_start_gate_result(
        start_single_head_validation_runner(repo, sha)
            .with_output("git", &["rev-parse", "--short", sha], repo, "aaaaaaa\n")
            .with_output(
                "git",
                &["show", "--format=%B", "--no-patch", sha],
                repo,
                "subject\n",
            ),
        repo,
        "true",
        0,
        "",
        "",
    )
    .with_status(
        "git",
        &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
        &[],
        true,
        repo,
        1,
    )
    .with_output(
        "git",
        &["rev-parse", concat!("HEAD^", "{", "tree", "}")],
        repo,
        &format!("{tree}\n"),
    )
    .with_output(
        "git",
        &[
            "commit-tree",
            "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
            "-m",
            "empty",
        ],
        repo,
        &format!("{synthetic_root}\n"),
    )
    .with_status(
        "git",
        &["reset", "--quiet", synthetic_root],
        &[],
        false,
        repo,
        reset_code,
    )
    .with_output("git", &["diff", "--stat"], repo, diff_stat)
    .with_output(
        "git",
        &["ls-files", "--others", "--exclude-standard"],
        repo,
        "",
    )
    .with_output("git", &["diff", "--stat"], repo, diff_stat)
    .with_output(
        "git",
        &["rev-parse", "--show-toplevel"],
        repo,
        &format!("{}\n", repo.display()),
    )
}

fn run_start_two_shas(ctx: &Ctx<'_>, sha_a: &str, sha_b: &str) -> Result<i32, FactorError> {
    run_with_args_vec(
        ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from(sha_a),
            OsString::from(sha_b),
        ],
    )
}

fn build_sequence_editor(repo: &Path) -> String {
    let canon_repo = fs::canonicalize(repo).or_abort("canonicalize repo");
    let editor = canon_repo.join("git-sequence-editor");
    let factor = repo.join("git-factor");
    let editor_str = editor.to_str().or_abort("editor path is UTF-8");
    let factor_str = factor.to_str().or_abort("factor path is UTF-8");
    let preflight_zero = format!(
        "{} {} {}",
        shell_quote(factor_str),
        shell_quote("rebase-exec-preflight"),
        shell_quote("0")
    );
    let begin_zero = format!(
        "{} {} {}",
        shell_quote(factor_str),
        shell_quote("rebase-exec-begin"),
        shell_quote("0")
    );
    let preflight_one = format!(
        "{} {} {}",
        shell_quote(factor_str),
        shell_quote("rebase-exec-preflight"),
        shell_quote("1")
    );
    let begin_one = format!(
        "{} {} {}",
        shell_quote(factor_str),
        shell_quote("rebase-exec-begin"),
        shell_quote("1")
    );
    [
        shell_quote(editor_str),
        shell_quote("--factor-target"),
        shell_quote("aaaaaaa"),
        shell_quote("--factor-preflight"),
        shell_quote(preflight_zero.as_str()),
        shell_quote("--factor-begin"),
        shell_quote(begin_zero.as_str()),
        shell_quote("--factor-target"),
        shell_quote("bbbbbbb"),
        shell_quote("--factor-preflight"),
        shell_quote(preflight_one.as_str()),
        shell_quote("--factor-begin"),
        shell_quote(begin_one.as_str()),
    ]
    .join(" ")
}

#[expect(
    clippy::too_many_lines,
    reason = "range start fixture scripts the entire preflight, sequence-editor, and rebase setup flow"
)]
fn start_range_ref_runner(repo: &Path, sha_a: &str, sha_b: &str) -> ScriptedRunner {
    let seq_editor = build_sequence_editor(repo);
    with_git_dir_outputs(ScriptedRunner::default(), repo, 2)
        .with_output("git", &["status", "--porcelain=v1"], repo, "")
        .with_output(
            "git",
            &["rev-list", "a..b"],
            repo,
            &format!("{sha_a}\n{sha_b}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", sha_a, sha_b],
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
            &["merge-base", "--is-ancestor", sha_a, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", sha_b, "HEAD"],
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
        .with_output("git", &["rev-parse", "--short", sha_a], repo, "aaaaaaa\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", sha_a],
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
        .with_output("git", &["rev-parse", "--short", sha_a], repo, "aaaaaaa\n")
        .with_output("git", &["rev-parse", "--short", sha_b], repo, "bbbbbbb\n")
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
                &format!("{sha_a}^"),
            ],
            &[
                ("GIT_EDITOR", "false"),
                ("GIT_SEQUENCE_EDITOR", seq_editor.as_str()),
            ],
            false,
            repo,
            0,
        )
        .with_output("git", &["status", "--porcelain=v1"], repo, "")
        .with_status("git", &["reset", "--quiet", "HEAD~1"], &[], false, repo, 0)
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", sha_a],
            repo,
            "msg\n",
        )
        .with_output("git", &["rev-parse", "--short", sha_a], repo, "aaaaaaa\n")
        .with_output("git", &["diff", "--stat"], repo, "")
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
        )
}

#[cfg(unix)]
fn exit_status(code: i32) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt as _;
    ExitStatus::from_raw(code)
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

fn ctx_from_parts<'ctx>(
    env: &'ctx dyn Env,
    runner: &'ctx dyn Runner,
    io: &'ctx dyn Io,
    fs: &'ctx dyn Fs,
) -> Result<Ctx<'ctx>, FactorError> {
    let cwd = match env.current_dir() {
        Ok(cwd) => cwd,
        Err(err) => {
            return Err(FactorError::GitCommand(non_empty_msg(format!(
                "cannot resolve cwd: {err}"
            ))));
        }
    };
    Ok(Ctx {
        cwd,
        env,
        fs,
        io,
        runner,
    })
}

#[test]
fn read_state_reports_corrupted_empty_state_file() {
    let dir = TempDir::new().or_abort("tempdir");
    let state_dir = dir.path().join("factor-state");
    fs::create_dir_all(&state_dir).or_abort("create state dir");
    fs::write(state_dir.join("short_sha"), "\n").or_abort("write state file");
    let ctx = ctx_for(dir.path());

    let err =
        read_state(&ctx, &state_dir, "short_sha").err_or_abort("empty state file should error");

    assert!(matches!(
        err,
        FactorError::GitCommand(message)
            if message.as_str() == "corrupted state file 'short_sha': file is empty"
    ));
}

#[test]
fn scripted_runner_status_includes_env_key() {
    let dir = TempDir::new().or_abort("tempdir");
    let runner = ScriptedRunner::default();

    let err = runner
        .status(
            "git",
            &["status"],
            &[("GIT_OPTIONAL_LOCKS", "0")],
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
        .output("git", &["version"], dir.path())
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
fn git_commit_preserving_metadata_returns_error_on_truncated_format() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
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

    let commit = CommitSha::new(sha).or_abort("commit sha");
    let msg = NonEmptyString::try_from("feat: msg".to_owned()).or_abort("msg");
    let messages = NonEmpty::new(msg);

    let err = git_commit_preserving_metadata(&ctx, &commit, &messages, false)
        .err_or_abort("truncated format must return error");

    assert!(
        err.to_string().contains("truncated commit metadata"),
        "unexpected error: {err}"
    );
}

#[test]
fn git_commit_preserving_metadata_propagates_git_output_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 6);
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

    let commit = CommitSha::new("a".repeat(SHA_LEN)).or_abort("commit sha");
    let msg = NonEmptyString::try_from("feat: msg".to_owned()).or_abort("msg");
    let messages = NonEmpty::new(msg);

    let err = git_commit_preserving_metadata(&ctx, &commit, &messages, false)
        .err_or_abort("expected git output error");

    assert!(
        err.to_string().contains("unexpected output call"),
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
    let runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 3);
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
    let runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 8);
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
    let runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 8);
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
fn cmd_continue_returns_original_exec_error_when_rehydrate_succeeds() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let original = "a".repeat(SHA_LEN);
    setup_factor_state(
        repo,
        &original,
        "0\n",
        Some("false\n"),
        Some(&format!("{}\n", "b".repeat(SHA_LEN))),
    );

    let idx_tree = format!("{}\n", "c".repeat(SHA_LEN));
    let base = continue_runner_with_commit(repo, &original, "")
        .with_output("git", &["write-tree"], repo, &idx_tree)
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
        .with_status("git", &["read-tree", idx_tree.trim()], &[], false, repo, 0);
    let runner = BashStatusFailureRunner { inner: base };
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
    let messages = test_messages();

    let err = cmd_continue_in(&ctx, &messages).err_or_abort("expected exec status failure");
    assert!(
        err.to_string().contains("forced bash status failure"),
        "unexpected error: {err}"
    );
}

#[test]
fn cmd_continue_returns_rehydrate_error_when_exec_status_call_fails() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let original = "a".repeat(SHA_LEN);
    setup_factor_state(
        repo,
        &original,
        "0\n",
        Some("false\n"),
        Some(TREE_EXPECTED_NL),
    );

    let base = continue_runner_with_commit(repo, &original, "");
    let runner = BashStatusFailureRunner { inner: base };
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
    let messages = test_messages();

    let err = cmd_continue_in(&ctx, &messages).err_or_abort("expected rehydrate failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git write-tree") && msg.contains("unexpected output call")),
        "unexpected error: {err}"
    );
}

#[test]
fn validate_not_merge_treats_status_error_as_non_merge() {
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

    validate_not_merge(&ctx, &sha).or_abort("status errors should be treated as non-merge");
}

#[test]
fn validate_not_merge_treats_nonzero_status_as_non_merge() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = CommitSha::new("a".repeat(SHA_LEN)).or_abort("valid sha");
    let runner = ScriptedRunner::default().with_status(
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

    validate_not_merge(&ctx, &sha).or_abort("nonzero status should be treated as non-merge");
}

#[test]
