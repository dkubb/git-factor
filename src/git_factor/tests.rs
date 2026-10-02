#[path = "tests/abort_contracts.rs"]
pub(in crate::git_factor) mod abort_contracts;
#[path = "tests/main_entry.rs"]
mod main_entry;
#[path = "tests/start_contracts.rs"]
pub(in crate::git_factor) mod start_contracts;
#[path = "tests/status_contracts.rs"]
pub(in crate::git_factor) mod status_contracts;

use alloc::collections::VecDeque;
use core::cell::RefCell;
use core::error::Error;
use core::fmt;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::panic::resume_unwind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;

use super::*;
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
const HEAD_TREEISH: &str = "HEAD^{tree}";

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
    #[expect(
        clippy::single_call_fn,
        reason = "test constructor keeps read-failure setup concise at the one current callsite"
    )]
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
pub(in crate::git_factor) struct ScriptedRunner {
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

    /// Arranges a commit-object response with zero or one real parent.
    fn with_commit_object(self, repo: &Path, sha: &str, parent: Option<&str>) -> Self {
        let parent_header =
            parent.map_or_else(String::new, |identity| format!("parent {identity}\n"));
        let object = format!(
            "tree {}\n{parent_header}author Example <example@example.com> 1 +0000\n\nsubject\n",
            "c".repeat(SHA_LEN)
        );
        self.with_output("git", &["cat-file", "commit", sha], repo, &object)
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

    pub(in crate::git_factor) fn with_status(
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
    start_resolved_head_validation_runner(repo, sha).with_commit_object(
        repo,
        sha,
        Some(&"d".repeat(SHA_LEN)),
    )
}

fn start_resolved_head_validation_runner(repo: &Path, sha: &str) -> ScriptedRunner {
    start_single_head_resolution_runner(repo, sha)
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", sha, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_commit_object(repo, sha, Some(&"d".repeat(SHA_LEN)))
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

fn start_multi_commit_runner_base(repo: &Path, sha_a: &str, sha_b: &str) -> ScriptedRunner {
    let head = "c".repeat(SHA_LEN);
    let span_expr = format!("{sha_a}..{sha_b}");

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
            &[
                "rev-list",
                "--reverse",
                "--ancestry-path",
                span_expr.as_str(),
            ],
            repo,
            &format!("{sha_b}\n"),
        )
        .with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{head}\n"),
        )
        .with_commit_object(repo, sha_a, Some(&"d".repeat(SHA_LEN)))
        .with_commit_object(repo, sha_b, Some(sha_a))
        .with_output(
            "git",
            &["rev-parse", "--verify", &format!("{sha_b}^")],
            repo,
            &format!("{sha_a}\n"),
        )
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", sha_a, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_output("git", &["rev-parse", "--short", sha_a], repo, "aaaaaaa\n")
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
            &["merge-base", "--is-ancestor", sha_b, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_commit_object(repo, sha_a, Some(&"d".repeat(SHA_LEN)))
        .with_output("git", &["rev-parse", "--short", sha_b], repo, "bbbbbbb\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", sha_b],
            repo,
            "subject\n",
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
                "--no-update-refs",
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
    start_resolved_head_runner(repo, sha, tree, diff_stat).with_commit_object(
        repo,
        sha,
        Some(&"d".repeat(SHA_LEN)),
    )
}

fn start_resolved_head_runner(
    repo: &Path,
    sha: &str,
    tree: &str,
    diff_stat: &str,
) -> ScriptedRunner {
    with_start_gate_result(
        start_resolved_head_validation_runner(repo, sha)
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
    .with_output(
        "git",
        &["rev-parse", concat!("HEAD^", "{", "tree", "}")],
        repo,
        &format!("{tree}\n"),
    )
    .with_status(
        "git",
        &["reset", "--quiet", &format!("{sha}^")],
        &[],
        false,
        repo,
        0,
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

fn start_single_head_root_runner(
    repo: &Path,
    sha: &str,
    synthetic_root: &str,
    tree: &str,
    diff_stat: &str,
    reset_code: i32,
) -> ScriptedRunner {
    with_start_gate_result(
        start_single_head_resolution_runner(repo, sha)
            .with_status(
                "git",
                &["merge-base", "--is-ancestor", sha, "HEAD"],
                &[],
                true,
                repo,
                0,
            )
            .with_commit_object(repo, sha, None)
            .with_commit_object(repo, sha, None)
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

fn build_sequence_editor(repo: &Path, sha_a: &str, sha_b: &str, start_head: &str) -> String {
    let canon_repo = fs::canonicalize(repo).or_abort("canonicalize repo");
    let editor = canon_repo.join("git-sequence-editor");
    let factor = repo.join("git-factor");
    let editor_str = editor.to_str().or_abort("editor path is UTF-8");
    let factor_str = factor.to_str().or_abort("factor path is UTF-8");
    let commits = format!("{sha_a},{sha_b}");
    let preflight = format!(
        "{} {} {} {}",
        shell_quote(factor_str),
        shell_quote("rebase-exec-preflight"),
        shell_quote("1"),
        shell_quote("true")
    );
    let begin = format!(
        "{} {} {} {} {} {} {}",
        shell_quote(factor_str),
        shell_quote("rebase-exec-begin"),
        shell_quote("1"),
        shell_quote(start_head),
        shell_quote("false"),
        shell_quote("true"),
        shell_quote(commits.as_str())
    );
    [
        shell_quote(editor_str),
        shell_quote("--factor-target"),
        shell_quote("bbbbbbb"),
        shell_quote("--factor-preflight"),
        shell_quote(preflight.as_str()),
        shell_quote("--factor-begin"),
        shell_quote(begin.as_str()),
    ]
    .join(" ")
}

fn start_range_ref_runner(repo: &Path, sha_a: &str, sha_b: &str) -> ScriptedRunner {
    let tree = "0123456789abcdef0123456789abcdef01234567";
    with_start_gate_result(
        with_git_dir_outputs(ScriptedRunner::default(), repo, 1)
            .with_output("git", &["status", "--porcelain=v1"], repo, "")
            .with_output(
                "git",
                &["rev-list", "--reverse", "--ancestry-path", "a^..b"],
                repo,
                &format!("{sha_a}\n{sha_b}\n"),
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
            .with_commit_object(repo, sha_a, Some(&"d".repeat(SHA_LEN)))
            .with_commit_object(repo, sha_b, Some(sha_a))
            .with_output(
                "git",
                &["rev-parse", "--verify", &format!("{sha_b}^")],
                repo,
                &format!("{sha_a}\n"),
            )
            .with_output(
                "git",
                &["rev-parse", "--verify", "HEAD"],
                repo,
                &format!("{sha_b}\n"),
            )
            .with_commit_object(repo, sha_a, Some(&"d".repeat(SHA_LEN)))
            .with_output("git", &["rev-parse", "--short", sha_b], repo, "bbbbbbb\n")
            .with_output(
                "git",
                &["show", "--format=%B", "--no-patch", sha_b],
                repo,
                "msg\n",
            ),
        repo,
        "true",
        0,
        "",
        "",
    )
    .with_output(
        "git",
        &["rev-parse", HEAD_TREEISH],
        repo,
        &format!("{tree}\n"),
    )
    .with_status(
        "git",
        &["reset", "--quiet", &format!("{sha_a}^")],
        &[],
        false,
        repo,
        0,
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

/// An unavailable fallback commit refuses public abort before Git mutation.
pub(in crate::git_factor) fn verify_public_abort_fallback_refusal(index: u8) {
    let directory = TempDir::new().or_abort("abort fallback refusal fixture");
    let repo = directory.path();
    let state = setup_factor_state(
        repo,
        &"a".repeat(SHA_LEN),
        "1\n",
        Some("false\n"),
        Some(TREE_EXPECTED_NL),
    );
    fs::write(state.join("current_index"), format!("{index}\n"))
        .or_abort("unavailable saved commit");
    fs::write(repo.join("unrelated"), b"user bytes\n").or_abort("unrelated abort input");
    let journal_bytes = || {
        let mut files = fs::read_dir(&state)
            .or_abort("abort journal inventory")
            .map(|entry| {
                let file = entry.or_abort("abort journal entry");
                (
                    file.file_name(),
                    fs::read(file.path()).or_abort("abort journal bytes"),
                )
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files
    };
    let before = journal_bytes();
    let runner = ScriptedRunner::default();
    let io = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let code = main_entry_with_vec(
        &io,
        ctx_from_parts(&environment, &runner, &io, &REAL_FS),
        &[OsString::from("git-factor"), OsString::from("--abort")],
    );

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(io.stdout(), "");
    assert_eq!(
        io.stderr(),
        format!("git command failed: commit index {index} out of range (have 1 commits)\n")
    );
    let mut after = journal_bytes();
    let diagnostic_position = after
        .iter()
        .position(|entry| entry.0 == "error.log")
        .or_abort("added diagnostic entry");
    after.remove(diagnostic_position);
    assert_eq!(after, before);
    let diagnostic =
        fs::read_to_string(state.join("error.log")).or_abort("abort refusal diagnostic");
    assert!(
        diagnostic
            .lines()
            .any(|line| line == "argv=git-factor --abort")
    );
    assert!(diagnostic.lines().any(|line| line
        == format!(
            "error=git command failed: commit index {index} out of range (have 1 commits)"
        )));
    assert_eq!(
        fs::read(repo.join("unrelated")).or_abort("preserved unrelated input"),
        b"user bytes\n"
    );
}

/// Invalid persisted phases refuse public status before output or mutation.
pub(in crate::git_factor) fn verify_public_status_phase_refusal(phase: &str) {
    let directory = TempDir::new().or_abort("status phase refusal fixture");
    let repo = directory.path();
    let state = setup_factor_state(
        repo,
        &"a".repeat(SHA_LEN),
        "1\n",
        Some("false\n"),
        Some(TREE_EXPECTED_NL),
    );
    fs::write(state.join("phase"), format!("{phase}\n")).or_abort("invalid saved phase");
    fs::write(repo.join("unrelated"), b"user bytes\n").or_abort("unrelated status input");
    let journal_bytes = || {
        let mut files = fs::read_dir(&state)
            .or_abort("status journal inventory")
            .map(|entry| {
                let file = entry.or_abort("status journal entry");
                (
                    file.file_name(),
                    fs::read(file.path()).or_abort("status journal bytes"),
                )
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files
    };
    let expected_error = if phase.is_empty() {
        "git command failed: corrupted state file 'phase': file is empty".to_owned()
    } else {
        format!("git command failed: corrupted state file 'phase': invalid value '{phase}'")
    };
    let before = journal_bytes();
    let runner = ScriptedRunner::default();
    let io = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let code = main_entry_with_vec(
        &io,
        ctx_from_parts(&environment, &runner, &io, &REAL_FS),
        &[OsString::from("git-factor"), OsString::from("--status")],
    );

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(io.stdout(), "");
    assert_eq!(io.stderr(), format!("{expected_error}\n"));
    let mut after = journal_bytes();
    let diagnostic_position = after
        .iter()
        .position(|entry| entry.0 == "error.log")
        .or_abort("added diagnostic entry");
    after.remove(diagnostic_position);
    assert_eq!(after, before);
    let diagnostic =
        fs::read_to_string(state.join("error.log")).or_abort("status refusal diagnostic");
    assert!(
        diagnostic
            .lines()
            .any(|line| line == "argv=git-factor --status")
    );
    assert!(
        diagnostic
            .lines()
            .any(|line| line == format!("error={expected_error}"))
    );
    assert_eq!(
        fs::read(repo.join("unrelated")).or_abort("preserved unrelated input"),
        b"user bytes\n"
    );
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
    let ctx = ctx_from_parts(&env, &runner, &io, &REAL_FS).or_abort("ctx");

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
    let ctx = ctx_from_parts(&env, &runner, &io, &REAL_FS).or_abort("ctx");

    let err = repo_status_stdout(&ctx).err_or_abort("stderr should be rejected");

    assert_eq!(
        err.to_string(),
        "git command failed: git status --porcelain=v1 produced unexpected output (exit 0)\nSTDERR:\nwarning: odd status output"
    );
}

#[test]
fn repo_status_matches_policy_distinguishes_fully_clean_and_staged_only() {
    assert!(repo_status_matches_policy("", RepoStatePolicy::FullyClean));
    assert!(repo_status_matches_policy(
        "M  src/git_factor.rs\n",
        RepoStatePolicy::StagedOnly
    ));
    assert!(!repo_status_matches_policy(
        "M  src/git_factor.rs\n",
        RepoStatePolicy::FullyClean
    ));
    assert!(!repo_status_matches_policy(
        " M src/git_factor.rs\n",
        RepoStatePolicy::StagedOnly
    ));
    assert!(!repo_status_matches_policy(
        "?? scratch.txt\n",
        RepoStatePolicy::StagedOnly
    ));
    assert!(!repo_status_matches_policy(
        "!! target/\n",
        RepoStatePolicy::StagedOnly
    ));
}

#[test]
fn ensure_repo_state_reports_status_when_policy_is_violated() {
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
        "?? scratch.txt\n",
        "",
    );
    let ctx = ctx_from_parts(&env, &runner, &io, &REAL_FS).or_abort("ctx");

    let err = ensure_repo_state(&ctx, RepoStatePolicy::FullyClean, "repo must be clean")
        .err_or_abort("dirty state should fail");

    assert_eq!(
        err.to_string(),
        "git command failed: repo must be clean\nSTATUS:\n?? scratch.txt"
    );
}

#[test]
fn current_exe_command_prefix_shell_quotes_current_executable() {
    let dir = TempDir::new().or_abort("tempdir");
    let env = TestEnv {
        cwd: dir.path().join("dir with spaces"),
    };
    let io = TestIo::default();
    let runner = ScriptedRunner::default();
    let ctx = ctx_from_parts(&env, &runner, &io, &REAL_FS).or_abort("ctx");

    let prefix = current_exe_command_prefix(&ctx).or_abort("current exe prefix");

    assert_eq!(
        prefix,
        shell_quote(env.cwd.join("git-factor").to_str().or_abort("utf-8 path"))
    );
}

#[test]
fn current_exe_command_prefix_propagates_current_exe_failures() {
    let dir = TempDir::new().or_abort("tempdir");
    let env = ExeFailingEnv {
        cwd: dir.path().to_path_buf(),
    };
    let io = TestIo::default();
    let runner = ScriptedRunner::default();
    let ctx = ctx_from_parts(&env, &runner, &io, &REAL_FS).or_abort("ctx");

    let err = current_exe_command_prefix(&ctx).err_or_abort("current_exe should fail");

    assert_eq!(
        err.to_string(),
        "git command failed: cannot resolve current executable: no exe"
    );
}

#[test]
fn rebase_exec_hidden_commands_quote_all_dynamic_arguments() {
    let dir = TempDir::new().or_abort("tempdir");
    let env = TestEnv {
        cwd: dir.path().join("dir with spaces"),
    };
    let io = TestIo::default();
    let runner = ScriptedRunner::default();
    let ctx = ctx_from_parts(&env, &runner, &io, &REAL_FS).or_abort("ctx");
    let exec_command =
        NonEmptyString::try_from("just ci && cargo test --quiet".to_owned()).or_abort("exec");
    let start_head = CommitSha::new("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned())
        .or_abort("start head");
    let mut commits = NonEmpty::new(
        CommitSha::new("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned()).or_abort("sha a"),
    );
    commits.push(
        CommitSha::new("cccccccccccccccccccccccccccccccccccccccc".to_owned()).or_abort("sha c"),
    );

    let preflight =
        rebase_exec_preflight_command(&ctx, CurrentIndex(3), &exec_command).or_abort("preflight");
    let begin = rebase_exec_begin_command(
        &ctx,
        CurrentIndex(3),
        &start_head,
        true,
        &exec_command,
        &commits,
    )
    .or_abort("begin");

    assert_eq!(
        preflight,
        format!(
            "{} {} {} {}",
            shell_quote(
                env.cwd
                    .join("git-factor")
                    .to_str()
                    .or_abort("utf-8 current exe"),
            ),
            shell_quote("rebase-exec-preflight"),
            shell_quote("3"),
            shell_quote(exec_command.as_str()),
        )
    );
    assert_eq!(
        begin,
        format!(
            "{} {} {} {} {} {} {}",
            shell_quote(
                env.cwd
                    .join("git-factor")
                    .to_str()
                    .or_abort("utf-8 current exe"),
            ),
            shell_quote("rebase-exec-begin"),
            shell_quote("3"),
            shell_quote(start_head.as_str()),
            shell_quote("true"),
            shell_quote(exec_command.as_str()),
            shell_quote(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa,cccccccccccccccccccccccccccccccccccccccc",
            ),
        )
    );
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
fn editor_path_errors_when_current_exe_fails() {
    let dir = TempDir::new().or_abort("tempdir");
    let env = ExeFailingEnv {
        cwd: dir.path().to_path_buf(),
    };
    let io = TestIo::default();
    let ctx = ctx_from_parts(&env, &REAL_RUNNER, &io, &REAL_FS).or_abort("ctx ok");

    let err = editor_path(&ctx).err_or_abort("expected editor_path to error");

    assert_eq!(env.var_os("ANY"), None);
    assert_eq!(
        err.to_string(),
        "git command failed: cannot resolve current exe: no exe"
    );
}

#[test]
fn editor_path_errors_when_exe_cannot_be_canonicalized() {
    let dir = TempDir::new().or_abort("tempdir");
    let env = TestEnv {
        cwd: dir.path().to_path_buf(),
    };
    let io = TestIo::default();
    let ctx = ctx_from_parts(&env, &REAL_RUNNER, &io, &REAL_FS).or_abort("ctx ok");

    let err = editor_path(&ctx).err_or_abort("expected editor_path to error");

    assert!(
        err.to_string()
            .starts_with("git command failed: cannot canonicalize exe: "),
        "unexpected error: {err}"
    );
}

#[test]
fn editor_path_errors_when_executable_has_no_parent_dir() {
    let dir = TempDir::new().or_abort("tempdir");
    let env = RootExeEnv {
        cwd: dir.path().to_path_buf(),
    };
    let io = TestIo::default();
    let ctx = ctx_from_parts(&env, &REAL_RUNNER, &io, &REAL_FS).or_abort("ctx ok");

    let err = editor_path(&ctx).err_or_abort("expected editor_path to error");

    assert_eq!(env.var_os("ANY"), None);
    assert_eq!(
        err.to_string(),
        "git command failed: executable has no parent directory"
    );
}

#[cfg(unix)]
#[test]
fn editor_path_returns_non_utf8_path_when_fs_is_non_utf8() {
    let dir = TempDir::new().or_abort("tempdir");
    let env = TestEnv {
        cwd: dir.path().to_path_buf(),
    };
    let io = TestIo::default();
    let fs = NonUtf8Fs;
    let ctx = ctx_from_parts(&env, &REAL_RUNNER, &io, &fs).or_abort("ctx ok");

    let mkdir = dir.path().join("mkdir");
    fs.create_dir_all(&mkdir).or_abort("mkdir ok");
    assert!(mkdir.is_dir());

    let rm_dir = dir.path().join("rm_dir");
    fs.create_dir_all(&rm_dir).or_abort("rm_dir create");
    fs.remove_dir_all(&rm_dir).or_abort("rm_dir remove");
    assert!(!rm_dir.exists());

    let rm_file = dir.path().join("rm_file");
    fs::write(&rm_file, "x").or_abort("rm_file write");
    fs.remove_file(&rm_file).or_abort("rm_file remove");
    assert!(!rm_file.exists());

    let read_file = dir.path().join("read_to_string");
    fs::write(&read_file, "hello").or_abort("read_file write");
    let content = fs.read_to_string(&read_file).or_abort("read_to_string ok");
    assert_eq!(content, "hello");

    let write_file = dir.path().join("write_string");
    fs.write_string(&write_file, "world")
        .or_abort("write_string ok");
    let written = fs.read_to_string(&write_file).or_abort("read back ok");
    assert_eq!(written, "world");

    let is_dir = fs.is_dir(&mkdir);
    assert!(is_dir);

    let exists = fs.exists(&mkdir);
    assert!(exists);

    let path = editor_path(&ctx).or_abort("editor_path should return non-UTF-8 PathBuf");
    assert!(
        path.to_str().is_none(),
        "path should not be valid UTF-8: {path:?}"
    );
}

#[test]
fn resolve_commit_refs_errors_on_invalid_rev_list_range() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output("git", &["status", "--porcelain=v1"], repo, "");
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
    .err_or_abort("expected invalid commit");
    assert_eq!(err.to_string(), "invalid commit: bad..range");
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

#[test]
fn print_session_started_single_commit_without_untracked_or_claude_hints() {
    let dir = TempDir::new().or_abort("tempdir");
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
    print_session_started(
        &ctx,
        "FACTOR: Split session started for aaaaaaa.",
        "subject",
    )
    .or_abort("print should succeed");

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
        "Run git factor -h for command help or git-factor --help for the full workflow guide.\n",
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
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let reference_dir = repo.join("references");
    fs::create_dir_all(&reference_dir).or_abort("create references dir");
    let rust_ref = reference_dir.join("rust.md");
    fs::write(&rust_ref, "# rust\n").or_abort("write rust reference");

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
    assert_eq!(env.current_dir().or_abort("cwd"), repo);
    assert_eq!(env.current_exe().or_abort("exe"), repo.join("git-factor"));
    assert_eq!(env.var_os("CLAUDECODE"), Some(OsString::from("1")));
    assert_eq!(env.var_os("ANY"), None);
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    print_session_started(
        &ctx,
        "FACTOR: Split session started for 2 commits (tip: aaaaaaa).",
        "subject",
    )
    .or_abort("print should succeed");

    let expected = format!(
        concat!(
            "FACTOR: Split session started for 2 commits (tip: aaaaaaa).\n",
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
            "Run git factor -h for command help or git-factor --help for the full workflow guide.\n",
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
    let rm_file = repo.join("rm-file.txt");
    fs.write_string(&rm_file, "x").or_abort("write rm file");
    fs.remove_file(&rm_file).or_abort("remove rm file");
    assert!(!fs.exists(&rm_file));
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

fn assert_cmd_start_state_write_failure<F>(repo: &Path, fs: &F, expected_error: &str)
where
    F: Fs,
{
    assert_fs_adapter_basics(repo, fs);
    let sha = "a".repeat(SHA_LEN);
    let runner = with_start_gate_result(
        start_single_head_validation_runner(repo, &sha)
            .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
            .with_output(
                "git",
                &["show", "--format=%B", "--no-patch", &sha],
                repo,
                "msg\n",
            ),
        repo,
        "true",
        0,
        "",
        "",
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
        fs,
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
    .err_or_abort("expected state write failure");

    assert_eq!(err.to_string(), expected_error);
}

#[test]
fn main_entry_with_prints_error_when_ctx_cannot_be_built() {
    let io = TestIo::default();
    let env = FailingEnv { message: "no cwd" };
    let ctx = ctx_from_parts(&env, &REAL_RUNNER, &io, &REAL_FS);

    let code = main_entry_with_vec(&io, ctx, &[OsString::from("git-factor")]);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(
        io.stderr(),
        "git command failed: cannot resolve cwd: no cwd\n"
    );
    assert_eq!(io.stdout(), "");
}

#[test]
fn advance_to_next_commit_propagates_io_error_when_outln_fails_after_rebase_finishes() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(state_dir.join("split_count"), "3\n").or_abort("write split_count");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");

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

    let outcome = Session::with_state(&ctx, StateDir::new(state_dir), vec![])
        .advance_to_next_commit()
        .or_abort("advance should succeed without IO");
    assert!(
        matches!(&outcome, AdvanceOutcome::Completed { .. }),
        "expected Completed"
    );
    let AdvanceOutcome::Completed { final_split_count } = outcome;
    assert_eq!(final_split_count.get(), 3);
}

#[test]
fn advance_to_next_commit_ignores_current_index_after_span_completion() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(state_dir.join("split_count"), "1\n").or_abort("write split_count");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("current_index"), format!("{}\n", usize::MAX))
        .or_abort("write current_index");

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

    let outcome = Session::with_state(&ctx, StateDir::new(state_dir.clone()), vec![])
        .advance_to_next_commit()
        .or_abort("advance should succeed without reading current_index");
    assert!(
        matches!(outcome, AdvanceOutcome::Completed { final_split_count } if final_split_count.get() == 1),
        "expected Completed with split_count 1"
    );
    assert!(
        !state_dir.exists(),
        "state dir should be removed after completion"
    );
}

#[test]
fn advance_to_next_commit_errors_when_rebase_is_required_but_not_in_progress() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(&git_dir).or_abort("create git dir");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(state_dir.join("split_count"), "1\n").or_abort("write split_count");

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

    let err = Session::with_state(&ctx, StateDir::new(state_dir), vec![])
        .advance_to_next_commit()
        .err_or_abort("expected no rebase error");
    assert_eq!(err.to_string(), "git command failed: no rebase in progress");
}

#[test]
fn advance_to_next_commit_error_includes_abort_hint() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).or_abort("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let next_commit = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{next_commit}\n")).or_abort("write commits");
    fs::write(state_dir.join("split_count"), "1\n").or_abort("write split_count");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
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

    let err = Session::with_state(
        &ctx,
        StateDir::new(state_dir),
        vec![CommitSha::new(next_commit).or_abort("sha")],
    )
    .advance_to_next_commit()
    .err_or_abort("rebase --continue failure must return error");

    let msg = err.to_string();
    assert!(
        msg.contains("--abort"),
        "error should contain --abort hint but was: {msg}"
    );
    assert!(
        msg.contains("git rebase failed"),
        "error should contain original failure message but was: {msg}"
    );
}

#[test]
fn advance_to_next_commit_omits_untracked_section_when_rebase_finishes() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let rebase_dir = git_dir.join("rebase-merge");
    fs::create_dir_all(&rebase_dir).or_abort("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let current = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{current}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("split_count"), "2\n").or_abort("write split_count");
    fs::write(state_dir.join("requires_rebase"), "true\n").or_abort("write requires_rebase");

    let base_runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        )
        .with_status("git", &["reset", "--quiet"], &[], false, repo, 0)
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
            repo.to_string_lossy().as_ref(),
        );
    let runner = RebaseContinueCompletesRunner::new(base_runner, rebase_dir);
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

    let commits = vec![CommitSha::new(current).or_abort("sha")];
    let outcome = Session::with_state(&ctx, StateDir::new(state_dir), commits)
        .advance_to_next_commit()
        .or_abort("advance should succeed");
    assert!(
        matches!(outcome, AdvanceOutcome::Completed { final_split_count } if final_split_count.get() == 2),
        "expected Completed with split_count 2"
    );
    assert!(io.stdout().is_empty(), "advance should produce no output");
    assert_eq!(io.stderr(), "");
}

#[test]
fn rehydrate_pool_preserving_index_succeeds_when_cherry_pick_succeeds() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();

    let runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, TREE_REHYDRATE_NL)
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(SHA_LEN),
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 0)
        .with_status("git", &["read-tree", TREE_REHYDRATE], &[], false, repo, 0);

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

    let commit = CommitSha::new("a".repeat(SHA_LEN)).or_abort("sha");
    let result = rehydrate_pool_preserving_index(&ctx, &commit);

    assert!(result.is_ok(), "rehydrate should succeed");
    assert_eq!(io.stdout(), "");
    assert_eq!(io.stderr(), "");
}

#[test]
fn rehydrate_pool_preserving_index_propagates_cherry_pick_status_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();

    let runner =
        ScriptedRunner::default().with_output("git", &["write-tree"], repo, TREE_REHYDRATE_NL);

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

    let commit = CommitSha::new("a".repeat(SHA_LEN)).or_abort("sha");
    let err = rehydrate_pool_preserving_index(&ctx, &commit).err_or_abort("expected error");

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
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();

    let runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, TREE_REHYDRATE_NL)
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(SHA_LEN),
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

    let commit = CommitSha::new("a".repeat(SHA_LEN)).or_abort("sha");
    let err =
        rehydrate_pool_preserving_index(&ctx, &commit).err_or_abort("expected conflict error");

    assert_eq!(
        err.to_string(),
        "git command failed: rehydrate cherry-pick left conflicts:\nconflict.txt"
    );
}

#[test]
fn rehydrate_pool_preserving_index_reports_non_conflict_failure_even_when_quit_fails() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();

    let runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, TREE_REHYDRATE_NL)
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(SHA_LEN),
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

    let commit = CommitSha::new("a".repeat(SHA_LEN)).or_abort("sha");
    let err = rehydrate_pool_preserving_index(&ctx, &commit)
        .err_or_abort("non-conflict failure must return error");

    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("cherry-pick failed") && msg.contains("no merge conflicts")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn rehydrate_pool_preserving_index_reports_read_tree_failure() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();

    let runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, TREE_REHYDRATE_NL)
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(SHA_LEN),
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 0)
        .with_status("git", &["read-tree", TREE_REHYDRATE], &[], false, repo, 2);

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

    let commit = CommitSha::new("a".repeat(SHA_LEN)).or_abort("sha");
    let err =
        rehydrate_pool_preserving_index(&ctx, &commit).err_or_abort("expected read-tree error");

    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git read-tree failed (exit ")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn rehydrate_pool_preserving_index_reports_write_tree_output_error() {
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

    let commit = CommitSha::new("a".repeat(SHA_LEN)).or_abort("sha");
    let err =
        rehydrate_pool_preserving_index(&ctx, &commit).err_or_abort("expected write-tree error");

    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git write-tree:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn rehydrate_pool_preserving_index_rejects_invalid_write_tree_hash() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();

    let runner =
        ScriptedRunner::default().with_output("git", &["write-tree"], repo, "not-a-valid-hash\n");
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

    let commit = CommitSha::new("a".repeat(SHA_LEN)).or_abort("sha");
    let err = rehydrate_pool_preserving_index(&ctx, &commit)
        .err_or_abort("invalid write-tree hash should fail");

    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("invalid tree hash")),
        "err was: {err:?}"
    );
}

#[test]
fn rehydrate_pool_preserving_index_propagates_unmerged_query_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();

    let runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, TREE_REHYDRATE_NL)
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(SHA_LEN),
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

    let commit = CommitSha::new("a".repeat(SHA_LEN)).or_abort("sha");
    let err = rehydrate_pool_preserving_index(&ctx, &commit)
        .err_or_abort("expected unmerged query error");

    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git diff:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn rehydrate_pool_preserving_index_returns_error_on_non_conflict_failure() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();

    let runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, TREE_REHYDRATE_NL)
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(SHA_LEN),
            ],
            &[],
            false,
            repo,
            1,
        )
        .with_output("git", &["diff", "--name-only", "--diff-filter=U"], repo, "")
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 0);

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

    let commit = CommitSha::new("a".repeat(SHA_LEN)).or_abort("sha");

    let err = rehydrate_pool_preserving_index(&ctx, &commit)
        .err_or_abort("non-conflict cherry-pick failure must return error");

    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("cherry-pick failed") && msg.contains("no merge conflicts")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn increment_split_count_propagates_state_write_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create state dir");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");

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

    let err = Session::with_state(&ctx, StateDir::new(state_dir), vec![])
        .increment_split_count()
        .err_or_abort("expected state write error");
    assert!(
        matches!(&err, FactorError::StateWrite(inner) if inner.to_string().contains("split_count write failed")),
        "err was: {err:?}"
    );
}

#[test]
fn capture_expected_tree_in_state_propagates_git_output_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create state dir");

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

    let err = capture_expected_tree_in_state(&ctx, &StateDir::new(state_dir))
        .err_or_abort("expected git output error");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git rev-parse:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn capture_expected_tree_in_state_propagates_state_write_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create state dir");

    let runner = ScriptedRunner::default().with_output(
        "git",
        &["rev-parse", "HEAD^{tree}"],
        repo,
        &format!("{}\n", "c".repeat(SHA_LEN)),
    );
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

    let err = capture_expected_tree_in_state(&ctx, &StateDir::new(state_dir))
        .err_or_abort("expected state write error");
    assert!(
        matches!(&err, FactorError::StateWrite(inner) if inner.to_string().contains("expected_tree write failed")),
        "err was: {err:?}"
    );
}

#[test]
fn capture_expected_tree_rejects_invalid_hash() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create state dir");

    let runner = ScriptedRunner::default().with_output(
        "git",
        &["rev-parse", "HEAD^{tree}"],
        repo,
        "not-a-valid-hash\n",
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

    let err = capture_expected_tree_in_state(&ctx, &StateDir::new(state_dir))
        .err_or_abort("invalid hash must return error");

    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("invalid tree hash")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn capture_expected_tree_rejects_non_hex_40_char_hash() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create state dir");

    let invalid_tree = format!("{}\n", "g".repeat(SHA_LEN));
    let runner = ScriptedRunner::default().with_output(
        "git",
        &["rev-parse", "HEAD^{tree}"],
        repo,
        &invalid_tree,
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

    let err = capture_expected_tree_in_state(&ctx, &StateDir::new(state_dir))
        .err_or_abort("non-hex hash must return error");

    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("invalid tree hash")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn write_state_pairs_propagates_first_state_write_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create state dir");

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
        &StateDir::new(state_dir),
        &[
            (StateFileKey::CurrentIndex, "0"),
            (StateFileKey::SplitCount, "1"),
            (StateFileKey::Exec, "true"),
        ],
    )
    .err_or_abort("expected state write error");
    assert!(
        matches!(&err, FactorError::StateWrite(inner) if inner.to_string().contains("split_count write failed")),
        "err was: {err:?}"
    );
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
fn remove_empty_root_is_noop_when_root_is_not_empty() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let ctx = ctx_for(repo);

    let status = Command::new("git")
        .args(["init"])
        .current_dir(repo)
        .status()
        .or_abort("git init");
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
    drop(
        Command::new("git")
            .args(["config", "core.hooksPath", ".git/hooks"])
            .current_dir(repo)
            .status(),
    );

    fs::write(repo.join("file.txt"), "one\n").or_abort("write file");
    assert!(
        Command::new("git")
            .args(["add", "file.txt"])
            .current_dir(repo)
            .status()
            .or_abort("git add")
            .success()
    );
    assert!(
        Command::new("git")
            .args(["commit", "--no-gpg-sign", "--message", "chore: base"])
            .current_dir(repo)
            .status()
            .or_abort("git commit")
            .success()
    );

    let result = remove_empty_root_in(&ctx);

    assert!(result.is_ok(), "should be a no-op when root is not empty");
}

#[test]
fn remove_empty_root_returns_error_when_rebase_fails() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);

    // Create git-factor file so editor_path can canonicalize
    fs::write(repo.join("git-factor"), "").or_abort("create git-factor");
    let canon_repo = fs::canonicalize(repo).or_abort("canonicalize repo");
    let editor = canon_repo.join("git-sequence-editor");
    let editor_str = editor.to_str().or_abort("editor path is UTF-8");

    let short_sha = "aaa1234";
    let seq_editor = format!(
        "{} {} {}",
        shell_quote(editor_str),
        shell_quote("--drop"),
        shell_quote(short_sha)
    );

    let runner = ScriptedRunner::default()
        .with_output(
            "git",
            &["rev-list", "--max-parents=0", "HEAD"],
            repo,
            &format!("{sha}\n"),
        )
        .with_output("git", &["ls-tree", &sha], repo, "")
        .with_output(
            "git",
            &["rev-parse", "--short", &sha],
            repo,
            &format!("{short_sha}\n"),
        )
        .with_status(
            "git",
            &[
                "rebase",
                "--empty",
                "drop",
                "--interactive",
                "--no-autosquash",
                "--no-update-refs",
                "--quiet",
                "--root",
            ],
            &[
                ("GIT_EDITOR", "false"),
                ("GIT_SEQUENCE_EDITOR", &seq_editor),
            ],
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

    let err = remove_empty_root_in(&ctx).err_or_abort("rebase failure must return error");

    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("rebase to remove empty root failed")),
        "unexpected error: {err:?}"
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
fn run_and_report_writes_error_log_for_unexpected_active_session_failure() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");

    let runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 4)
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
        fs: &FailingRemoveDirAllFs,
    };

    let code = run_and_report_with_args_vec(
        &ctx,
        &[OsString::from("git-factor"), OsString::from("--abort")],
    );

    assert_eq!(code, EXIT_SOFTWARE);
    assert!(
        io.stderr().contains("failed to remove factor state path"),
        "stderr was: {}",
        io.stderr()
    );
    let error_log =
        fs::read_to_string(state_dir.join("error.log")).or_abort("error log should exist");
    assert!(error_log.contains("argv=git-factor --abort"));
    assert!(error_log.contains("error=git command failed: failed to remove factor state path"));
    assert!(error_log.contains(format!("factor_current_commit={sha}").as_str()));
}

#[test]
fn run_and_report_skips_error_log_for_expected_continue_recovery() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");

    let runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 2).with_status(
        "git",
        &["diff", "--quiet", "--staged"],
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

    let code = run_and_report_with_args_vec(
        &ctx,
        &[
            OsString::from("git-factor"),
            OsString::from("--continue"),
            OsString::from("--message"),
            OsString::from("test: next"),
        ],
    );

    assert_eq!(code, EXIT_USAGE);
    assert!(
        io.stderr().contains("no staged changes to commit"),
        "stderr was: {}",
        io.stderr()
    );
    assert!(
        !state_dir.join("error.log").exists(),
        "error log should not be written for expected recovery cases"
    );
}

#[test]
fn write_error_log_includes_trace_path_and_error_sources() {
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

    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let sha = "a".repeat(SHA_LEN);
    let tree = "b".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "2\n").or_abort("write split_count");
    fs::write(state_dir.join("requires_rebase"), "true\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), format!("{tree}\n")).or_abort("write expected tree");

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
    let error = FactorError::StateWrite(io::Error::other(OuterCause(InnerCause)));

    write_error_log(&ctx, &args, &error).or_abort("write error log should succeed");

    let error_log =
        fs::read_to_string(state_dir.join(ERROR_LOG_FILE)).or_abort("error log should exist");
    assert!(error_log.contains(format!("trace_log={}", trace_log.display()).as_str()));
    assert!(error_log.contains("source_0=outer cause"));
    assert!(error_log.contains("source_1=root cause"));
    assert!(error_log.contains("factor_split_count=2"));
    assert!(error_log.contains("factor_requires_rebase=true"));
    assert!(error_log.contains("staged_paths=staged.txt"));
    assert!(error_log.contains("unstaged_paths=unstaged.txt"));
    assert!(error_log.contains("untracked_paths=new.txt"));
}

#[test]
fn write_error_log_includes_false_requires_rebase_and_apply_state() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    let rebase_apply = git_dir.join(REBASE_APPLY_DIR);
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::create_dir_all(&rebase_apply).or_abort("create rebase-apply dir");

    let sha = "c".repeat(SHA_LEN);
    let tree = "d".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
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

    write_error_log(&ctx, &args, &error).or_abort("write error log should succeed");

    let error_log =
        fs::read_to_string(state_dir.join(ERROR_LOG_FILE)).or_abort("error log should exist");
    assert!(error_log.contains("factor_requires_rebase=false"));
    assert!(error_log.contains(format!("factor_current_commit={sha}").as_str()));
    assert!(error_log.contains("rebase_state=rebase-apply"));
    assert!(error_log.contains("rebase_msgnum=3"));
    assert!(error_log.contains("rebase_end=5"));
    assert!(error_log.contains("rebase_todo_head=patch"));
}

#[test]
fn push_snapshot_fields_includes_false_requires_rebase() {
    let mut buf = String::new();
    let snapshot = RepoSnapshot {
        factor_requires_rebase: Some(StateBool::from_bool(false)),
        ..RepoSnapshot::default()
    };

    push_snapshot_fields(&mut buf, "state", &snapshot);

    assert!(buf.contains("\"state_factor_requires_rebase\":\"false\""));
}

#[test]
fn push_snapshot_fields_includes_true_requires_rebase() {
    let mut buf = String::new();
    let snapshot = RepoSnapshot {
        factor_requires_rebase: Some(StateBool::from_bool(true)),
        ..RepoSnapshot::default()
    };

    push_snapshot_fields(&mut buf, "state", &snapshot);

    assert!(buf.contains("\"state_factor_requires_rebase\":\"true\""));
}

#[test]
fn run_with_args_rejects_abort_when_combined_with_status() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("abort/status should be rejected");

    assert_eq!(
        err.to_string(),
        "--abort cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_abort_when_combined_with_continue() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("abort/continue should be rejected");

    assert_eq!(
        err.to_string(),
        "--abort cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_abort_when_combined_with_finish() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("abort/finish should be rejected");

    assert_eq!(
        err.to_string(),
        "--abort cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_abort_when_combined_with_exec() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("abort/exec should be rejected");

    assert_eq!(
        err.to_string(),
        "--abort cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_abort_when_combined_with_commit() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("abort/commit should be rejected");

    assert_eq!(
        err.to_string(),
        "--abort cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_status_when_combined_with_message() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("status/message should be rejected");

    assert_eq!(
        err.to_string(),
        "--status cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_status_when_combined_with_continue() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("status/continue should be rejected");

    assert_eq!(
        err.to_string(),
        "--status cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_status_when_combined_with_finish() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("status/finish should be rejected");

    assert_eq!(
        err.to_string(),
        "--status cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_status_when_combined_with_exec() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("status/exec should be rejected");

    assert_eq!(
        err.to_string(),
        "--status cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_status_when_combined_with_commit() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("status/commit should be rejected");

    assert_eq!(
        err.to_string(),
        "--status cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_finish_when_combined_with_continue() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("finish/continue should be rejected");

    assert_eq!(
        err.to_string(),
        "--finish cannot be combined with --continue, --exec, or COMMIT"
    );
}

#[test]
fn run_with_args_rejects_finish_when_combined_with_exec() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("finish/exec should be rejected");

    assert_eq!(
        err.to_string(),
        "--finish cannot be combined with --continue, --exec, or COMMIT"
    );
}

#[test]
fn run_with_args_rejects_finish_when_combined_with_commit() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("finish/commit should be rejected");

    assert_eq!(
        err.to_string(),
        "--finish cannot be combined with --continue, --exec, or COMMIT"
    );
}

#[test]
fn run_with_args_rejects_continue_when_combined_with_exec() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("continue/exec should be rejected");

    assert_eq!(
        err.to_string(),
        "--continue cannot be combined with --exec or COMMIT"
    );
}

#[test]
fn run_with_args_rejects_continue_when_combined_with_commit() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("continue/commit should be rejected");

    assert_eq!(
        err.to_string(),
        "--continue cannot be combined with --exec or COMMIT"
    );
}

#[test]
fn run_with_args_rejects_retry_when_combined_with_message() {
    let dir = TempDir::new().or_abort("tempdir");
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
            OsString::from("--retry"),
            OsString::from("--message"),
            OsString::from("msg"),
        ],
    )
    .err_or_abort("retry/message should be rejected");

    assert_eq!(
        err.to_string(),
        "--retry cannot be combined with other options"
    );
}

#[test]
fn run_with_args_rejects_continue_without_message() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("continue without message should be rejected");

    assert_eq!(err.to_string(), "--continue requires --message <MSG>");
}

#[test]
fn run_with_args_rejects_message_when_not_continuing_or_finishing() {
    let dir = TempDir::new().or_abort("tempdir");
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
    .err_or_abort("message without continue/finish should be rejected");

    assert_eq!(
        err.to_string(),
        "--message can only be used with --continue or --finish"
    );
}

#[test]
fn run_with_args_rejects_message_without_exec_or_commit() {
    let dir = TempDir::new().or_abort("tempdir");
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
            OsString::from("--message"),
            OsString::from("msg"),
        ],
    )
    .err_or_abort("message without exec/commit should be rejected");

    assert_eq!(
        err.to_string(),
        "--exec <COMMAND> is required when starting a factor session"
    );
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
    assert!(stderr.contains("--exec <COMMAND> is required"));
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
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output("git", &["status", "--porcelain=v1"], repo, "");
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
        ],
    )
    .err_or_abort("expected missing HEAD resolution to fail");
    assert!(
        matches!(&err, FactorError::InvalidCommit(commit) if commit == "HEAD"),
        "err was: {err:?}"
    );
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
        &[OsString::from("git-factor"), OsString::from("--status")],
    );
    assert_eq!(code, EXIT_OK);
    assert_eq!(io.stdout(), "FACTOR: No active session.\n");
}

#[test]
fn run_with_args_finish_delegates_to_finish_handler() {
    let dir = TempDir::new().or_abort("tempdir");
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
        vec![OsString::from("git-factor"), OsString::from("--retry")],
    )
    .err_or_abort("retry should delegate to command handler");
    assert!(
        matches!(err, FactorError::NoActiveSession),
        "err was: {err:?}"
    );
}

#[test]
fn run_with_args_start_accepts_explicit_commit_ref() {
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
    let ctx = ctx_from_parts(&REAL_ENV, &REAL_RUNNER, &REAL_IO, &REAL_FS).or_abort("real_ctx");
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
fn cmd_continue_errors_on_split_count_overflow() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).or_abort("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n", "a".repeat(SHA_LEN)),
    )
    .or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("split_count"), format!("{}\n", u8::MAX))
        .or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");

    let original = "a".repeat(SHA_LEN);
    let commit_meta = "a\0b\0c\0d\0e\0f";
    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
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
            "deleted-path\n \n",
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

    let err = cmd_continue_in(&ctx, &messages).err_or_abort("expected overflow");
    assert!(
        matches!(&err, FactorError::GitCommand(err_msg) if err_msg.as_str() == "split_count overflow"),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_continue_propagates_restore_status_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).or_abort("create rebase-merge");

    let original = "a".repeat(SHA_LEN);
    setup_factor_state(repo, &original, "0\n", None, None);
    let messages = test_messages();
    let runner = continue_runner_with_commit(repo, &original, "deleted-path\n \n")
        .with_output(
            "git",
            &["rev-parse", "HEAD^{tree}"],
            repo,
            &format!("{}\n", "c".repeat(SHA_LEN)),
        )
        .with_output(
            "git",
            &["rev-parse", &format!("{original}^{{tree}}")],
            repo,
            TREE_EXPECTED_NL,
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

    let err = cmd_continue_in(&ctx, &messages).err_or_abort("expected restore status error");
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
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).or_abort("create rebase-merge");

    let original = "a".repeat(SHA_LEN);
    let split_count = format!("{}\n", u8::MAX);
    setup_factor_state(repo, &original, &split_count, None, None);
    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
    let messages = [message];
    let expected_tree_output = TREE_EXPECTED_NL;
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

    let err = cmd_finish_in(&ctx, &messages).err_or_abort("expected overflow");
    assert!(
        matches!(&err, FactorError::GitCommand(err_msg) if err_msg.as_str() == "split_count overflow"),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_propagates_restore_status_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).or_abort("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n", "a".repeat(SHA_LEN)),
    )
    .or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");

    let original = "a".repeat(SHA_LEN);
    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
    let messages = [message];

    let expected_tree_output = TREE_EXPECTED_NL;
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

    let err = cmd_finish_in(&ctx, &messages).err_or_abort("expected restore status error");
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
fn cmd_finish_rejects_invalid_actual_tree_hash_before_commit() {
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

    let messages = [NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty")];
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
        .with_output("git", &["write-tree"], repo, "not-a-valid-hash\n");
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

    let err = cmd_finish_in(&ctx, &messages).err_or_abort("invalid write-tree hash should fail");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("invalid tree hash")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_propagates_io_error_when_completion_summary_write_fails() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let original = "a".repeat(SHA_LEN);
    let state_dir = setup_factor_state(
        repo,
        &original,
        "0\n",
        Some("false\n"),
        Some(TREE_EXPECTED_NL),
    );

    let messages = [NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty")];
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
        .with_output("git", &["write-tree"], repo, TREE_EXPECTED_NL)
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
            TEST_COMMIT_META,
        )
        .with_status(
            "git",
            &["commit", "--quiet", "--message", "test: message"],
            &TEST_COMMIT_ENVS,
            false,
            repo,
            0,
        );
    let io = MatchingOutlnFailureIo {
        fail_on: "FACTOR: Complete. Final commit split into 1 commits.",
    };
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

    let err = cmd_finish_in(&ctx, &messages)
        .err_or_abort("completion summary write failure should bubble up");
    assert!(
        matches!(&err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
        "err was: {err:?}"
    );
    assert!(
        !state_dir.exists(),
        "factor state dir should be removed before summary write failure is returned"
    );
}

#[test]
fn cmd_finish_propagates_git_commit_preserving_metadata_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).or_abort("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n", "a".repeat(SHA_LEN)),
    )
    .or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");

    let original = "a".repeat(SHA_LEN);
    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
    let messages = [message];

    let expected_tree_output = TREE_EXPECTED_NL;
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

    let err = cmd_finish_in(&ctx, &messages).err_or_abort("expected metadata lookup to fail");
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
fn cmd_continue_errors_when_no_staged_changes() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n", "a".repeat(SHA_LEN)),
    )
    .or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
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

    let err = cmd_continue_in(&ctx, &messages).err_or_abort("expected no staged changes");
    assert!(
        matches!(err, FactorError::NoStagedChanges),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_retry_errors_when_no_active_session() {
    let dir = TempDir::new().or_abort("tempdir");
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

    let err = cmd_retry_in(&ctx).err_or_abort("expected no active session");
    assert!(
        matches!(err, FactorError::NoActiveSession),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_retry_errors_when_session_is_pending_start() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let original = "a".repeat(SHA_LEN);
    let state_dir = setup_factor_state(
        repo,
        &original,
        "0\n",
        Some("false\n"),
        Some(TREE_EXPECTED_NL),
    );
    fs::write(state_dir.join("phase"), "pending_start\n").or_abort("write phase");

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

    let err = cmd_retry_in(&ctx).err_or_abort("expected pending-start usage error");
    assert_eq!(
        err.to_string(),
        "run 'git factor --continue' with no --message to begin splitting this commit"
    );
}

#[test]
fn cmd_retry_errors_when_rebase_is_required_but_not_active() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let original = "a".repeat(SHA_LEN);
    setup_factor_state(
        repo,
        &original,
        "0\n",
        Some("true\n"),
        Some(TREE_EXPECTED_NL),
    );

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

    let err = cmd_retry_in(&ctx).err_or_abort("expected no rebase error");
    assert_eq!(err.to_string(), "git command failed: no rebase in progress");
}

#[test]
fn cmd_retry_restores_remaining_pool_and_prints_guidance() {
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

    let runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 2)
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
        .with_output("git", &["write-tree"], repo, TREE_EXPECTED_NL)
        .with_status("git", &["reset", "--quiet"], &[], false, repo, 0)
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
            "scratch.tmp\n",
        )
        .with_output(
            "git",
            &["rev-parse", "--show-toplevel"],
            repo,
            &format!("{}\n", repo.display()),
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

    let code = cmd_retry_in(&ctx).or_abort("retry should succeed");
    assert_eq!(code, EXIT_OK);
    let stdout = io.stdout();
    assert!(
        stdout.contains("FACTOR: Split attempt discarded."),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("UNTRACKED:"), "stdout: {stdout}");
    assert!(stdout.contains("  scratch.tmp"), "stdout: {stdout}");
}

#[test]
fn cmd_finish_errors_when_no_active_session() {
    let dir = TempDir::new().or_abort("tempdir");
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

    let err = cmd_finish_in(&ctx, &[]).err_or_abort("expected no active session");
    assert!(
        matches!(err, FactorError::NoActiveSession),
        "err was: {err:?}"
    );
}

#[test]
fn expected_tree_for_current_step_prefers_state_file() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");

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
    let original = CommitSha::new("a".repeat(SHA_LEN)).or_abort("sha");

    let tree = expected_tree_for_current_step(&ctx, &StateDir::new(state_dir), &original)
        .or_abort("expected tree from state");
    assert_eq!(tree.as_str(), TREE_EXPECTED);
}

#[test]
fn expected_tree_for_current_step_falls_back_to_original_commit_tree() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let original = "b".repeat(SHA_LEN);
    let runner = ScriptedRunner::default().with_output(
        "git",
        &["rev-parse", &format!("{original}^{{tree}}")],
        repo,
        TREE_DIFFERENT_NL,
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
    let original_sha = CommitSha::new(original).or_abort("sha");

    let tree = expected_tree_for_current_step(&ctx, &StateDir::new(state_dir), &original_sha)
        .or_abort("expected tree fallback");
    assert_eq!(tree.as_str(), TREE_DIFFERENT);
}

#[test]
fn git_commit_preserving_metadata_supports_allow_empty() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let original = "a".repeat(SHA_LEN);
    let commit_meta = "a\0b\0c\0d\0e\0f";
    let message = NonEmptyString::try_from("feat: message".to_owned()).or_abort("non-empty");
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
    let commit = CommitSha::new(original).or_abort("sha");

    git_commit_preserving_metadata(&ctx, &commit, &messages, true).or_abort("allow-empty commit");
}

#[test]
fn git_commit_preserving_metadata_errors_on_nonzero_commit_status() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let original = "a".repeat(SHA_LEN);
    let commit_meta = "a\0b\0c\0d\0e\0f";
    let message = NonEmptyString::try_from("feat: message".to_owned()).or_abort("non-empty");
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
    let commit = CommitSha::new(original).or_abort("sha");

    let err = git_commit_preserving_metadata(&ctx, &commit, &messages, false)
        .err_or_abort("expected nonzero commit status");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.starts_with("git commit failed (exit ")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_continue_reports_tree_mismatch_after_restore() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();

    let original = "a".repeat(SHA_LEN);
    setup_factor_state(repo, &original, "0\n", Some("false\n"), None);
    let messages = test_messages();
    let runner = continue_runner_with_commit(repo, &original, "")
        .with_output(
            "git",
            &["rev-parse", "HEAD^{tree}"],
            repo,
            &format!("{}\n", "c".repeat(SHA_LEN)),
        )
        .with_output(
            "git",
            &["rev-parse", &format!("{original}^{{tree}}")],
            repo,
            TREE_EXPECTED_NL,
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
        .with_output("git", &["write-tree"], repo, TREE_DIFFERENT_NL);
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

    let err = cmd_continue_in(&ctx, &messages).err_or_abort("expected tree mismatch");
    assert!(
        matches!(
            &err,
            FactorError::TreeHashMismatch { actual, expected }
                if actual.as_str() == TREE_DIFFERENT && expected.as_str() == TREE_EXPECTED
        ),
        "err was: {err:?}"
    );
}

#[test]
fn advance_to_next_commit_propagates_runner_failure_from_rebase_continue() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(git_dir.join("rebase-merge")).or_abort("create rebase-merge");

    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(state_dir.join("split_count"), "3\n").or_abort("write split_count");
    fs::write(state_dir.join("requires_rebase"), "true\n").or_abort("write requires_rebase");

    let base_runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let runner = NthRunnerFailure::new(base_runner, 2);
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let err = Session::with_state(&ctx, StateDir::new(state_dir), vec![])
        .advance_to_next_commit()
        .err_or_abort("expected forced runner failure");
    assert!(is_forced_runner_failure(&err), "err was: {err:?}");
}

#[test]
fn cmd_continue_runner_failures_cover_command_error_paths() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();

    let original = "a".repeat(SHA_LEN);
    let state_dir = setup_factor_state(
        repo,
        &original,
        "0\n",
        Some("false\n"),
        Some(TREE_EXPECTED_NL),
    );
    let messages = test_messages();
    let base_runner =
        continue_runner_with_remaining_output(repo, &original, "file.txt | 1 +\n", "newfile.txt\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    for fail_at in 4..=16 {
        if fail_at == 8 {
            continue;
        }
        fs::write(state_dir.join("split_count"), "0\n").or_abort("reset split_count");
        let runner = NthRunnerFailure::new(base_runner.clone(), fail_at);
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        let err = cmd_continue_in(&ctx, &messages).err_or_abort("expected forced runner failure");
        assert!(is_forced_runner_failure(&err), "err was: {err:?}");
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "continue gate failure coverage enumerates many distinct IO breakpoints in one transcript"
)]
fn cmd_continue_io_failures_cover_exec_gate_failure_output_paths() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n", "a".repeat(SHA_LEN)),
    )
    .or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "false\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
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
        .with_output("git", &["status", "--porcelain=v1"], repo, "M  file.txt\n")
        .with_status("bash", &["-c", "false"], &[], false, repo, 1)
        .with_output("git", &["status", "--porcelain=v1"], repo, "M  file.txt\n")
        .with_output("git", &["write-tree"], repo, TREE_REHYDRATE_NL)
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(SHA_LEN),
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 0)
        .with_status("git", &["read-tree", TREE_REHYDRATE], &[], false, repo, 0);
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let mut io_failures: u32 = 0;
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
                io_failures = io_failures
                    .checked_add(1)
                    .or_abort("counter should not overflow");
                assert!(inner.to_string().contains("io fail"), "inner was: {inner}");
            }
            Err(FactorError::ExecFailed { .. }) => {}
            other => assert!(
                matches!(
                    other,
                    Err(FactorError::Io(_) | FactorError::ExecFailed { .. })
                ),
                "unexpected result: {other:?}"
            ),
        }
    }
    assert!(io_failures > 0, "expected at least one io failure");
}

#[test]
fn cmd_continue_io_failures_cover_remaining_output_paths() {
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
    let messages = test_messages();
    let base_runner =
        continue_runner_with_remaining_output(repo, &original, "file.txt | 1 +\n", "newfile.txt\n");
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let mut io_failures: u32 = 0;
    for fail_at in 1..=SHA_LEN {
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
                io_failures = io_failures
                    .checked_add(1)
                    .or_abort("counter should not overflow");
                assert!(inner.to_string().contains("io fail"), "inner was: {inner}");
            }
            Ok(code) => assert_eq!(code, EXIT_OK),
            other => assert!(
                matches!(other, Err(FactorError::Io(_)) | Ok(_)),
                "unexpected result: {other:?}"
            ),
        }
    }
    assert!(io_failures > 0, "expected at least one io failure");
}

#[test]
fn cmd_continue_omits_untracked_section_when_remaining_output_has_no_untracked_files() {
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
    let messages = test_messages();
    let runner = continue_runner_with_remaining_output(repo, &original, "file.txt | 1 +\n", "");
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

    let code = cmd_continue_in(&ctx, &messages).or_abort("continue should succeed");
    assert_eq!(code, EXIT_OK);
    let stdout = io.stdout();
    assert!(
        stdout.contains("FACTOR: Split 1 committed."),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("STATE: Remaining changes are unstaged."),
        "stdout: {stdout}"
    );
    assert!(!stdout.contains("UNTRACKED:"), "stdout: {stdout}");
}

#[test]
fn cmd_continue_omits_remaining_hint_when_remaining_changes_are_only_untracked_files() {
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
    let messages = test_messages();
    let runner = continue_runner_with_remaining_output(repo, &original, "", "newfile.txt\n");
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

    let code = cmd_continue_in(&ctx, &messages).or_abort("continue should succeed");
    assert_eq!(code, EXIT_OK);
    let stdout = io.stdout();
    assert!(
        stdout.contains("FACTOR: Split 1 committed."),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("UNTRACKED:"), "stdout: {stdout}");
    assert!(stdout.contains("  newfile.txt"), "stdout: {stdout}");
    assert!(!stdout.contains("REMAINING:"), "stdout: {stdout}");
}

#[test]
fn cmd_finish_runner_failures_cover_command_error_paths() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n", "a".repeat(SHA_LEN)),
    )
    .or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");

    let original = "a".repeat(SHA_LEN);
    let commit_meta = "a\0b\0c\0d\0e\0f";
    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
    let messages = [message];

    let envs = [
        ("GIT_AUTHOR_NAME", "a"),
        ("GIT_AUTHOR_EMAIL", "b"),
        ("GIT_AUTHOR_DATE", "c"),
        ("GIT_COMMITTER_NAME", "d"),
        ("GIT_COMMITTER_EMAIL", "e"),
        ("GIT_COMMITTER_DATE", "f"),
    ];

    let base_runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 2)
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
        .with_output("git", &["write-tree"], repo, TREE_EXPECTED_NL)
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
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

    for fail_at in 3..=8 {
        fs::write(state_dir.join("split_count"), "0\n").or_abort("reset split_count");
        let runner = NthRunnerFailure::new(base_runner.clone(), fail_at);
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        let err = cmd_finish_in(&ctx, &messages).err_or_abort("expected forced runner failure");
        assert!(is_forced_runner_failure(&err), "err was: {err:?}");
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "finish failure coverage keeps the full integration transcript in one assertion-oriented test"
)]
fn cmd_finish_io_failures_cover_exec_gate_failure_output_paths() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n", "a".repeat(SHA_LEN)),
    )
    .or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "false\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");

    let original = "a".repeat(SHA_LEN);
    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
    let messages = [message];

    let base_runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 2)
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
        .with_output("git", &["write-tree"], repo, TREE_EXPECTED_NL)
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1)
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                original.as_str(),
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
        );
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let mut io_failures: u32 = 0;
    for fail_at in 1..=20 {
        fs::create_dir_all(&state_dir).or_abort("recreate factor dir");
        fs::write(
            state_dir.join("commits"),
            format!("{}\n", "a".repeat(SHA_LEN)),
        )
        .or_abort("rewrite commits");
        fs::write(state_dir.join("current_index"), "0\n").or_abort("rewrite current_index");
        fs::write(state_dir.join("split_count"), "0\n").or_abort("rewrite split_count");
        fs::write(state_dir.join("exec"), "false\n").or_abort("rewrite exec");
        fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("rewrite requires_rebase");
        fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL)
            .or_abort("rewrite expected_tree");

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
                io_failures = io_failures
                    .checked_add(1)
                    .or_abort("counter should not overflow");
                assert!(inner.to_string().contains("io fail"), "inner was: {inner}");
            }
            Ok(EXIT_OK) => {}
            other => assert!(
                matches!(other, Err(FactorError::Io(_)) | Ok(EXIT_OK)),
                "unexpected result: {other:?}"
            ),
        }
    }
    assert!(io_failures > 0, "expected at least one io failure");
}

#[test]
fn cmd_continue_precondition_failures_cover_internal_question_mark_paths() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
    let messages = NonEmpty::new(message);
    let base_runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status("git", &["diff", "--quiet", "--staged"], &[], false, repo, 1);
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let factor_dir_runner = NthRunnerFailure::new(base_runner.clone(), 2);
    let factor_dir_ctx = Ctx {
        runner: &factor_dir_runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let factor_dir_err =
        cmd_continue_in(&factor_dir_ctx, &messages).err_or_abort("expected factor_dir_in failure");
    assert!(
        matches!(&factor_dir_err, FactorError::GitDir(msg) if msg.contains("forced runner failure")),
        "err was: {factor_dir_err:?}"
    );

    let base_ctx = Ctx {
        runner: &base_runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    fs::remove_file(state_dir.join("exec")).or_abort("remove exec");
    let missing_exec_err =
        cmd_continue_in(&base_ctx, &messages).err_or_abort("expected missing exec state");
    assert!(
        matches!(&missing_exec_err, FactorError::StateRead(inner) if inner.kind() == io::ErrorKind::NotFound),
        "err was: {missing_exec_err:?}"
    );

    fs::write(state_dir.join("exec"), "true\n").or_abort("restore exec");
    let staged_diff_runner = NthRunnerFailure::new(base_runner.clone(), 3);
    let staged_diff_ctx = Ctx {
        runner: &staged_diff_runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let staged_diff_err = cmd_continue_in(&staged_diff_ctx, &messages)
        .err_or_abort("expected staged diff status failure");
    assert!(
        matches!(&staged_diff_err, FactorError::GitCommand(msg) if msg.contains("forced runner failure")),
        "err was: {staged_diff_err:?}"
    );
}

#[test]
fn cmd_finish_precondition_failures_cover_internal_question_mark_paths() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
    let messages = [message];
    let base_runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let factor_dir_runner = NthRunnerFailure::new(base_runner.clone(), 2);
    let factor_dir_ctx = Ctx {
        runner: &factor_dir_runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let factor_dir_err =
        cmd_finish_in(&factor_dir_ctx, &messages).err_or_abort("expected factor_dir_in failure");
    assert!(
        matches!(&factor_dir_err, FactorError::GitDir(msg) if msg.contains("forced runner failure")),
        "err was: {factor_dir_err:?}"
    );

    let missing_commits_runner = base_runner.clone();
    let missing_commits_ctx = Ctx {
        runner: &missing_commits_runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    fs::remove_file(state_dir.join("commits")).or_abort("remove commits");
    let missing_commits_err = cmd_finish_in(&missing_commits_ctx, &messages)
        .err_or_abort("expected missing commits state");
    assert!(
        matches!(&missing_commits_err, FactorError::StateRead(inner) if inner.kind() == io::ErrorKind::NotFound),
        "err was: {missing_commits_err:?}"
    );

    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("restore commits");
    fs::remove_file(state_dir.join("current_index")).or_abort("remove current_index");
    let missing_current_index_ctx = Ctx {
        runner: &base_runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let missing_current_index_err = cmd_finish_in(&missing_current_index_ctx, &messages)
        .err_or_abort("expected missing current_index state");
    assert!(
        matches!(&missing_current_index_err, FactorError::StateRead(inner) if inner.kind() == io::ErrorKind::NotFound),
        "err was: {missing_current_index_err:?}"
    );
}

#[test]
fn cmd_continue_errors_when_rebase_is_required_but_not_active() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n", "a".repeat(SHA_LEN)),
    )
    .or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "true\n").or_abort("write requires_rebase");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
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

    let err = cmd_continue_in(&ctx, &messages).err_or_abort("expected no rebase error");
    assert_eq!(err.to_string(), "git command failed: no rebase in progress");
}

#[test]
fn cmd_continue_errors_when_session_is_pending_start() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let original = "a".repeat(SHA_LEN);
    let state_dir = setup_factor_state(
        repo,
        &original,
        "0\n",
        Some("false\n"),
        Some(TREE_EXPECTED_NL),
    );
    fs::write(state_dir.join("phase"), "pending_start\n").or_abort("write phase");

    let messages = test_messages();
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

    let err = cmd_continue_in(&ctx, &messages).err_or_abort("expected pending-start usage error");
    assert_eq!(
        err.to_string(),
        "run 'git factor --continue' with no --message to begin splitting this commit"
    );
}

#[test]
fn cmd_continue_errors_when_repo_has_unstaged_changes_before_gate() {
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

    let messages = test_messages();
    let runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 2)
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
        .with_output_status(
            "git",
            &["status", "--porcelain=v1"],
            repo,
            0,
            " M file.txt\n",
            "",
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

    let err = cmd_continue_in(&ctx, &messages).err_or_abort("expected repo-state error");
    assert_eq!(
        err.to_string(),
        "git command failed: continue gate requires staged changes only; remove unstaged or untracked changes first\nSTATUS:\n M file.txt"
    );
}

#[test]
fn cmd_continue_pending_start_opens_split_session() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let original = "a".repeat(SHA_LEN);
    let state_dir = setup_factor_state(
        repo,
        &original,
        "0\n",
        Some("false\n"),
        Some(TREE_EXPECTED_NL),
    );
    fs::write(state_dir.join("phase"), "pending_start\n").or_abort("write phase");

    let runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 2)
        .with_output("git", &["status", "--porcelain=v1"], repo, "")
        .with_status(
            "git",
            &["reset", "--quiet", &format!("{original}^")],
            &[],
            false,
            repo,
            0,
        )
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", original.as_str()],
            repo,
            "subject\n",
        )
        .with_output(
            "git",
            &["rev-parse", "--short", original.as_str()],
            repo,
            "aaaaaaa\n",
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

    let code = cmd_continue_pending_start_in(&ctx).or_abort("pending start should open");
    assert_eq!(code, EXIT_OK);
    assert_eq!(
        fs::read_to_string(state_dir.join("phase")).or_abort("read phase"),
        "splitting\n"
    );
    let stdout = io.stdout();
    assert!(
        stdout.contains("FACTOR: Now splitting aaaaaaa."),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("ORIGINAL MESSAGE: subject"),
        "stdout: {stdout}"
    );
}

#[test]
fn cmd_finish_errors_when_rebase_is_required_but_not_active() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n", "a".repeat(SHA_LEN)),
    )
    .or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "true\n").or_abort("write requires_rebase");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
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

    let err = cmd_finish_in(&ctx, &messages).err_or_abort("expected no rebase error");
    assert_eq!(err.to_string(), "git command failed: no rebase in progress");
}

#[test]
fn cmd_continue_converged_tree_completes_session_without_remainder() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
    let messages = NonEmpty::new(message);

    let runner = continue_runner_with_commit(repo, &original, "").with_output(
        "git",
        &["rev-parse", "HEAD^{tree}"],
        repo,
        TREE_EXPECTED_NL,
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

    let code = cmd_continue_in(&ctx, &messages).or_abort("continue should succeed");
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
fn cmd_continue_rejects_invalid_head_tree_hash_after_commit() {
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
    let messages = test_messages();
    let runner = continue_runner_with_commit(repo, &original, "").with_output(
        "git",
        &["rev-parse", "HEAD^{tree}"],
        repo,
        "not-a-valid-hash\n",
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

    let err = cmd_continue_in(&ctx, &messages).err_or_abort("invalid head tree hash should fail");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("invalid tree hash")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_continue_propagates_invalid_restored_tree_hash_after_restore() {
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

    let messages = test_messages();
    let runner = continue_runner_with_commit(repo, &original, "")
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
        .with_output("git", &["write-tree"], repo, "not-a-valid-hash\n");
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

    let err = cmd_continue_in(&ctx, &messages)
        .err_or_abort("invalid restored tree hash should fail before reset");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("invalid tree hash")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_continue_propagates_io_error_when_completion_summary_write_fails() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");

    let messages = test_messages();
    let runner = continue_runner_with_commit(repo, &original, "").with_output(
        "git",
        &["rev-parse", "HEAD^{tree}"],
        repo,
        TREE_EXPECTED_NL,
    );
    let io = MatchingOutlnFailureIo {
        fail_on: "FACTOR: Complete. Final commit split into 1 commits.",
    };
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

    let err = cmd_continue_in(&ctx, &messages)
        .err_or_abort("completion summary write failure should bubble up");
    assert!(
        matches!(&err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_continue_completes_when_rebase_finishes_after_tree_converges() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let rebase_dir = git_dir.join("rebase-merge");
    fs::create_dir_all(&rebase_dir).or_abort("create rebase-merge");
    let original = "a".repeat(SHA_LEN);
    let state_dir = setup_factor_state(
        repo,
        &original,
        "0\n",
        Some("true\n"),
        Some(TREE_EXPECTED_NL),
    );
    let messages = test_messages();
    let base_runner = continue_runner_with_commit(repo, &original, "")
        .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, TREE_EXPECTED_NL)
        .with_status(
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        );
    let runner = RebaseContinueCompletesRunner::new(base_runner, rebase_dir);
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

    let code = cmd_continue_in(&ctx, &messages).or_abort("continue should succeed");
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
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    fs::write(repo.join("git-factor"), "").or_abort("write fake exe");

    let canon_repo = fs::canonicalize(repo).or_abort("canonicalize repo");
    let editor = canon_repo.join("git-sequence-editor");
    let editor_str = editor.to_str().or_abort("editor path is UTF-8");
    let seq_editor = format!(
        "{} {} {}",
        shell_quote(editor_str),
        shell_quote("--drop"),
        shell_quote("aaaaaaa")
    );

    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(state_dir.join("split_count"), "2\n").or_abort("write split_count");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("is_root"), "true\n").or_abort("write is_root");

    let root = "a".repeat(SHA_LEN);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["rev-list", "--max-parents=0", "HEAD"],
            repo,
            &format!("{root}\n"),
        )
        .with_output("git", &["ls-tree", &root], repo, "")
        .with_output("git", &["rev-parse", "--short", &root], repo, "aaaaaaa\n")
        .with_status(
            "git",
            &[
                "rebase",
                "--empty",
                "drop",
                "--interactive",
                "--no-autosquash",
                "--no-update-refs",
                "--quiet",
                "--root",
            ],
            &[
                ("GIT_EDITOR", "false"),
                ("GIT_SEQUENCE_EDITOR", &seq_editor),
            ],
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

    let outcome = Session::with_state(&ctx, StateDir::new(state_dir.clone()), vec![])
        .advance_to_next_commit()
        .or_abort("advance should succeed");
    assert!(
        matches!(&outcome, AdvanceOutcome::Completed { .. }),
        "expected Completed"
    );
    let AdvanceOutcome::Completed { final_split_count } = outcome;
    assert_eq!(final_split_count.get(), 2);
    assert!(io.stdout().is_empty(), "advance should produce no output");
    assert!(io.stderr().is_empty(), "stderr should be empty");
    assert!(
        !state_dir.exists(),
        "factor state dir should be removed after completion"
    );
}

#[test]
fn cmd_finish_rehydrates_remaining_changes_with_restore() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");

    let message = NonEmptyString::try_from("test: finish".to_owned()).or_abort("non-empty");
    let messages = [message];
    let commit_meta = "A U Thor\0author@example.com\x001700000000 +0000\0C O M Mitter\0committer@example.com\x001700000001 +0000";
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
        .with_output("git", &["write-tree"], repo, TREE_EXPECTED_NL)
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

    let code = cmd_finish_in(&ctx, &messages).or_abort("finish should succeed");
    assert_eq!(code, EXIT_OK);
}

#[test]
fn cmd_finish_reports_restore_failure() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
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

    let err = cmd_finish_in(&ctx, &messages).err_or_abort("expected restore failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git restore failed (exit ")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_propagates_restore_nonzero_exit() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
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

    let err = cmd_finish_in(&ctx, &messages).err_or_abort("expected restore failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git restore failed (exit ")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_errors_when_original_message_is_empty_without_messages() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");

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

    let err = cmd_finish_in(&ctx, &messages).err_or_abort("expected empty message error");
    assert_eq!(
        err.to_string(),
        "git command failed: original commit has empty message"
    );
}

#[test]
fn cmd_finish_reports_tree_mismatch_when_no_messages_are_provided() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");

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
        .with_output("git", &["write-tree"], repo, TREE_DIFFERENT_NL);
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

    let err = cmd_finish_in(&ctx, &messages).err_or_abort("expected tree mismatch");
    assert!(
        matches!(
            &err,
            FactorError::TreeHashMismatch { actual, expected }
                if actual.as_str() == TREE_DIFFERENT && expected.as_str() == TREE_EXPECTED
        ),
        "err was: {err:?}"
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
fn increment_split_count_reports_invalid_split_count_value() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create state dir");
    fs::write(state_dir.join("split_count"), "not-a-number\n").or_abort("write split_count");

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

    let err = Session::with_state(&ctx, StateDir::new(state_dir), vec![])
        .increment_split_count()
        .err_or_abort("invalid split_count should fail");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("split_count")),
        "err was: {err:?}"
    );
}

#[test]
fn advance_to_next_commit_reports_invalid_split_count_before_rebase_step() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(state_dir.join("split_count"), "not-a-number\n").or_abort("write split_count");

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

    let err = Session::with_state(&ctx, StateDir::new(state_dir), vec![])
        .advance_to_next_commit()
        .err_or_abort("expected invalid split_count");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("split_count")),
        "err was: {err:?}"
    );
}

#[test]
fn advance_to_next_commit_reports_invalid_requires_rebase_before_rebase_step() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("requires_rebase"), "not-bool\n").or_abort("write requires_rebase");

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

    let err = Session::with_state(&ctx, StateDir::new(state_dir), vec![])
        .advance_to_next_commit()
        .err_or_abort("expected invalid requires_rebase");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("requires_rebase")),
        "err was: {err:?}"
    );
}

#[test]
fn advance_to_next_commit_propagates_empty_root_cleanup_failure() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(state_dir.join("split_count"), "2\n").or_abort("write split_count");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("is_root"), "true\n").or_abort("write is_root");

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

    let err = Session::with_state(&ctx, StateDir::new(state_dir), vec![])
        .advance_to_next_commit()
        .err_or_abort("expected remove_empty_root failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("rev-list") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn rehydrate_pool_preserving_index_propagates_quit_status_io_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let base_runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, TREE_REHYDRATE_NL)
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(SHA_LEN),
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 0)
        .with_status("git", &["read-tree", TREE_REHYDRATE], &[], false, repo, 0);
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

    let commit = CommitSha::new("a".repeat(SHA_LEN)).or_abort("sha");
    let err = rehydrate_pool_preserving_index(&ctx, &commit).err_or_abort("expected quit io error");
    assert!(is_forced_runner_failure(&err), "err was: {err:?}");
}

#[test]
fn rehydrate_pool_preserving_index_propagates_read_tree_status_io_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let base_runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, TREE_REHYDRATE_NL)
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &"a".repeat(SHA_LEN),
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 0)
        .with_status("git", &["read-tree", TREE_REHYDRATE], &[], false, repo, 0);
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

    let commit = CommitSha::new("a".repeat(SHA_LEN)).or_abort("sha");
    let err =
        rehydrate_pool_preserving_index(&ctx, &commit).err_or_abort("expected read-tree io error");
    assert!(is_forced_runner_failure(&err), "err was: {err:?}");
}

#[test]
fn cmd_continue_propagates_expected_tree_fallback_lookup_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");

    let runner = continue_runner_with_commit(repo, &original, "").with_output(
        "git",
        &["rev-parse", "HEAD^{tree}"],
        repo,
        &format!("{}\n", "c".repeat(SHA_LEN)),
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
    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
    let messages = NonEmpty::new(message);

    let err =
        cmd_continue_in(&ctx, &messages).err_or_abort("expected expected-tree lookup failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("rev-parse") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_propagates_restore_status_error_when_runner_errors() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
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

    let err = cmd_finish_in(&ctx, &messages).err_or_abort("expected restore status error");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git restore failed (exit ")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_without_messages_propagates_original_message_lookup_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");

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

    let err =
        cmd_finish_in(&ctx, &messages).err_or_abort("expected original message lookup failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git show:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_propagates_advance_error_after_successful_commit() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");
    fs::write(state_dir.join("is_root"), "true\n").or_abort("write is_root");

    let commit_meta = "a\0b\0c\0d\0e\0f";
    let envs = [
        ("GIT_AUTHOR_NAME", "a"),
        ("GIT_AUTHOR_EMAIL", "b"),
        ("GIT_AUTHOR_DATE", "c"),
        ("GIT_COMMITTER_NAME", "d"),
        ("GIT_COMMITTER_EMAIL", "e"),
        ("GIT_COMMITTER_DATE", "f"),
    ];
    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
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
        .with_output("git", &["write-tree"], repo, TREE_EXPECTED_NL)
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

    let err = cmd_finish_in(&ctx, &messages).err_or_abort("expected advance failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("rev-list") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn advance_to_next_commit_errors_when_state_dir_removal_fails_on_completion() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(state_dir.join("split_count"), "3\n").or_abort("write split_count");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");

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
        fs: &FailingRemoveDirAllFs,
    };

    let err = Session::with_state(&ctx, StateDir::new(state_dir.clone()), vec![])
        .advance_to_next_commit()
        .err_or_abort("expected cleanup failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("failed to remove factor state path")),
        "err was: {err:?}"
    );
    assert!(io.stdout().is_empty(), "advance should produce no output");
    assert!(io.stderr().is_empty(), "stderr should be empty");
    assert!(
        state_dir.exists(),
        "state dir should remain when cleanup fails"
    );
}

#[test]
fn rehydrate_pool_preserving_index_reports_quit_failure_after_successful_cherry_pick() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let original = "a".repeat(SHA_LEN);

    let runner = ScriptedRunner::default()
        .with_output("git", &["write-tree"], repo, TREE_REHYDRATE_NL)
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &original,
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 5)
        .with_status("git", &["cherry-pick", "--abort"], &[], false, repo, 0);

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

    let commit = CommitSha::new(original).or_abort("sha");
    let err = rehydrate_pool_preserving_index(&ctx, &commit)
        .err_or_abort("expected cherry-pick quit error");

    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git cherry-pick --quit failed (exit")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn advance_to_next_commit_errors_when_cleanup_leaves_state_path_behind() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(state_dir.join("split_count"), "3\n").or_abort("write split_count");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");

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
        fs: &StickyStatePathFs,
    };

    let err = Session::with_state(&ctx, StateDir::new(state_dir), vec![])
        .advance_to_next_commit()
        .err_or_abort("expected persistent-state-path failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("still exists after cleanup")),
        "err was: {err:?}"
    );
}

#[test]
fn remove_state_path_returns_ok_when_state_path_is_missing() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = StateDir::new(repo.join(".git").join("factor"));
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

    remove_state_path(&ctx, &state_dir).or_abort("missing state path should be ignored");
    assert!(io.stdout().is_empty(), "stdout should be empty");
    assert!(io.stderr().is_empty(), "stderr should be empty");
}

#[test]
fn remove_state_path_removes_stray_file() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_path = repo.join(".git").join("factor");
    fs::create_dir_all(state_path.parent().or_abort("parent")).or_abort("create parent");
    fs::write(&state_path, "stale").or_abort("write stale file");
    let state_dir = StateDir::new(state_path.clone());
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

    remove_state_path(&ctx, &state_dir).or_abort("stray state file should be removed");
    assert!(!state_path.exists(), "state path should be removed");
    assert!(io.stdout().is_empty(), "stdout should be empty");
    assert!(io.stderr().is_empty(), "stderr should be empty");
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

#[test]
fn cmd_continue_propagates_requires_rebase_state_read_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::create_dir_all(state_dir.join("requires_rebase"))
        .or_abort("create invalid requires_rebase");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
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

    let err = cmd_continue_in(&ctx, &messages).err_or_abort("expected state read failure");
    assert!(
        matches!(&err, FactorError::StateRead(_)),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_continue_reports_current_commit_index_out_of_range() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "1\n").or_abort("write current index");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
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

    let err = cmd_continue_in(&ctx, &messages).err_or_abort("expected out-of-range index");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("out of range")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_continue_propagates_rehydrate_write_tree_error_when_exec_fails() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
    let messages = NonEmpty::new(message);
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
        .with_output("git", &["status", "--porcelain=v1"], repo, "M  file.txt\n")
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

    let err = cmd_continue_in(&ctx, &messages).err_or_abort("expected rehydrate error");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git write-tree") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_continue_propagates_exec_status_io_error_after_rehydrating_pool() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected tree");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
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
        .with_output("git", &["status", "--porcelain=v1"], repo, "M  file.txt\n")
        .with_output("git", &["write-tree"], repo, TREE_REHYDRATE_NL)
        .with_status(
            "git",
            &[
                "cherry-pick",
                "--no-commit",
                "--strategy-option",
                "theirs",
                &original,
            ],
            &[],
            false,
            repo,
            0,
        )
        .with_status("git", &["cherry-pick", "--quit"], &[], false, repo, 0)
        .with_status("git", &["read-tree", TREE_REHYDRATE], &[], false, repo, 0);
    let runner = base_runner;
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
        cmd_continue_in(&ctx, &messages).err_or_abort("expected exec status error after rehydrate");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("bash") && msg.contains("unexpected status call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_propagates_current_commit_lookup_error_after_session_load() {
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
    fs::write(
        repo.join(".git").join("factor").join("current_index"),
        "1\n",
    )
    .or_abort("write current_index");

    let messages = [NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty")];
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

    let err =
        cmd_finish_in(&ctx, &messages).err_or_abort("out-of-range current commit should fail");
    assert_eq!(
        err.to_string(),
        "git command failed: commit index 1 out of range (have 1 commits)"
    );
}

#[test]
fn cmd_continue_propagates_rehydrate_error_when_exec_status_io_error_rehydrate_fails() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected tree");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
    let messages = NonEmpty::new(message);
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
        .with_output("git", &["status", "--porcelain=v1"], repo, "M  file.txt\n");
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

    let err = cmd_continue_in(&ctx, &messages)
        .err_or_abort("expected rehydrate write-tree error after exec status I/O error");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git write-tree") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_continue_converged_path_propagates_advance_split_count_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("split_count"), "not-a-number\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");

    let message = NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty");
    let messages = NonEmpty::new(message);
    let runner = continue_runner_with_commit(repo, &original, "").with_output(
        "git",
        &["rev-parse", "HEAD^{tree}"],
        repo,
        TREE_EXPECTED_NL,
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

    let err = cmd_continue_in(&ctx, &messages).err_or_abort("expected split_count parse error");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("split_count")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_propagates_requires_rebase_state_read_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::create_dir_all(state_dir.join("requires_rebase"))
        .or_abort("create invalid requires_rebase");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");

    let messages = [NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty")];
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

    let err = cmd_finish_in(&ctx, &messages).err_or_abort("expected state read failure");
    assert!(
        matches!(&err, FactorError::StateRead(_)),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_propagates_expected_tree_lookup_error_when_missing_from_state() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");

    let messages = [NonEmptyString::try_from("test: message".to_owned()).or_abort("non-empty")];
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

    let err = cmd_finish_in(&ctx, &messages).err_or_abort("expected expected-tree lookup failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git rev-parse") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_finish_with_multiple_messages_hits_nonempty_rest_path() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let original = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{original}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
    fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("expected_tree"), TREE_EXPECTED_NL).or_abort("write expected_tree");

    let messages = [
        NonEmptyString::try_from("test: subject".to_owned()).or_abort("non-empty"),
        NonEmptyString::try_from("test: body".to_owned()).or_abort("non-empty"),
    ];
    let runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 2)
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

    let err = cmd_finish_in(&ctx, &messages).err_or_abort("expected write-tree lookup failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git write-tree") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_continue_converged_path_propagates_advance_split_count_read_error() {
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
    let messages = test_messages();
    let runner = continue_runner_with_commit(repo, &original, "").with_output(
        "git",
        &["rev-parse", "HEAD^{tree}"],
        repo,
        TREE_EXPECTED_NL,
    );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let fs = CorruptSplitCountWriteFs;
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &fs,
    };

    let err = cmd_continue_in(&ctx, &messages).err_or_abort("expected split_count parse error");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("split_count")),
        "err was: {err:?}"
    );
}

#[test]
fn print_session_started_propagates_print_hints_failure() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let base_runner = ScriptedRunner::default()
        .with_output("git", &["diff", "--stat"], repo, "file.txt | 1 +\n")
        .with_output(
            "git",
            &["ls-files", "--others", "--exclude-standard"],
            repo,
            "",
        );
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

    let err = print_session_started(
        &ctx,
        "FACTOR: Split session started for aaaaaaa.",
        "subject",
    )
    .err_or_abort("expected hint error");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("forced runner failure")),
        "err was: {err:?}"
    );
}
