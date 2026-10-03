//! Public Finish observations; scripted Git replies are requests, not native effects.
use super::start_contracts::RecordedCall;
use super::*;
use crate::exit_codes::EXIT_DATAERR;
use core::cell::Cell;
use core::fmt::Write as _;

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor) enum FinishCase {
    Absent,
    Empty,
    EmptyOriginal,
    Fallback,
    InvalidActual,
    InvalidCommits,
    InvalidCount,
    InvalidExpected,
    InvalidIndex,
    InvalidMetadata,
    InvalidPhase,
    InvalidRequires,
    Original,
    Output(usize),
    OutsideIndex,
    Overflow,
    Pending,
    Process(FinishStage, bool),
    Read(&'static str, usize),
    RebaseComplete,
    RebaseMissing,
    RebaseRetained,
    RemoveDenied,
    RemoveRetained,
    ReopenQuery,
    RootComplete,
    Supplied,
    UnequalTree,
    WriteCount,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::git_factor) enum FinishStage {
    Checkout,
    Clean,
    Commit,
    ExpectedTree,
    Metadata,
    OriginalMessage,
    Rebase,
    Restore,
    Root,
    StagedDiff,
    WriteTree,
}

struct FinishFs {
    case: FinishCase,
    observed: Cell<bool>,
    reads: Cell<usize>,
}

struct FinishIo {
    fail_at: Option<usize>,
    inner: TestIo,
    observed: Cell<bool>,
    writes: Cell<usize>,
}

struct FinishRunner {
    calls: RefCell<Vec<RecordedCall>>,
    case: FinishCase,
    directory_reads: Cell<usize>,
    inner: ScriptedRunner,
    observed: Cell<bool>,
}

impl FinishStage {
    fn command(self) -> &'static str {
        match self {
            Self::Checkout => "checkout",
            Self::Clean => "clean",
            Self::Restore => "restore",
            Self::OriginalMessage | Self::Metadata => "show",
            Self::ExpectedTree => "rev-parse",
            Self::WriteTree => "write-tree",
            Self::StagedDiff => "diff",
            Self::Commit => "commit",
            Self::Rebase => "rebase",
            Self::Root => "rev-list",
        }
    }

    fn matches(self, args: &[&str]) -> bool {
        match self {
            Self::OriginalMessage => args.get(1) == Some(&"--format=%B"),
            Self::Metadata => {
                args.get(1) == Some(&"--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI")
            }
            Self::ExpectedTree => {
                args.first() == Some(&"rev-parse")
                    && args.len() == 2
                    && args.get(1) != Some(&"--git-dir")
            }
            Self::Checkout
            | Self::Clean
            | Self::Restore
            | Self::WriteTree
            | Self::StagedDiff
            | Self::Commit
            | Self::Rebase
            | Self::Root => args.first() == Some(&self.command()),
        }
    }

    const fn output(self) -> bool {
        matches!(
            self,
            Self::OriginalMessage
                | Self::ExpectedTree
                | Self::WriteTree
                | Self::Metadata
                | Self::Root
        )
    }
}

impl Fs for FinishFs {
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
        if let FinishCase::Read(key, ordinal) = self.case
            && path.file_name().is_some_and(|name| name == key)
        {
            let read = self
                .reads
                .get()
                .checked_add(1)
                .or_abort("bounded selected state reads");
            self.reads.set(read);
            if read == ordinal {
                self.observed.set(true);
                return Err(io::Error::other("finish state read denied"));
            }
        }
        REAL_FS.read_to_string(path)
    }
    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        match self.case {
            FinishCase::RemoveDenied => {
                self.observed.set(true);
                Err(io::Error::other("finish state removal denied"))
            }
            FinishCase::RemoveRetained => {
                self.observed.set(true);
                Ok(())
            }
            FinishCase::Absent
            | FinishCase::Empty
            | FinishCase::EmptyOriginal
            | FinishCase::Fallback
            | FinishCase::InvalidActual
            | FinishCase::InvalidCommits
            | FinishCase::InvalidCount
            | FinishCase::InvalidExpected
            | FinishCase::InvalidIndex
            | FinishCase::InvalidMetadata
            | FinishCase::InvalidPhase
            | FinishCase::InvalidRequires
            | FinishCase::Original
            | FinishCase::Output(_)
            | FinishCase::OutsideIndex
            | FinishCase::Overflow
            | FinishCase::Pending
            | FinishCase::Process(..)
            | FinishCase::Read(..)
            | FinishCase::RebaseComplete
            | FinishCase::RebaseMissing
            | FinishCase::RebaseRetained
            | FinishCase::ReopenQuery
            | FinishCase::RootComplete
            | FinishCase::Supplied
            | FinishCase::UnequalTree
            | FinishCase::WriteCount => REAL_FS.remove_dir_all(path),
        }
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_file(path)
    }
    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        if matches!(self.case, FinishCase::WriteCount)
            && path.file_name().is_some_and(|name| name == "split_count")
        {
            self.observed.set(true);
            return Err(io::Error::other("finish state write denied"));
        }
        REAL_FS.write_string(path, content)
    }
}

impl Io for FinishIo {
    fn err(&self, text: &str) -> io::Result<()> {
        self.inner.err(text)
    }
    fn errln(&self, line: &str) -> io::Result<()> {
        self.inner.errln(line)
    }
    fn out(&self, text: &str) -> io::Result<()> {
        let write = self
            .writes
            .get()
            .checked_add(1)
            .or_abort("two output writes");
        self.writes.set(write);
        if self.fail_at == Some(write) {
            self.observed.set(true);
            return Err(io::Error::other("finish output denied"));
        }
        self.inner.out(text)
    }
    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }
}

impl Runner for FinishRunner {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        self.calls
            .borrow_mut()
            .push(recorded_output(bin, args, cwd));
        if args == ["rev-parse", "--git-dir"] {
            let read = self
                .directory_reads
                .get()
                .checked_add(1)
                .or_abort("bounded directory reads");
            self.directory_reads.set(read);
            if matches!(self.case, FinishCase::ReopenQuery) && read == 2 {
                self.observed.set(true);
                return Err(io::Error::other("finish directory refused"));
            }
        }
        if let FinishCase::Process(stage, spawn) = self.case
            && stage.output()
            && stage.matches(args)
            && !self.observed.get()
        {
            self.observed.set(true);
            return if spawn {
                Err(io::Error::other("finish process denied"))
            } else {
                Ok(Output {
                    status: exit_status(23 << 8),
                    stdout: Vec::new(),
                    stderr: b"finish query refused".to_vec(),
                })
            };
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
            .push(recorded_status(bin, args, envs, quiet, cwd));
        if let FinishCase::Process(stage, spawn) = self.case
            && !stage.output()
            && stage.matches(args)
            && !self.observed.get()
        {
            self.observed.set(true);
            return if spawn {
                Err(io::Error::other("finish process denied"))
            } else {
                Ok(exit_status(23 << 8))
            };
        }
        let result = self.inner.status(bin, args, envs, quiet, cwd);
        if args == ["rebase", "--continue"]
            && result.as_ref().is_ok_and(ExitStatus::success)
            && !matches!(self.case, FinishCase::RebaseRetained)
        {
            // An explicit simulated successful native effect; no native Git assurance claimed.
            fs::remove_dir_all(cwd.join(".git/rebase-merge"))
                .or_abort("simulate native completion");
        }
        result
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "commit frontier oracle stays distinct from admission and advance"
)]
fn append_commit_requests(
    requests: &mut Vec<RecordedCall>,
    repo: &Path,
    case: FinishCase,
    sha: &str,
    commit: &[&str],
) -> bool {
    if original_message(case) {
        requests.push(output_request(
            &["show", "--format=%B", "--no-patch", sha],
            repo,
        ));
        if matches!(
            case,
            FinishCase::EmptyOriginal | FinishCase::Process(FinishStage::OriginalMessage, _)
        ) {
            return false;
        }
    }
    requests.push(output_request(&["write-tree"], repo));
    if matches!(
        case,
        FinishCase::InvalidActual
            | FinishCase::UnequalTree
            | FinishCase::Process(FinishStage::WriteTree, _)
    ) {
        return false;
    }
    requests.push(status_request(
        &["diff", "--quiet", "--staged"],
        &[],
        false,
        repo,
    ));
    if matches!(case, FinishCase::Process(FinishStage::StagedDiff, true)) {
        return false;
    }
    requests.push(output_request(
        &[
            "show",
            "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
            "--no-patch",
            sha,
        ],
        repo,
    ));
    if matches!(
        case,
        FinishCase::InvalidMetadata | FinishCase::Process(FinishStage::Metadata, _)
    ) {
        return false;
    }
    requests.push(status_request(commit, &TEST_COMMIT_ENVS, false, repo));
    if matches!(
        case,
        FinishCase::Process(FinishStage::Commit, _)
            | FinishCase::InvalidCount
            | FinishCase::Overflow
            | FinishCase::Read("split_count" | "requires_rebase", _)
            | FinishCase::WriteCount
            | FinishCase::RemoveDenied
            | FinishCase::RemoveRetained
    ) {
        return false;
    }
    true
}

#[expect(
    clippy::single_call_fn,
    reason = "completion frontier oracle stays distinct from reconstruction and commit"
)]
fn append_completion_requests(
    requests: &mut Vec<RecordedCall>,
    repo: &Path,
    case: FinishCase,
    sha: &str,
) {
    if rebase(case) {
        requests.push(directory_request(repo));
        requests.push(status_request(
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
        ));
        if matches!(case, FinishCase::Process(FinishStage::Rebase, _)) {
            return;
        }
        requests.push(directory_request(repo));
    }
    if root(case) {
        requests.push(output_request(
            &["rev-list", "--max-parents=0", "HEAD"],
            repo,
        ));
        if matches!(case, FinishCase::Process(FinishStage::Root, _)) {
            return;
        }
        requests.push(output_request(&["ls-tree", sha], repo));
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "reconstruction replies remain distinct from original and rebase replies"
)]
fn arrange_reconstruction(
    repo: &Path,
    case: FinishCase,
    sha: &str,
    args: &[&str],
) -> ScriptedRunner {
    ScriptedRunner::default()
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
                sha,
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
            &["write-tree"],
            repo,
            match case {
                FinishCase::InvalidActual => "not-a-tree\n",
                FinishCase::UnequalTree => TREE_DIFFERENT_NL,
                FinishCase::Absent
                | FinishCase::Empty
                | FinishCase::EmptyOriginal
                | FinishCase::Fallback
                | FinishCase::InvalidCommits
                | FinishCase::InvalidCount
                | FinishCase::InvalidExpected
                | FinishCase::InvalidIndex
                | FinishCase::InvalidMetadata
                | FinishCase::InvalidPhase
                | FinishCase::InvalidRequires
                | FinishCase::Original
                | FinishCase::Output(_)
                | FinishCase::OutsideIndex
                | FinishCase::Overflow
                | FinishCase::Pending
                | FinishCase::Process(..)
                | FinishCase::Read(..)
                | FinishCase::RebaseComplete
                | FinishCase::RebaseMissing
                | FinishCase::RebaseRetained
                | FinishCase::RemoveDenied
                | FinishCase::RemoveRetained
                | FinishCase::ReopenQuery
                | FinishCase::RootComplete
                | FinishCase::Supplied
                | FinishCase::WriteCount => TREE_EXPECTED_NL,
            },
        )
        .with_status(
            "git",
            &["diff", "--quiet", "--staged"],
            &[],
            false,
            repo,
            if matches!(case, FinishCase::Empty) {
                0
            } else {
                1 << 8
            },
        )
        .with_output(
            "git",
            &[
                "show",
                "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                "--no-patch",
                sha,
            ],
            repo,
            if matches!(case, FinishCase::InvalidMetadata) {
                "truncated"
            } else {
                TEST_COMMIT_META
            },
        )
        .with_status("git", args, &TEST_COMMIT_ENVS, false, repo, 0)
}

#[expect(
    clippy::single_call_fn,
    reason = "optional original-message and completion replies form one arrangement boundary"
)]
fn arrange_runner(
    repo: &Path,
    case: FinishCase,
    sha: &str,
    args: &[&str],
    original: &str,
) -> ScriptedRunner {
    let mut runner = arrange_reconstruction(repo, case, sha, args);
    if original_message(case) {
        let original_output = if matches!(case, FinishCase::EmptyOriginal) {
            "\n".to_owned()
        } else {
            format!("{original}\n\n")
        };
        runner = runner.with_output(
            "git",
            &["show", "--format=%B", "--no-patch", sha],
            repo,
            &original_output,
        );
    }
    if fallback(case) {
        runner = runner.with_output(
            "git",
            &["rev-parse", &format!("{sha}^{{tree}}")],
            repo,
            TREE_EXPECTED_NL,
        );
    }
    if rebase(case) {
        runner = runner.with_status(
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        );
    }
    if root(case) {
        runner = runner
            .with_output("git", &["rev-list", "--max-parents=0", "HEAD"], repo, sha)
            .with_output("git", &["ls-tree", sha], repo, "nonempty original root\n");
    }
    runner
}

#[expect(
    clippy::single_call_fn,
    reason = "literal saved-state arrangement remains separate from the public Act"
)]
fn arrange_state(repo: &Path, case: FinishCase, sha: &str, count: u8) -> PathBuf {
    let state = repo.join(".git/factor");
    fs::create_dir_all(&state).or_abort("owned Finish journal");
    for (key, value) in [
        ("commits", format!("{sha}\n")),
        ("current_index", "0\n".to_owned()),
        ("phase", "splitting\n".to_owned()),
        ("split_count", format!("{count}\n")),
        ("requires_rebase", "false\n".to_owned()),
        ("expected_tree", TREE_EXPECTED_NL.to_owned()),
        ("is_root", "false\n".to_owned()),
        ("exec", "false\n".to_owned()),
    ] {
        fs::write(state.join(key), value).or_abort("literal saved field");
    }
    let override_field = match case {
        FinishCase::Pending => Some(("phase", "pending_start\n")),
        FinishCase::InvalidPhase => Some(("phase", "wrong\n")),
        FinishCase::InvalidCommits => Some(("commits", "not-a-sha\n")),
        FinishCase::InvalidIndex => Some(("current_index", "wrong\n")),
        FinishCase::OutsideIndex => Some(("current_index", "1\n")),
        FinishCase::InvalidRequires => Some(("requires_rebase", "wrong\n")),
        FinishCase::InvalidExpected => Some(("expected_tree", "not-a-tree\n")),
        FinishCase::InvalidCount => Some(("split_count", "wrong\n")),
        FinishCase::Overflow => Some(("split_count", "255\n")),
        FinishCase::RebaseMissing => Some(("requires_rebase", "true\n")),
        FinishCase::Absent
        | FinishCase::Empty
        | FinishCase::EmptyOriginal
        | FinishCase::Fallback
        | FinishCase::InvalidActual
        | FinishCase::InvalidMetadata
        | FinishCase::Original
        | FinishCase::Output(_)
        | FinishCase::Process(..)
        | FinishCase::Read(..)
        | FinishCase::RebaseComplete
        | FinishCase::RebaseRetained
        | FinishCase::RemoveDenied
        | FinishCase::RemoveRetained
        | FinishCase::ReopenQuery
        | FinishCase::RootComplete
        | FinishCase::Supplied
        | FinishCase::UnequalTree
        | FinishCase::WriteCount => None,
    };
    if let Some((key, value)) = override_field {
        fs::write(state.join(key), value).or_abort("selected saved reply");
    }
    if fallback(case) {
        fs::remove_file(state.join("expected_tree")).or_abort("legitimate legacy fallback");
    }
    if rebase(case) {
        fs::write(state.join("requires_rebase"), "true\n").or_abort("required native continuation");
        fs::create_dir_all(repo.join(".git/rebase-merge"))
            .or_abort("owned simulated native metadata");
    }
    if root(case) {
        fs::write(state.join("is_root"), "true\n").or_abort("root cleanup ingress");
    }
    if matches!(case, FinishCase::Absent) {
        fs::remove_dir_all(&state).or_abort("inactive ingress");
    }
    state
}

#[expect(
    clippy::single_call_fn,
    reason = "independent commit argument oracle stays separate from reply arrangement"
)]
fn commit_arguments<'message>(
    case: FinishCase,
    subject: &'message str,
    body: &'message str,
    original: &'message str,
) -> Vec<&'message str> {
    let mut args = vec!["commit", "--quiet"];
    if matches!(case, FinishCase::Empty) {
        args.push("--allow-empty");
    }
    if original_message(case) {
        args.extend(["--message", original.trim()]);
    } else {
        args.extend(["--message", subject, "--message", body]);
    }
    args
}

fn count_was_written(case: FinishCase) -> bool {
    matches!(
        case,
        FinishCase::Supplied
            | FinishCase::Original
            | FinishCase::Empty
            | FinishCase::Fallback
            | FinishCase::Read("expected_tree", _)
            | FinishCase::RootComplete
            | FinishCase::RebaseComplete
            | FinishCase::RebaseRetained
            | FinishCase::Read("split_count" | "requires_rebase", 2)
            | FinishCase::RemoveDenied
            | FinishCase::RemoveRetained
            | FinishCase::Output(_)
            | FinishCase::Process(FinishStage::Rebase | FinishStage::Root, _)
            | FinishCase::Process(FinishStage::StagedDiff, false)
    )
}

fn directory_request(repo: &Path) -> RecordedCall {
    output_request(&["rev-parse", "--git-dir"], repo)
}

#[expect(
    clippy::single_call_fn,
    reason = "literal diagnostic-body oracle stays separate from timestamp validation"
)]
fn expected_diagnostic(
    case: FinishCase,
    repo: &Path,
    sha: &str,
    previous: u8,
    argv: &[OsString],
    failure: Option<&(i32, String)>,
) -> String {
    let final_count = previous.checked_add(1);
    let argv_text = argv
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let error = &failure.or_abort("failure oracle").1;
    let mut expected_log = format!("argv={argv_text}\ncwd={}\nerror={error}\n", repo.display());
    match case {
        FinishCase::Read(_, _) => {
            expected_log.push_str("source_0=finish state read denied\n");
        }
        FinishCase::WriteCount => {
            expected_log.push_str("source_0=finish state write denied\n");
        }
        FinishCase::Output(_) => expected_log.push_str("source_0=finish output denied\n"),
        FinishCase::Absent
        | FinishCase::Empty
        | FinishCase::EmptyOriginal
        | FinishCase::Fallback
        | FinishCase::InvalidActual
        | FinishCase::InvalidCommits
        | FinishCase::InvalidCount
        | FinishCase::InvalidExpected
        | FinishCase::InvalidIndex
        | FinishCase::InvalidMetadata
        | FinishCase::InvalidPhase
        | FinishCase::InvalidRequires
        | FinishCase::Original
        | FinishCase::OutsideIndex
        | FinishCase::Overflow
        | FinishCase::Pending
        | FinishCase::Process(..)
        | FinishCase::RebaseComplete
        | FinishCase::RebaseMissing
        | FinishCase::RebaseRetained
        | FinishCase::RemoveDenied
        | FinishCase::RemoveRetained
        | FinishCase::ReopenQuery
        | FinishCase::RootComplete
        | FinishCase::Supplied
        | FinishCase::UnequalTree => {}
    }
    writeln!(expected_log, "git_dir={}", repo.join(".git").display())
        .or_abort("write literal diagnostic into String");
    if !matches!(case, FinishCase::InvalidIndex | FinishCase::OutsideIndex) {
        writeln!(expected_log, "factor_current_commit={sha}")
            .or_abort("write literal diagnostic into String");
    }
    if !matches!(case, FinishCase::InvalidIndex) {
        expected_log.push_str(if matches!(case, FinishCase::OutsideIndex) {
            "factor_current_index=1\n"
        } else {
            "factor_current_index=0\n"
        });
    }
    if !matches!(case, FinishCase::InvalidCount) {
        let saved_count = if matches!(case, FinishCase::Overflow) {
            u8::MAX
        } else if count_was_written(case) {
            final_count.or_abort("written count")
        } else {
            previous
        };
        writeln!(expected_log, "factor_split_count={saved_count}")
            .or_abort("write literal diagnostic into String");
    }
    if !matches!(case, FinishCase::InvalidRequires) {
        expected_log.push_str(
            if rebase(case) || matches!(case, FinishCase::RebaseMissing) {
                "factor_requires_rebase=true\n"
            } else {
                "factor_requires_rebase=false\n"
            },
        );
    }
    if !fallback(case) {
        let tree = if matches!(case, FinishCase::InvalidExpected) {
            "not-a-tree"
        } else {
            TREE_EXPECTED
        };
        writeln!(expected_log, "factor_expected_tree={tree}")
            .or_abort("write literal diagnostic into String");
    }
    if rebase(case) {
        expected_log.push_str("rebase_state=rebase-merge\n");
    }
    expected_log.push_str("staged_paths=\nunstaged_paths=\nuntracked_paths=\n");
    expected_log
}

#[expect(
    clippy::single_call_fn,
    reason = "literal public diagnostic oracle remains separate from actor behavior"
)]
fn expected_error(case: FinishCase, repo: &Path, sha: &str) -> Option<(i32, String)> {
    let state = repo.join(".git/factor");
    let ordinary = |text: &str| Some((EXIT_SOFTWARE, format!("git command failed: {text}")));
    match case {
        FinishCase::Supplied
        | FinishCase::Original
        | FinishCase::Empty
        | FinishCase::Fallback
        | FinishCase::RebaseComplete
        | FinishCase::RootComplete
        | FinishCase::Read("expected_tree", _)
        | FinishCase::Process(FinishStage::StagedDiff, false) => None,
        FinishCase::Absent => Some((EXIT_USAGE, "no active factor session".to_owned())),
        FinishCase::ReopenQuery => Some((
            EXIT_DATAERR,
            "failed to determine git directory: finish directory refused".to_owned(),
        )),
        FinishCase::Pending => Some((
            EXIT_USAGE,
            "run 'git factor --continue' with no --message to begin splitting this commit"
                .to_owned(),
        )),
        FinishCase::InvalidPhase => ordinary("corrupted state file 'phase': invalid value 'wrong'"),
        FinishCase::InvalidCommits => Some((EXIT_DATAERR, "invalid commit: not-a-sha".to_owned())),
        FinishCase::InvalidIndex => {
            ordinary("corrupted state file 'current_index': invalid value 'wrong'")
        }
        FinishCase::OutsideIndex => ordinary("commit index 1 out of range (have 1 commits)"),
        FinishCase::InvalidRequires => {
            ordinary("corrupted state file 'requires_rebase': invalid value 'wrong'")
        }
        FinishCase::RebaseMissing => ordinary("no rebase in progress"),
        FinishCase::InvalidExpected | FinishCase::InvalidActual => {
            ordinary("invalid tree hash: 'not-a-tree'")
        }
        FinishCase::UnequalTree => Some((
            EXIT_TEMPFAIL,
            format!("tree hash mismatch: expected {TREE_EXPECTED}, got {TREE_DIFFERENT}"),
        )),
        FinishCase::EmptyOriginal => ordinary("original commit has empty message"),
        FinishCase::InvalidMetadata => ordinary(&format!(
            "truncated commit metadata: expected 6 fields, got 1 for {sha}"
        )),
        FinishCase::InvalidCount => {
            ordinary("corrupted state file 'split_count': invalid value 'wrong'")
        }
        FinishCase::Overflow => ordinary("split_count overflow"),
        FinishCase::Read(_, _) => Some((
            EXIT_SOFTWARE,
            "failed to read state: finish state read denied".to_owned(),
        )),
        FinishCase::WriteCount => Some((
            EXIT_SOFTWARE,
            "failed to write state: finish state write denied".to_owned(),
        )),
        FinishCase::RemoveDenied => ordinary(&format!(
            "failed to remove factor state path '{}': finish state removal denied",
            state.display()
        )),
        FinishCase::RemoveRetained => ordinary(&format!(
            "factor state path '{}' still exists after cleanup",
            state.display()
        )),
        FinishCase::Output(_) => Some((
            EXIT_SOFTWARE,
            "failed to write output: finish output denied".to_owned(),
        )),
        FinishCase::RebaseRetained => ordinary("rebase remained active after span completion"),
        FinishCase::Process(effect, spawn) => {
            let text = if spawn {
                format!("git {}: finish process denied", effect.command())
            } else if effect.output() {
                "finish query refused".to_owned()
            } else {
                format!("git {} failed (exit 23)", effect.command())
            };
            let recovery_text = if effect == FinishStage::Rebase {
                format!(
                    "git command failed: {text}\n\nResolve the rebase issue, then rerun 'git rebase --continue'.\nTo abandon the factor session, run 'git factor --abort'"
                )
            } else {
                text
            };
            ordinary(&recovery_text)
        }
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "independent full request oracle stays separate from the public Act"
)]
fn expected_requests(
    repo: &Path,
    case: FinishCase,
    sha: &str,
    commit: &[&str],
) -> Vec<RecordedCall> {
    let mut requests = vec![directory_request(repo)];
    if matches!(case, FinishCase::Absent) {
        return requests;
    }
    requests.push(directory_request(repo));
    if matches!(
        case,
        FinishCase::ReopenQuery
            | FinishCase::Pending
            | FinishCase::InvalidPhase
            | FinishCase::InvalidCommits
            | FinishCase::InvalidIndex
            | FinishCase::OutsideIndex
            | FinishCase::InvalidRequires
            | FinishCase::InvalidExpected
            | FinishCase::Read("commits" | "phase" | "current_index", _)
            | FinishCase::Read("requires_rebase", 1)
    ) {
        return requests;
    }
    if matches!(case, FinishCase::RebaseMissing) {
        requests.push(directory_request(repo));
        return requests;
    }
    if rebase(case) {
        requests.push(directory_request(repo));
    }
    if fallback(case) {
        requests.push(output_request(
            &["rev-parse", &format!("{sha}^{{tree}}")],
            repo,
        ));
        if matches!(case, FinishCase::Process(FinishStage::ExpectedTree, _)) {
            return requests;
        }
    }
    for (stage, args) in [
        (
            FinishStage::Checkout,
            vec!["checkout", "--quiet", "--", "."],
        ),
        (
            FinishStage::Clean,
            vec!["clean", "--force", "--quiet", "-d"],
        ),
        (
            FinishStage::Restore,
            vec![
                "restore",
                "--source",
                sha,
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
        ),
    ] {
        requests.push(status_request(&args, &[], false, repo));
        if matches!(case, FinishCase::Process(selected, _) if selected == stage) {
            return requests;
        }
    }
    if !append_commit_requests(&mut requests, repo, case, sha, commit) {
        return requests;
    }
    append_completion_requests(&mut requests, repo, case, sha);
    requests
}

fn fallback(case: FinishCase) -> bool {
    matches!(
        case,
        FinishCase::Fallback
            | FinishCase::Read("expected_tree", _)
            | FinishCase::Process(FinishStage::ExpectedTree, _)
    )
}

fn journal(state: &Path) -> Vec<(OsString, Vec<u8>)> {
    let mut files = fs::read_dir(state)
        .or_abort("saved journal inventory")
        .map(|result| {
            let entry = result.or_abort("saved journal entry");
            (
                entry.file_name(),
                fs::read(entry.path()).or_abort("saved journal bytes"),
            )
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn original_message(case: FinishCase) -> bool {
    matches!(
        case,
        FinishCase::Original
            | FinishCase::EmptyOriginal
            | FinishCase::Process(FinishStage::OriginalMessage, _)
    )
}

fn output_request(args: &[&str], repo: &Path) -> RecordedCall {
    recorded_output("git", args, repo)
}

fn rebase(case: FinishCase) -> bool {
    matches!(
        case,
        FinishCase::RebaseComplete
            | FinishCase::RebaseRetained
            | FinishCase::Process(FinishStage::Rebase, _)
    )
}

fn recorded_output(bin: &str, args: &[&str], repo: &Path) -> RecordedCall {
    RecordedCall::Output {
        bin: bin.to_owned(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        cwd: repo.to_path_buf(),
    }
}

fn recorded_status(
    bin: &str,
    args: &[&str],
    envs: &[(&str, &str)],
    quiet: bool,
    repo: &Path,
) -> RecordedCall {
    RecordedCall::Status {
        bin: bin.to_owned(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        cwd: repo.to_path_buf(),
        envs: envs
            .iter()
            .map(|&(key, value)| (key.to_owned(), value.to_owned()))
            .collect(),
        quiet,
    }
}

fn root(case: FinishCase) -> bool {
    matches!(
        case,
        FinishCase::RootComplete | FinishCase::Process(FinishStage::Root, _)
    )
}

#[expect(
    clippy::single_call_fn,
    reason = "error-log snapshot request oracle stays separate from the public Act"
)]
fn snapshot_requests(repo: &Path) -> Vec<RecordedCall> {
    vec![
        output_request(&["rev-parse", "--verify", "HEAD"], repo),
        output_request(&["rev-parse", "--verify", HEAD_TREEISH], repo),
        directory_request(repo),
        output_request(&["rev-parse", "--show-toplevel"], repo),
        output_request(
            &[
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
            ],
            repo,
        ),
    ]
}

fn status_request(args: &[&str], envs: &[(&str, &str)], quiet: bool, repo: &Path) -> RecordedCall {
    recorded_status("git", args, envs, quiet, repo)
}

/// One public Act, with an independent exact request/stream/journal oracle.
pub(in crate::git_factor) fn verify_finish(
    case: FinishCase,
    sha: &str,
    previous: u8,
    subject: &str,
    body: &str,
    user: &[u8],
) {
    let directory = TempDir::new().or_abort("owned Finish fixture");
    let repo = directory.path();
    let state = arrange_state(repo, case, sha, previous);
    fs::write(repo.join("unrelated"), user).or_abort("protected user input");
    fs::write(repo.join(".git/index"), b"opaque saved index bytes").or_abort("save index");
    fs::create_dir_all(repo.join(".git/refs/heads")).or_abort("saved reference directory");
    fs::write(repo.join(".git/HEAD"), "ref: refs/heads/feature\n").or_abort("saved symbolic HEAD");
    fs::write(repo.join(".git/refs/heads/feature"), format!("{sha}\n")).or_abort("save ref");
    let before = if state.exists() {
        journal(&state)
    } else {
        Vec::new()
    };
    let original = format!("{subject}\n\n{body}");
    let commit = commit_arguments(case, subject, body, &original);
    let runner = FinishRunner {
        case,
        directory_reads: Cell::new(0),
        observed: Cell::new(false),
        calls: RefCell::new(Vec::new()),
        inner: arrange_runner(repo, case, sha, &commit, &original),
    };
    let filesystem = FinishFs {
        case,
        reads: Cell::new(0),
        observed: Cell::new(false),
    };
    let output = FinishIo {
        fail_at: if let FinishCase::Output(slot) = case {
            Some(slot)
        } else {
            None
        },
        writes: Cell::new(0),
        observed: Cell::new(false),
        inner: TestIo::default(),
    };
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = ctx_from_parts(&environment, &runner, &output, &filesystem);
    let argv: Vec<OsString> = if original_message(case) {
        vec!["git-factor", "--finish"]
    } else {
        vec![
            "git-factor",
            "--finish",
            "--message",
            subject,
            "--message",
            body,
        ]
    }
    .into_iter()
    .map(OsString::from)
    .collect();
    let failure = expected_error(case, repo, sha);
    let mut requests = expected_requests(repo, case, sha, &commit);
    let removed = failure.is_none()
        || matches!(
            case,
            FinishCase::Output(_) | FinishCase::Process(FinishStage::Root, _)
        );
    let persist = failure.as_ref().is_some_and(|&(code, _)| {
        code != EXIT_USAGE && !matches!(case, FinishCase::ReopenQuery | FinishCase::InvalidCommits)
    });
    if persist {
        requests.push(directory_request(repo));
        if !removed {
            requests.push(directory_request(repo));
            requests.extend(snapshot_requests(repo));
        }
    }

    let actual_code = main_entry_with_vec(&output, ctx, &argv);

    assert_eq!(
        actual_code,
        failure.as_ref().map_or(EXIT_OK, |&(code, _)| code)
    );
    verify_streams(&output, case, failure.as_ref(), previous);
    assert_eq!(*runner.calls.borrow(), requests);
    verify_saved_bytes(repo, case, sha, user, removed);
    verify_journal(case, repo, sha, previous, &argv, failure.as_ref(), before);
    if matches!(
        case,
        FinishCase::Read(_, _)
            | FinishCase::WriteCount
            | FinishCase::RemoveDenied
            | FinishCase::RemoveRetained
    ) {
        assert!(filesystem.observed.get());
    }
    if matches!(case, FinishCase::Process(_, _) | FinishCase::ReopenQuery) {
        assert!(runner.observed.get());
    }
    if matches!(case, FinishCase::Output(_)) {
        assert!(output.observed.get());
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "saved journal conservation stays separate from the public Act"
)]
fn verify_journal(
    case: FinishCase,
    repo: &Path,
    sha: &str,
    previous: u8,
    argv: &[OsString],
    failure: Option<&(i32, String)>,
    before: Vec<(OsString, Vec<u8>)>,
) {
    let state = repo.join(".git/factor");
    let final_count = previous.checked_add(1);
    let persist = failure.is_some_and(|&(code, _)| {
        code != EXIT_USAGE && !matches!(case, FinishCase::ReopenQuery | FinishCase::InvalidCommits)
    });
    if state.exists() {
        let mut expected = before;
        if count_was_written(case) {
            let count = final_count.or_abort("admitted bounded count write");
            let entry = expected
                .iter_mut()
                .find(|entry| entry.0 == "split_count")
                .or_abort("saved count field");
            entry.1 = format!("{count}\n").into_bytes();
        }
        let mut observed = journal(&state);
        if persist {
            let text = fs::read_to_string(state.join("error.log")).or_abort("owned diagnostic log");
            let (timestamp, logged) = text
                .split_once('\n')
                .or_abort("timestamp and owned diagnostic");
            let millis = timestamp
                .strip_prefix("ts_unix_ms=")
                .or_abort("literal timestamp field");
            assert!(!millis.is_empty());
            assert!(millis.bytes().all(|byte| byte.is_ascii_digit()));
            let expected_log = expected_diagnostic(case, repo, sha, previous, argv, failure);
            assert_eq!(logged, expected_log);
            observed.retain(|entry| entry.0 != "error.log");
        }
        assert_eq!(observed, expected);
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "protected file observers stay separate from stream and journal assertions"
)]
fn verify_saved_bytes(repo: &Path, case: FinishCase, sha: &str, user: &[u8], removed: bool) {
    let state = repo.join(".git/factor");
    assert_eq!(
        fs::read(repo.join("unrelated")).or_abort("protected input remains"),
        user
    );
    assert_eq!(
        fs::read(repo.join(".git/index")).or_abort("raw test index remains"),
        b"opaque saved index bytes"
    );
    assert_eq!(
        fs::read(repo.join(".git/HEAD")).or_abort("symbolic HEAD remains"),
        b"ref: refs/heads/feature\n"
    );
    assert_eq!(
        fs::read(repo.join(".git/refs/heads/feature")).or_abort("branch bytes remain"),
        format!("{sha}\n").into_bytes()
    );
    // These are fixture bytes, not a claim of native Git ref updates.
    assert_eq!(
        repo.join(".git/rebase-merge").exists(),
        rebase(case) && !matches!(case, FinishCase::RebaseComplete)
    );
    assert_eq!(
        state.exists(),
        !removed && !matches!(case, FinishCase::Absent)
    );
}

#[expect(
    clippy::single_call_fn,
    reason = "exact stream observer stays separate from journal and byte observers"
)]
fn verify_streams(
    output: &FinishIo,
    case: FinishCase,
    failure: Option<&(i32, String)>,
    previous: u8,
) {
    let final_count = previous.checked_add(1);
    let complete = if failure.is_none() || matches!(case, FinishCase::Output(2)) {
        format!(
            "{{\"operation\":\"finish\",\"split_count\":{}}}",
            final_count.or_abort("admitted positive completed count")
        )
    } else {
        String::new()
    };
    let expected_stdout = if failure.is_none() {
        format!("{complete}\n")
    } else {
        complete
    };
    assert_eq!(output.inner.stdout(), expected_stdout);
    assert_eq!(
        output.inner.stderr(),
        failure.map_or_else(String::new, |error| format!("{}\n", error.1))
    );
}
