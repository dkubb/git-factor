#[path = "start_query_contracts.rs"]
pub(in crate::git_factor) mod query;

#[path = "start_replay_contracts.rs"]
pub(in crate::git_factor) mod replay;

use super::*;
use alloc::collections::BTreeMap;
use core::cell::Cell;
use core::num::NonZeroUsize;

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor) enum GateCase {
    Fail,
    FailOutputFailure(NonZeroUsize),
    Pass,
    PassOutputFailure(NonZeroUsize),
}

pub(in crate::git_factor) struct CapturedIo {
    fail_at: Option<NonZeroUsize>,
    stderr: RefCell<String>,
    stdout: RefCell<String>,
    writes: Cell<usize>,
}

impl CapturedIo {
    pub(in crate::git_factor) fn stderr(&self) -> String {
        self.stderr.borrow().clone()
    }

    pub(in crate::git_factor) fn stdout(&self) -> String {
        self.stdout.borrow().clone()
    }

    fn write(&self, stream: &RefCell<String>, text: &str) -> io::Result<()> {
        let writes = self
            .writes
            .get()
            .checked_add(1)
            .or_abort("bounded output count");
        self.writes.set(writes);
        if self.fail_at.is_some_and(|at| at.get() == writes) {
            return Err(io::Error::other("selected output write failed"));
        }
        stream.borrow_mut().push_str(text);
        Ok(())
    }
}

impl Io for CapturedIo {
    fn err(&self, text: &str) -> io::Result<()> {
        self.write(&self.stderr, text)
    }

    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(&format!("{line}\n"))
    }

    fn out(&self, text: &str) -> io::Result<()> {
        self.write(&self.stdout, text)
    }

    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(&format!("{line}\n"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::git_factor) enum RecordedCall {
    Output {
        args: Vec<String>,
        bin: String,
        cwd: PathBuf,
    },
    Status {
        args: Vec<String>,
        bin: String,
        cwd: PathBuf,
        envs: Vec<(String, String)>,
        quiet: bool,
    },
}

impl RecordedCall {
    fn output(bin: &str, args: &[&str], cwd: &Path) -> Self {
        Self::Output {
            bin: bin.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            cwd: cwd.to_path_buf(),
        }
    }

    fn status(bin: &str, args: &[&str], envs: &[(&str, &str)], quiet: bool, cwd: &Path) -> Self {
        Self::Status {
            bin: bin.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            envs: envs
                .iter()
                .map(|&(key, value)| (key.to_owned(), value.to_owned()))
                .collect(),
            quiet,
            cwd: cwd.to_path_buf(),
        }
    }
}

#[derive(Default)]
struct ObservedRunner {
    calls: RefCell<Vec<RecordedCall>>,
    expected: Vec<RecordedCall>,
    inner: ScriptedRunner,
    io_fault_at: Option<NonZeroUsize>,
}

impl ObservedRunner {
    #[expect(
        clippy::single_call_fn,
        reason = "pre-gate request script in production order, apart from expected-output literals"
    )]
    fn admission(
        repo: &Path,
        shas: &NonEmpty<CommitSha>,
        root: bool,
        pass: bool,
        stdout: &str,
        stderr: &str,
    ) -> Self {
        let first = shas.first().as_str();
        let tip = shas.last().as_str();
        let mut runner = Self::default().with_output(
            "git",
            &["rev-parse", "--verify", "HEAD"],
            repo,
            &format!("{tip}\n"),
        );
        for sha in shas {
            runner = runner
                .with_status(
                    "git",
                    &["merge-base", "--is-ancestor", sha.as_str(), "HEAD"],
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
                    1 << 8,
                );
        }
        runner = runner
            .with_status(
                "git",
                &["rev-parse", "--quiet", "--verify", &format!("{first}^")],
                &[],
                true,
                repo,
                if root { 1 << 8 } else { 0 },
            )
            .with_output("git", &["rev-parse", "--short", tip], repo, "abcdef0\n")
            .with_output(
                "git",
                &["show", "--format=%B", "--no-patch", tip],
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
            .with_output_status(
                "bash",
                &["-c", "true"],
                repo,
                if pass { 0 } else { 7 << 8 },
                stdout,
                stderr,
            );
        runner
    }

    fn calls(&self) -> Vec<RecordedCall> {
        self.calls.borrow().clone()
    }

    fn expectations(&self) -> Vec<RecordedCall> {
        self.expected.clone()
    }

    fn opened(
        self,
        repo: &Path,
        first: &str,
        root: bool,
        gate_writes: usize,
        fail_at: Option<NonZeroUsize>,
    ) -> Self {
        let mut runner = self;
        runner = runner
            .with_output("git", &["status", "--porcelain=v1"], repo, "")
            .with_output("git", &["rev-parse", "HEAD^{tree}"], repo, TREE_EXPECTED_NL);
        let reset_target = if root {
            runner = runner.with_output(
                "git",
                &[
                    "commit-tree",
                    "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
                    "-m",
                    "empty",
                ],
                repo,
                TREE_DIFFERENT_NL,
            );
            TREE_DIFFERENT.to_owned()
        } else {
            format!("{first}^")
        };
        runner = runner
            .with_status(
                "git",
                &["reset", "--quiet", &reset_target],
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
            );
        // Nine session-guide writes precede the hint's repository lookup.
        let before_hints = gate_writes.checked_add(9).or_abort("bounded guide writes");
        if fail_at.is_none_or(|at| at.get() > before_hints) {
            runner = runner.with_output(
                "git",
                &["rev-parse", "--show-toplevel"],
                repo,
                &format!("{}\n", repo.display()),
            );
        }
        runner
    }

    fn remaining_keys(&self) -> Vec<String> {
        let outputs = self.inner.outputs.borrow();
        let statuses = self.inner.statuses.borrow();
        let mut keys = outputs
            .iter()
            .filter(|&(_, queue)| !queue.is_empty())
            .map(|(key, _)| format!("output {key}"))
            .chain(
                statuses
                    .iter()
                    .filter(|&(_, queue)| !queue.is_empty())
                    .map(|(key, _)| format!("status {key}")),
            )
            .collect::<Vec<_>>();
        keys.sort();
        keys
    }

    fn with_output(self, bin: &str, args: &[&str], cwd: &Path, stdout: &str) -> Self {
        self.with_output_status(bin, args, cwd, 0, stdout, "")
    }

    fn with_output_status(
        mut self,
        bin: &str,
        args: &[&str],
        cwd: &Path,
        code: i32,
        stdout: &str,
        stderr: &str,
    ) -> Self {
        self.expected.push(RecordedCall::output(bin, args, cwd));
        self.inner = self
            .inner
            .with_output_status(bin, args, cwd, code, stdout, stderr);
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
        self.expected
            .push(RecordedCall::status(bin, args, envs, quiet, cwd));
        self.inner = self.inner.with_status(bin, args, envs, quiet, cwd, code);
        self
    }
}

impl Runner for ObservedRunner {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        self.calls
            .borrow_mut()
            .push(RecordedCall::output(bin, args, cwd));
        if self
            .io_fault_at
            .is_some_and(|at| at.get() == self.calls.borrow().len())
        {
            return Err(io::Error::other("selected query IO failure"));
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
        self.calls
            .borrow_mut()
            .push(RecordedCall::status(bin, args, envs, quiet, cwd));
        if self
            .io_fault_at
            .is_some_and(|at| at.get() == self.calls.borrow().len())
        {
            return Err(io::Error::other("selected query IO failure"));
        }
        self.inner.status(bin, args, envs, quiet, cwd)
    }
}

// Net contents/layout inside the owned tempdir, excluding factor state; Git has a separate ledger.
#[derive(Debug, Eq, PartialEq)]
pub(in crate::git_factor) enum DirectEntry {
    Directory,
    File(Vec<u8>),
}

#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "sibling canonical tests read only deliberately shared fixture facts"
)]
pub(in crate::git_factor) struct DirectStart {
    _dir: TempDir,
    pub(in crate::git_factor) direct_files_before: BTreeMap<PathBuf, DirectEntry>,
    env: TestEnv,
    pub(in crate::git_factor) exec: NonEmpty<NonEmptyString>,
    pub(in crate::git_factor) expected_calls: Vec<RecordedCall>,
    pub(in crate::git_factor) expected_journal: Option<BTreeMap<OsString, String>>,
    pub(in crate::git_factor) expected_result: Result<i32, String>,
    pub(in crate::git_factor) expected_stderr: String,
    pub(in crate::git_factor) expected_stdout: String,
    pub(in crate::git_factor) io: CapturedIo,
    runner: ObservedRunner,
    pub(in crate::git_factor) selected: NonEmpty<CommitSha>,
    pub(in crate::git_factor) state: StateDir,
}

impl DirectStart {
    fn banner(commit_count: usize, pass: bool) -> Vec<String> {
        let started = if commit_count == 1 {
            "FACTOR: Split session started for abcdef0.".to_owned()
        } else {
            format!("FACTOR: Split session started for {commit_count} commits (tip: abcdef0).")
        };
        if pass {
            vec![
                started,
                "ORIGINAL MESSAGE: subject".to_owned(),
                "UNSTAGED:".to_owned(),
                String::new(),
                "NEXT: Stage changes for the first atomic commit, then run:".to_owned(),
                "  git factor --continue --message \"type: description\"".to_owned(),
                String::new(),
                concat!(
                    "Run git factor -h for command help or git-factor --help ",
                    "for the full workflow guide."
                )
                .to_owned(),
                String::new(),
                "HINTS:".to_owned(),
                "  - Find the ONE smallest addition nothing depends on".to_owned(),
                "  - Target 15-30 lines (50 max)".to_owned(),
                "  - Message: single concrete action, no \"and\"/\"or\"".to_owned(),
                "  - Verify: git log --oneline | wc -l".to_owned(),
                "  - NEVER use git commit. ONLY use git factor --continue.".to_owned(),
                "  RECOVERY: git factor --abort".to_owned(),
            ]
        } else {
            vec![
                "FACTOR: Start gate failed.".to_owned(),
                "EXEC: true".to_owned(),
                "CODE: 7".to_owned(),
                String::new(),
                "NEXT: Fix the current commit, amend it, then rerun git factor.".to_owned(),
            ]
        }
    }

    pub(in crate::git_factor) fn ctx(&self) -> Ctx<'_> {
        Ctx {
            runner: &self.runner,
            cwd: self.env.cwd.clone(),
            io: &self.io,
            env: &self.env,
            fs: &REAL_FS,
        }
    }

    fn direct_files(root: &Path, state: &Path) -> BTreeMap<PathBuf, DirectEntry> {
        let mut files = BTreeMap::new();
        for entry in fs::read_dir(root).or_abort("direct filesystem snapshot") {
            let path = entry.or_abort("snapshot entry").path();
            if path == state {
                continue;
            }
            if path.is_dir() {
                files.insert(path.clone(), DirectEntry::Directory);
                files.extend(Self::direct_files(&path, state));
            } else {
                files.insert(
                    path.clone(),
                    DirectEntry::File(fs::read(path).or_abort("snapshot file")),
                );
            }
        }
        files
    }

    pub(in crate::git_factor) fn direct_files_after(&self) -> BTreeMap<PathBuf, DirectEntry> {
        Self::direct_files(&self.env.cwd, self.state.as_path())
    }

    fn journal(shas: &NonEmpty<CommitSha>, root: bool) -> BTreeMap<OsString, String> {
        let tip = shas.last().as_str();
        [
            (
                "commits",
                format!(
                    "{}\n",
                    shas.iter()
                        .map(CommitSha::as_str)
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
            ),
            ("current_index", format!("{}\n", shas.tail.len())),
            ("exec", "true\n".to_owned()),
            ("phase", "splitting\n".to_owned()),
            ("split_count", "0\n".to_owned()),
            ("requires_rebase", "false\n".to_owned()),
            ("started_rebase", "false\n".to_owned()),
            ("start_head", format!("{tip}\n")),
            ("is_root", format!("{root}\n")),
            ("expected_tree", TREE_EXPECTED_NL.to_owned()),
        ]
        .into_iter()
        .map(|(name, text)| (OsString::from(name), text))
        .collect()
    }

    pub(in crate::git_factor) fn new(
        shas: &NonEmpty<CommitSha>,
        root: bool,
        case: GateCase,
        stdout: &str,
        stderr: &str,
    ) -> Self {
        let dir = TempDir::new().or_abort("tempdir");
        let repo = dir.path();
        // The journal parent already exists, so creating factor state cannot change its layout.
        fs::create_dir_all(repo.join(".git")).or_abort("state parent fixture");
        let first = shas.first().as_str();
        let pass = matches!(case, GateCase::Pass | GateCase::PassOutputFailure(_));
        let fail_at = match case {
            GateCase::PassOutputFailure(at) | GateCase::FailOutputFailure(at) => Some(at),
            GateCase::Pass | GateCase::Fail => None,
        };
        let admission = ObservedRunner::admission(repo, shas, root, pass, stdout, stderr);
        let gate_writes = [stdout, stderr]
            .into_iter()
            .filter(|text| !text.is_empty())
            .count();
        let expected_state_exists = pass && fail_at.is_none_or(|at| at.get() > gate_writes);
        let runner = if expected_state_exists {
            admission.opened(repo, first, root, gate_writes, fail_at)
        } else {
            admission
        };
        let banner = Self::banner(shas.len(), pass);
        let total_writes = gate_writes
            .checked_add(banner.len())
            .or_abort("bounded fixture writes");
        let (expected_stdout, expected_stderr) = Self::streams(stdout, stderr, banner, fail_at);
        let expected_result = if fail_at.is_some_and(|at| at.get() <= total_writes) {
            Err("failed to write output: selected output write failed".to_owned())
        } else if pass {
            Ok(EXIT_OK)
        } else {
            Err("exec gate failed: true (exit code 7)".to_owned())
        };
        let expected_journal = expected_state_exists.then(|| Self::journal(shas, root));
        let expected_calls = runner.expectations();
        fs::write(repo.join("direct-write-sentinel"), b"sentinel\n").or_abort("existing sentinel");
        let direct_files_before = Self::direct_files(repo, &repo.join(".git/factor"));
        Self {
            env: TestEnv {
                cwd: repo.to_path_buf(),
            },
            state: StateDir::new(repo.join(".git/factor")),
            selected: shas.clone(),
            exec: NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("exec")),
            io: CapturedIo {
                stdout: RefCell::new(String::new()),
                stderr: RefCell::new(String::new()),
                writes: Cell::new(0),
                fail_at,
            },
            _dir: dir,
            runner,
            expected_result,
            direct_files_before,
            expected_stdout,
            expected_stderr,
            expected_journal,
            expected_calls,
        }
    }

    pub(in crate::git_factor) fn observed_calls(&self) -> Vec<RecordedCall> {
        self.runner.calls()
    }

    pub(in crate::git_factor) fn observed_journal(&self) -> Option<BTreeMap<OsString, String>> {
        let entries = match fs::read_dir(self.state.as_path()) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
            other => other.or_abort("journal observation"),
        };
        Some(
            entries
                .map(|entry| {
                    let file = entry.or_abort("journal entry");
                    (
                        file.file_name(),
                        fs::read_to_string(file.path()).or_abort("journal content"),
                    )
                })
                .collect(),
        )
    }

    pub(in crate::git_factor) fn remaining_keys(&self) -> Vec<String> {
        self.runner.remaining_keys()
    }

    pub(in crate::git_factor) fn single_commit_two_stream_fault_end(pass: bool) -> NonZeroUsize {
        NonZeroUsize::new(
            Self::banner(1, pass)
                .len()
                .checked_add(2)
                .or_abort("bounded output writes"),
        )
        .or_abort("nonempty output guide")
    }

    fn streams(
        stdout: &str,
        stderr: &str,
        banner: Vec<String>,
        fail_at: Option<NonZeroUsize>,
    ) -> (String, String) {
        let writes = [(false, stdout.to_owned()), (true, stderr.to_owned())]
            .into_iter()
            .filter(|write| !write.1.is_empty())
            .chain(banner.into_iter().map(|line| (false, format!("{line}\n"))))
            .collect::<Vec<_>>();
        // NonZero proves every fault has an exact predecessor; None completes all writes.
        let completed = fail_at.map_or(writes.len(), |at| {
            at.get()
                .checked_sub(1)
                .or_abort("positive write ordinal predecessor")
        });
        let expected_stdout = writes
            .iter()
            .take(completed)
            .filter(|&&(err, _)| !err)
            .map(|write| write.1.as_str())
            .collect();
        let expected_stderr = writes
            .iter()
            .take(completed)
            .filter(|&&(err, _)| err)
            .map(|write| write.1.as_str())
            .collect();
        (expected_stdout, expected_stderr)
    }
}
