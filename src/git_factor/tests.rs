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
