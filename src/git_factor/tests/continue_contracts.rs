use super::start_contracts::RecordedCall;
use super::*;
use alloc::collections::BTreeMap;
use core::cell::Cell;

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor) enum ContinueCase {
    Complete,
    GateRejected,
    GateSpawn,
    NoStaged,
    PostGateDirty,
    PreGateDirty,
    Remainder,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor) enum ContinueFault {
    AfterStatus,
    BeforeStatus,
    Checkout,
    CheckoutIndex,
    Clean,
    Commit,
    DiffStat,
    HeadTree,
    Metadata,
    Reset,
    Restore,
    RestoredTree,
    SessionDir,
    StagedStatus,
    Untracked,
}

impl ContinueFault {
    fn command(self) -> &'static str {
        match self {
            Self::SessionDir | Self::HeadTree => "rev-parse",
            Self::StagedStatus | Self::DiffStat => "diff",
            Self::Checkout => "checkout",
            Self::Clean => "clean",
            Self::CheckoutIndex => "checkout-index",
            Self::BeforeStatus | Self::AfterStatus => "status",
            Self::Metadata => "show",
            Self::Commit => "commit",
            Self::Restore => "restore",
            Self::RestoredTree => "write-tree",
            Self::Reset => "reset",
            Self::Untracked => "ls-files",
        }
    }

    fn matches(self, output: bool, args: &[&str], ordinal: usize) -> bool {
        match self {
            Self::SessionDir => output && args == ["rev-parse", "--git-dir"] && ordinal == 1,
            Self::StagedStatus => !output && args == ["diff", "--quiet", "--staged"],
            Self::Checkout
            | Self::Clean
            | Self::CheckoutIndex
            | Self::Commit
            | Self::Restore
            | Self::Reset => !output && args.first().is_some_and(|arg| *arg == self.command()),
            Self::BeforeStatus => output && args == ["status", "--porcelain=v1"] && ordinal == 0,
            Self::AfterStatus => output && args == ["status", "--porcelain=v1"] && ordinal == 1,
            Self::HeadTree => output && args == ["rev-parse", HEAD_TREEISH],
            Self::DiffStat => output && args == ["diff", "--stat"],
            Self::Metadata | Self::RestoredTree | Self::Untracked => {
                output && args.first().is_some_and(|arg| *arg == self.command())
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor) enum ContinueState {
    Absent,
    IndexOutsideSpan,
    Pending,
    RebaseRequired,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor) enum ContinueFileFault {
    ReadCommits,
    ReadCount,
    ReadCountAfterCommit,
    ReadExec,
    ReadExpected,
    ReadIndex,
    ReadPhase,
    ReadRequires,
    WriteCount,
}

impl ContinueFileFault {
    fn name(self) -> &'static str {
        match self {
            Self::ReadCommits => "commits",
            Self::ReadPhase => "phase",
            Self::ReadRequires => "requires_rebase",
            Self::ReadIndex => "current_index",
            Self::ReadExec => "exec",
            Self::ReadCount | Self::ReadCountAfterCommit | Self::WriteCount => "split_count",
            Self::ReadExpected => "expected_tree",
        }
    }
}

struct ContinueFs {
    fault: Option<ContinueFileFault>,
    observed: Cell<bool>,
    reads: Cell<usize>,
    removals: RefCell<Vec<PathBuf>>,
}

impl Fs for ContinueFs {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        REAL_FS.canonicalize(path)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.create_dir_all(path)
    }

    fn exists(&self, path: &Path) -> bool {
        REAL_FS.exists(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        REAL_FS.is_dir(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        if let Some(fault) = self.fault
            && !matches!(fault, ContinueFileFault::WriteCount)
            && path.file_name().is_some_and(|name| name == fault.name())
        {
            let reads = self
                .reads
                .get()
                .checked_add(1)
                .or_abort("bounded continuation observation counter");
            self.reads.set(reads);
            if !matches!(fault, ContinueFileFault::ReadCountAfterCommit) || reads == 2 {
                self.observed.set(true);
                return Err(io::Error::other("continuation state read denied"));
            }
        }
        REAL_FS.read_to_string(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.removals.borrow_mut().push(path.to_path_buf());
        if path.is_relative() {
            return Err(io::Error::other("relative test removal refused"));
        }
        REAL_FS.remove_file(path)
    }

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        if self.fault.is_some_and(|fault| {
            matches!(fault, ContinueFileFault::WriteCount)
                && path.file_name().is_some_and(|name| name == "split_count")
        }) {
            self.observed.set(true);
            return Err(io::Error::other("continuation state write denied"));
        }
        REAL_FS.write_string(path, content)
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor) enum ContinueWrite {
    Complete,
    GateBanner,
    GateCode,
    GateCommand,
    GateNext,
    GateSeparator,
    GateUsage,
    Remainder,
}

impl ContinueWrite {
    fn ordinal(self) -> usize {
        match self {
            Self::Complete | Self::GateBanner | Self::Remainder => 1,
            Self::GateCommand => 2,
            Self::GateCode => 3,
            Self::GateSeparator => 4,
            Self::GateNext => 5,
            Self::GateUsage => 6,
        }
    }

    pub(in crate::git_factor) fn prefix(self) -> String {
        let lines = [
            "FACTOR: Exec gate failed. No commit created.\n",
            "EXEC: true\n",
            "CODE: 7\n",
            "\n",
            "NEXT: Adjust staged changes so the exec gate passes, then retry:\n",
        ];
        match self {
            Self::Complete | Self::Remainder => String::new(),
            Self::GateBanner
            | Self::GateCommand
            | Self::GateCode
            | Self::GateSeparator
            | Self::GateNext
            | Self::GateUsage => lines
                .get(
                    ..self
                        .ordinal()
                        .checked_sub(1)
                        .or_abort("nonzero write frontier"),
                )
                .or_abort("gate banner frontier")
                .concat(),
        }
    }
}

struct ContinueIo {
    fail: Option<ContinueWrite>,
    inner: TestIo,
    observed: Cell<bool>,
    writes: Cell<usize>,
}

impl ContinueIo {
    fn admit_write(&self) -> io::Result<()> {
        let ordinal = self
            .writes
            .get()
            .checked_add(1)
            .or_abort("bounded continuation observation counter");
        self.writes.set(ordinal);
        if self.fail.is_some_and(|fail| fail.ordinal() == ordinal) {
            self.observed.set(true);
            return Err(io::Error::other("continuation output denied"));
        }
        Ok(())
    }
}

impl Io for ContinueIo {
    fn err(&self, text: &str) -> io::Result<()> {
        self.inner.err(text)
    }

    fn errln(&self, line: &str) -> io::Result<()> {
        self.inner.errln(line)
    }

    fn out(&self, text: &str) -> io::Result<()> {
        self.admit_write()?;
        self.inner.out(text)
    }

    fn outln(&self, line: &str) -> io::Result<()> {
        self.admit_write()?;
        self.inner.outln(line)
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor) enum ContinueRecoveryFailure {
    GateRejected,
    GateSpawn,
    PostGateDirty,
}

struct ContinueRunner {
    calls: RefCell<Vec<RecordedCall>>,
    case: ContinueCase,
    fault: Option<ContinueFault>,
    gate_code: u8,
    git_dir_queries: Cell<usize>,
    head_tree: Option<String>,
    inner: ScriptedRunner,
    status_queries: Cell<usize>,
}

impl Runner for ContinueRunner {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        if bin == "git"
            && args == ["rev-parse", HEAD_TREEISH]
            && let Some(tree) = self.head_tree.as_ref()
        {
            return Ok(Output {
                status: exit_status(0),
                stdout: tree.as_bytes().to_vec(),
                stderr: Vec::new(),
            });
        }

        let ordinal = if bin == "git" && args == ["rev-parse", "--git-dir"] {
            let git_dir_ordinal = self.git_dir_queries.get();
            self.git_dir_queries.set(
                git_dir_ordinal
                    .checked_add(1)
                    .or_abort("bounded continuation observation counter"),
            );
            git_dir_ordinal
        } else {
            self.status_queries.get()
        };
        if bin == "git"
            && self
                .fault
                .is_some_and(|fault| fault.matches(true, args, ordinal))
        {
            return Err(io::Error::other("continuation query denied"));
        }
        if bin == "git" && args == ["status", "--porcelain=v1"] {
            let status_ordinal = self.status_queries.get();
            self.status_queries.set(
                status_ordinal
                    .checked_add(1)
                    .or_abort("bounded continuation observation counter"),
            );
            if matches!(
                (self.case, status_ordinal),
                (ContinueCase::PreGateDirty, 0) | (ContinueCase::PostGateDirty, 1)
            ) {
                return Ok(Output {
                    status: exit_status(0),
                    stdout: b" M file.txt\n".to_vec(),
                    stderr: Vec::new(),
                });
            }
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
        self.calls.borrow_mut().push(RecordedCall::Status {
            bin: bin.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            envs: envs
                .iter()
                .map(|&(key, value)| (key.to_owned(), value.to_owned()))
                .collect(),
            quiet,
            cwd: cwd.to_path_buf(),
        });
        if bin == "git"
            && self
                .fault
                .is_some_and(|fault| fault.matches(false, args, 0))
        {
            return Err(io::Error::other("continuation query denied"));
        }
        if bin == "bash" && args == ["-c", "true"] {
            match self.case {
                ContinueCase::GateRejected => {
                    return Ok(exit_status(i32::from(self.gate_code) << 8));
                }
                ContinueCase::GateSpawn => return Err(io::Error::other("gate spawn denied")),
                ContinueCase::Complete
                | ContinueCase::NoStaged
                | ContinueCase::PostGateDirty
                | ContinueCase::PreGateDirty
                | ContinueCase::Remainder => {}
            }
        }
        if matches!(self.case, ContinueCase::NoStaged)
            && bin == "git"
            && args == ["diff", "--quiet", "--staged"]
        {
            return Ok(exit_status(0));
        }
        self.inner.status(bin, args, envs, quiet, cwd)
    }
}

pub(in crate::git_factor) struct Continuation {
    case: ContinueCase,
    dir: TempDir,
    env: TestEnv,
    filesystem: ContinueFs,
    io: ContinueIo,
    original: String,
    runner: ContinueRunner,
    split_count: u8,
}

impl Continuation {
    pub(in crate::git_factor) fn ctx(&self) -> Ctx<'_> {
        Ctx {
            runner: &self.runner,
            cwd: self.dir.path().to_path_buf(),
            io: &self.io,
            env: &self.env,
            fs: &self.filesystem,
        }
    }

    pub(in crate::git_factor) fn deleted_requests(&self) -> Vec<PathBuf> {
        self.filesystem.removals.borrow().clone()
    }

    pub(in crate::git_factor) fn effect_requests(&self) -> Vec<Vec<String>> {
        self.runner
            .calls
            .borrow()
            .clone()
            .into_iter()
            .filter_map(|call| {
                if let RecordedCall::Status { bin, args, .. } = call {
                    let mut request = vec![bin];
                    request.extend(args);
                    Some(request)
                } else {
                    None
                }
            })
            .collect()
    }

    pub(in crate::git_factor) const fn expected_code(&self) -> i32 {
        match self.case {
            ContinueCase::Complete | ContinueCase::Remainder => EXIT_OK,
            ContinueCase::GateRejected => EXIT_TEMPFAIL,
            ContinueCase::NoStaged => EXIT_USAGE,
            ContinueCase::GateSpawn | ContinueCase::PostGateDirty | ContinueCase::PreGateDirty => {
                EXIT_SOFTWARE
            }
        }
    }

    fn expected_effect_boundary(&self) -> usize {
        match self.runner.fault {
            Some(ContinueFault::SessionDir) => 0,
            Some(ContinueFault::StagedStatus) => 1,
            Some(ContinueFault::Checkout) => 2,
            Some(ContinueFault::Clean) => 3,
            Some(ContinueFault::CheckoutIndex | ContinueFault::BeforeStatus) => 4,
            Some(ContinueFault::Metadata) => 5,
            Some(ContinueFault::Commit | ContinueFault::HeadTree) => 6,
            Some(ContinueFault::RestoredTree)
                if matches!(
                    self.case,
                    ContinueCase::GateRejected
                        | ContinueCase::GateSpawn
                        | ContinueCase::PostGateDirty
                ) =>
            {
                5
            }
            Some(ContinueFault::Restore | ContinueFault::RestoredTree) => 7,
            Some(
                ContinueFault::AfterStatus
                | ContinueFault::Reset
                | ContinueFault::DiffStat
                | ContinueFault::Untracked,
            ) => 8,
            None => match self.filesystem.fault {
                Some(
                    ContinueFileFault::ReadCommits
                    | ContinueFileFault::ReadPhase
                    | ContinueFileFault::ReadRequires
                    | ContinueFileFault::ReadIndex
                    | ContinueFileFault::ReadExec,
                ) => 0,
                Some(
                    ContinueFileFault::ReadCount
                    | ContinueFileFault::ReadCountAfterCommit
                    | ContinueFileFault::ReadExpected
                    | ContinueFileFault::WriteCount,
                ) => 6,
                None => match self.case {
                    ContinueCase::NoStaged => 1,
                    ContinueCase::PreGateDirty => 4,
                    ContinueCase::Complete => 6,
                    ContinueCase::GateRejected
                    | ContinueCase::GateSpawn
                    | ContinueCase::PostGateDirty
                    | ContinueCase::Remainder => 8,
                },
            },
        }
    }

    pub(in crate::git_factor) fn expected_effect_requests(&self) -> Vec<RecordedCall> {
        let cwd = self.dir.path();
        let request = |bin: &str, args: &[&str], envs: &[(&str, &str)]| RecordedCall::Status {
            bin: bin.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            envs: envs
                .iter()
                .map(|&(key, value)| (key.to_owned(), value.to_owned()))
                .collect(),
            quiet: false,
            cwd: cwd.to_path_buf(),
        };
        let mut calls = vec![
            request("git", &["diff", "--quiet", "--staged"], &[]),
            request("git", &["checkout", "--quiet", "--", "."], &[]),
            request("git", &["clean", "--force", "--quiet", "-d"], &[]),
            request(
                "git",
                &["checkout-index", "--all", "--force", "--quiet"],
                &[],
            ),
            request("bash", &["-c", "true"], &[]),
        ];
        if matches!(
            self.case,
            ContinueCase::GateRejected | ContinueCase::GateSpawn | ContinueCase::PostGateDirty
        ) || matches!(self.runner.fault, Some(ContinueFault::AfterStatus))
        {
            calls.extend([
                request(
                    "git",
                    &[
                        "cherry-pick",
                        "--no-commit",
                        "--strategy-option",
                        "theirs",
                        &self.original,
                    ],
                    &[],
                ),
                request("git", &["cherry-pick", "--quit"], &[]),
                request(
                    "git",
                    &[
                        "read-tree",
                        if matches!(self.runner.fault, Some(ContinueFault::AfterStatus)) {
                            TREE_EXPECTED
                        } else {
                            TREE_REHYDRATE
                        },
                    ],
                    &[],
                ),
            ]);
        } else {
            calls.extend([
                request(
                    "git",
                    &["commit", "--quiet", "--message", "test: message"],
                    &TEST_COMMIT_ENVS,
                ),
                request(
                    "git",
                    &[
                        "restore",
                        "--source",
                        &self.original,
                        "--staged",
                        "--worktree",
                        "--",
                        ".",
                    ],
                    &[],
                ),
                request("git", &["reset", "--quiet"], &[]),
            ]);
        }
        calls.truncate(self.expected_effect_boundary());
        calls
    }

    pub(in crate::git_factor) fn expected_journal(&self) -> BTreeMap<String, String> {
        if matches!(self.case, ContinueCase::Complete) {
            return BTreeMap::new();
        }
        let count = if matches!(self.case, ContinueCase::Remainder)
            && self.runner.fault.is_none_or(|fault| {
                matches!(
                    fault,
                    ContinueFault::HeadTree
                        | ContinueFault::Restore
                        | ContinueFault::RestoredTree
                        | ContinueFault::Reset
                        | ContinueFault::DiffStat
                        | ContinueFault::Untracked
                )
            }) {
            self.split_count
                .checked_add(1)
                .or_abort("admitted successful split count")
        } else {
            self.split_count
        };
        [
            ("commits", format!("{}\n", self.original)),
            ("current_index", "0\n".to_owned()),
            ("split_count", format!("{count}\n")),
            ("exec", "true\n".to_owned()),
            ("requires_rebase", "false\n".to_owned()),
            ("expected_tree", TREE_EXPECTED_NL.to_owned()),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
    }

    pub(in crate::git_factor) fn expected_stderr(&self) -> &'static str {
        match self.case {
            ContinueCase::Complete | ContinueCase::Remainder => "",
            ContinueCase::GateRejected => "exec gate failed: true (exit code 7)\n",
            ContinueCase::GateSpawn => "git command failed: bash -c: gate spawn denied\n",
            ContinueCase::NoStaged => concat!(
                "no staged changes to commit\n",
                "NEXT: stage exactly one atomic change, then rerun:\n",
                "  git factor --continue --message \"type: description\"\n"
            ),
            ContinueCase::PreGateDirty => concat!(
                "git command failed: continue gate requires ",
                "staged changes only; remove unstaged or untracked changes first\nSTATUS:\n M file.txt\n"
            ),
            ContinueCase::PostGateDirty => concat!(
                "git command failed: exec gate must not leave ",
                "unstaged or untracked changes behind\nSTATUS:\n M file.txt\n"
            ),
        }
    }

    pub(in crate::git_factor) fn expected_stdout(&self) -> String {
        match self.case {
            ContinueCase::Complete => format!(
                "FACTOR: Complete. Final commit split into {} commits.\n",
                self.split_count
                    .checked_add(1)
                    .or_abort("admitted successful split count")
            ),
            ContinueCase::GateRejected => concat!(
                "FACTOR: Exec gate failed. No commit created.\nEXEC: true\nCODE: 7\n\n",
                "NEXT: Adjust staged changes so the exec gate passes, then retry:\n",
                "  git factor --continue --message \"type: description\"\n",
            )
            .to_owned(),
            ContinueCase::Remainder => format!(
                concat!(
                    "FACTOR: Split {} committed.\nSTATE: Remaining changes are unstaged.\n",
                    "UNSTAGED:\n  file.txt | 1 +\nUNTRACKED:\n  new.txt\n\n",
                    "NEXT: Stage changes for the next commit, then run:\n",
                    "  git factor --continue --message \"type: description\"\n\n",
                    "HINTS:\n  - Find the ONE smallest addition nothing depends on\n",
                    "  - Target 15-30 lines (50 max)\n",
                    "  - Message: single concrete action, no \"and\"/\"or\"\n",
                    "  - Verify: git log --oneline | wc -l\n",
                    "  - NEVER use git commit. ONLY use git factor --continue.\n",
                    "  REMAINING: file.txt | 1 +\n  RECOVERY: git factor --abort\n",
                ),
                self.split_count
                    .checked_add(1)
                    .or_abort("admitted successful split count")
            ),
            ContinueCase::GateSpawn
            | ContinueCase::NoStaged
            | ContinueCase::PostGateDirty
            | ContinueCase::PreGateDirty => String::new(),
        }
    }

    pub(in crate::git_factor) fn fault_stderr(fault: ContinueFault) -> String {
        if matches!(fault, ContinueFault::SessionDir) {
            return "failed to determine git directory: continuation query denied\n".to_owned();
        }
        format!(
            "git command failed: git {}: continuation query denied\n",
            fault.command()
        )
    }

    pub(in crate::git_factor) fn file_fault_observed(&self) -> bool {
        self.filesystem.observed.get()
    }

    pub(in crate::git_factor) fn full_effect_requests(&self) -> Vec<RecordedCall> {
        self.runner.calls.borrow().clone()
    }

    pub(in crate::git_factor) fn journal(&self) -> BTreeMap<String, String> {
        let state = self.dir.path().join(".git/factor");
        [
            "commits",
            "current_index",
            "split_count",
            "exec",
            "requires_rebase",
            "expected_tree",
            "phase",
        ]
        .into_iter()
        .filter_map(|name| {
            let path = state.join(name);
            path.exists().then(|| {
                (
                    name.to_owned(),
                    fs::read_to_string(path).or_abort("journal field"),
                )
            })
        })
        .collect()
    }

    pub(in crate::git_factor) fn new(original: &str, split_count: u8, case: ContinueCase) -> Self {
        let dir = TempDir::new().or_abort("continuation repository");
        let repo = dir.path();
        setup_factor_state(
            repo,
            original,
            &format!("{split_count}\n"),
            Some("false\n"),
            Some(TREE_EXPECTED_NL),
        );
        fs::write(repo.join("user.txt"), "unrelated user bytes\n").or_abort("user file");
        fs::write(repo.join(".git/index"), "raw index witness").or_abort("index witness");
        fs::write(repo.join(".git/HEAD"), "ref: refs/heads/topic\n").or_abort("HEAD witness");
        let source_runner = match case {
            ContinueCase::Remainder => continue_runner_with_remaining_output(
                repo,
                original,
                "file.txt | 1 +\n",
                "new.txt\n",
            ),
            ContinueCase::Complete
            | ContinueCase::GateRejected
            | ContinueCase::GateSpawn
            | ContinueCase::NoStaged
            | ContinueCase::PostGateDirty
            | ContinueCase::PreGateDirty => continue_runner_with_commit(repo, original)
                .with_output("git", &["rev-parse", HEAD_TREEISH], repo, TREE_EXPECTED_NL),
        };
        let inner = source_runner
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
            .with_status("git", &["read-tree", TREE_REHYDRATE], &[], false, repo, 0);
        Self {
            case,
            env: TestEnv {
                cwd: repo.to_path_buf(),
            },
            io: ContinueIo {
                inner: TestIo::default(),
                fail: None,
                observed: Cell::new(false),
                writes: Cell::new(0),
            },
            filesystem: ContinueFs {
                fault: None,
                observed: Cell::new(false),
                reads: Cell::new(0),
                removals: RefCell::new(Vec::new()),
            },
            original: original.to_owned(),
            runner: ContinueRunner {
                calls: RefCell::new(Vec::new()),
                case,
                fault: None,
                head_tree: None,
                inner,
                status_queries: Cell::new(0),
                gate_code: 7,
                git_dir_queries: Cell::new(0),
            },
            split_count,
            dir,
        }
    }

    pub(in crate::git_factor) fn protected_bytes(&self) -> Vec<Vec<u8>> {
        ["user.txt", ".git/index", ".git/HEAD"]
            .into_iter()
            .map(|name| fs::read(self.dir.path().join(name)).or_abort("protected bytes"))
            .collect()
    }

    pub(in crate::git_factor) fn session_active(&self) -> bool {
        self.dir.path().join(".git/factor").exists()
    }

    pub(in crate::git_factor) fn stderr(&self) -> String {
        self.io.inner.stderr()
    }

    pub(in crate::git_factor) fn stdout(&self) -> String {
        self.io.inner.stdout()
    }

    pub(in crate::git_factor) fn with_active_rebase(original: &str, merge: bool) -> Self {
        let fixture = Self::new(original, 0, ContinueCase::NoStaged);
        let git_dir = fixture.dir.path().join(".git");
        fs::write(git_dir.join("factor/requires_rebase"), "true\n")
            .or_abort("required active native rebase");
        let native_dir = if merge {
            "rebase-merge"
        } else {
            "rebase-apply"
        };
        fs::create_dir_all(git_dir.join(native_dir)).or_abort("active native rebase metadata");
        fixture
    }

    pub(in crate::git_factor) fn with_counter_overflow(original: &str) -> Self {
        Self::new(original, u8::MAX, ContinueCase::Complete)
    }

    pub(in crate::git_factor) fn with_fault(original: &str, fault: ContinueFault) -> Self {
        let mut fixture = Self::new(original, 0, ContinueCase::Remainder);
        fixture.runner.fault = Some(fault);
        fixture
    }

    pub(in crate::git_factor) fn with_file_fault(original: &str, fault: ContinueFileFault) -> Self {
        let mut fixture = Self::new(original, 0, ContinueCase::Complete);
        fs::write(fixture.dir.path().join(".git/factor/phase"), "splitting\n")
            .or_abort("explicit splitting phase");
        fixture.filesystem.fault = Some(fault);
        if matches!(fault, ContinueFileFault::ReadExpected) {
            fixture.runner.inner = fixture.runner.inner.with_output_status(
                "git",
                &["rev-parse", &format!("{original}^{{tree}}")],
                fixture.dir.path(),
                128 << 8,
                "",
                "expected tree fallback denied",
            );
        }
        fixture
    }

    pub(in crate::git_factor) fn with_gate_code(original: &str, code: u8) -> Self {
        let mut fixture = Self::new(original, 0, ContinueCase::GateRejected);
        fixture.runner.gate_code = code;
        fixture
    }

    pub(in crate::git_factor) fn with_invalid_tree(
        original: &str,
        invalid: &str,
        expected: bool,
    ) -> Self {
        let mut fixture = Self::new(original, 0, ContinueCase::Complete);
        if expected {
            fs::write(
                fixture.dir.path().join(".git/factor/expected_tree"),
                format!("{invalid}\n"),
            )
            .or_abort("malformed expected tree");
        } else {
            fixture.runner.head_tree = Some(invalid.to_owned());
        }
        fixture
    }

    pub(in crate::git_factor) fn with_recovery_failure(
        original: &str,
        failure: ContinueRecoveryFailure,
    ) -> Self {
        let case = match failure {
            ContinueRecoveryFailure::GateRejected => ContinueCase::GateRejected,
            ContinueRecoveryFailure::GateSpawn => ContinueCase::GateSpawn,
            ContinueRecoveryFailure::PostGateDirty => ContinueCase::PostGateDirty,
        };
        let mut fixture = Self::new(original, 0, case);
        fixture.runner.fault = Some(ContinueFault::RestoredTree);
        fixture
    }

    pub(in crate::git_factor) fn with_state(original: &str, state: ContinueState) -> Self {
        let fixture = Self::new(original, 0, ContinueCase::Remainder);
        let dir = fixture.dir.path().join(".git/factor");
        match state {
            ContinueState::Absent => fs::remove_dir_all(dir).or_abort("absent journal"),
            ContinueState::Pending => {
                fs::write(dir.join("phase"), "pending_start\n").or_abort("pending journal");
            }
            ContinueState::RebaseRequired => {
                fs::write(dir.join("requires_rebase"), "true\n").or_abort("required native rebase");
            }
            ContinueState::IndexOutsideSpan => {
                fs::write(dir.join("current_index"), "1\n").or_abort("index outside selected span");
            }
        }
        fixture
    }

    pub(in crate::git_factor) fn with_write(original: &str, write: ContinueWrite) -> Self {
        let case = match write {
            ContinueWrite::Complete => ContinueCase::Complete,
            ContinueWrite::Remainder => ContinueCase::Remainder,
            ContinueWrite::GateBanner
            | ContinueWrite::GateCommand
            | ContinueWrite::GateCode
            | ContinueWrite::GateSeparator
            | ContinueWrite::GateNext
            | ContinueWrite::GateUsage => ContinueCase::GateRejected,
        };
        let mut fixture = Self::new(original, 0, case);
        fixture.io.fail = Some(write);
        fixture
    }

    pub(in crate::git_factor) fn write_observed(&self) -> bool {
        self.io.observed.get()
    }
}
