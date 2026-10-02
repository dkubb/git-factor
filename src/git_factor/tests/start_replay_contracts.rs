#[path = "start_replay_cleanup_contracts.rs"]
pub(in crate::git_factor) mod cleanup;

use super::*;

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor) enum ReplayCase {
    BannerFailure(NonZeroUsize),
    FailedWithoutPause,
    FinishedWithoutPause,
    InvalidCommits,
    InvalidPhase,
    Paused,
    Waiting,
    WaitingAfterBegin,
}

impl ReplayCase {
    fn expected_result(&self, count: usize) -> Result<i32, String> {
        match *self {
            Self::BannerFailure(at) => {
                if at.get() > DirectStart::banner(count, true).len() {
                    Ok(EXIT_OK)
                } else {
                    Err("failed to write output: selected output write failed".to_owned())
                }
            }
            Self::FailedWithoutPause => {
                Err("git command failed: git rebase failed (exit 1)".to_owned())
            }
            Self::FinishedWithoutPause => Err(concat!(
                "git command failed: git rebase finished without pausing ",
                "at the factor session break"
            )
            .to_owned()),
            Self::InvalidCommits => Err("invalid commit: bad".to_owned()),
            Self::InvalidPhase => Err(
                "git command failed: factor session is not waiting to begin splitting".to_owned(),
            ),
            Self::Paused => Ok(EXIT_OK),
            Self::Waiting | Self::WaitingAfterBegin => Ok(EXIT_TEMPFAIL),
        }
    }

    fn has_native_flag(&self) -> bool {
        matches!(
            *self,
            Self::BannerFailure(_)
                | Self::InvalidCommits
                | Self::InvalidPhase
                | Self::Paused
                | Self::Waiting
                | Self::WaitingAfterBegin
        )
    }
}

struct ReplayBootstrap {
    command: RecordedCall,
    journal: Option<BTreeMap<OsString, String>>,
    native_flag: Option<PathBuf>,
    state: PathBuf,
}

impl ReplayBootstrap {
    #[expect(
        clippy::create_dir,
        reason = "fail fast if the bootstrap runs twice or the SUT pre-created state"
    )]
    fn apply(&self) {
        // Arrangement failures must abort as fixture errors, never become SUT query failures.
        if let Some(journal) = self.journal.as_ref() {
            fs::create_dir(&self.state).or_abort("replay journal bootstrap");
            for (name, text) in journal {
                fs::write(self.state.join(name), text).or_abort("replay journal field bootstrap");
            }
        }
        if let Some(native_flag) = self.native_flag.as_ref() {
            fs::create_dir(native_flag).or_abort("replay native flag bootstrap");
        }
    }
}

struct ReplayRunner {
    bootstrap: ReplayBootstrap,
    inner: ObservedRunner,
}

impl Runner for ReplayRunner {
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
        if RecordedCall::status(bin, args, envs, quiet, cwd) == self.bootstrap.command {
            self.bootstrap.apply();
        }
        self.inner.status(bin, args, envs, quiet, cwd)
    }
}

#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "canonical tests read fixture facts; environment and runner capabilities stay private"
)]
// The separate closed runner keeps rebase bootstrap authority out of Gate/Query fixtures.
// Duplicate observation methods are intentional; no generic fixture layer is needed.
pub(in crate::git_factor) struct ReplayStart {
    _dir: TempDir,
    pub(in crate::git_factor) direct_files_before: BTreeMap<PathBuf, DirectEntry>,
    env: TestEnv,
    pub(in crate::git_factor) exec: NonEmpty<NonEmptyString>,
    pub(in crate::git_factor) expected_calls: Vec<RecordedCall>,
    pub(in crate::git_factor) expected_journal: Option<BTreeMap<OsString, String>>,
    pub(in crate::git_factor) expected_result: Result<i32, String>,
    pub(in crate::git_factor) expected_stdout: String,
    pub(in crate::git_factor) io: CapturedIo,
    runner: ReplayRunner,
    pub(in crate::git_factor) selected: NonEmpty<CommitSha>,
    pub(in crate::git_factor) state: StateDir,
}

impl ReplayStart {
    pub(in crate::git_factor) fn ctx(&self) -> Ctx<'_> {
        Ctx {
            runner: &self.runner,
            cwd: self.env.cwd.clone(),
            io: &self.io,
            env: &self.env,
            fs: &REAL_FS,
        }
    }

    pub(in crate::git_factor) fn direct_files_after(&self) -> BTreeMap<PathBuf, DirectEntry> {
        direct_files(self.env.cwd.as_path(), &self.state)
    }

    pub(in crate::git_factor) fn observed_calls(&self) -> Vec<RecordedCall> {
        self.runner.inner.calls()
    }

    pub(in crate::git_factor) fn observed_journal(&self) -> Option<BTreeMap<OsString, String>> {
        let entries = match fs::read_dir(self.state.as_path()) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
            other => other.or_abort("replay journal observation"),
        };
        Some(
            entries
                .map(|item| {
                    let entry = item.or_abort("replay journal entry");
                    let text = fs::read_to_string(entry.path()).or_abort("replay journal value");
                    (entry.file_name(), text)
                })
                .collect(),
        )
    }

    pub(in crate::git_factor) fn remaining_keys(&self) -> Vec<String> {
        self.runner.inner.remaining_keys()
    }

    #[expect(
        clippy::single_call_fn,
        reason = "the strategy ordinal bound belongs with the private banner fixture"
    )]
    pub(in crate::git_factor) fn single_commit_banner_fault_end() -> NonZeroUsize {
        NonZeroUsize::new(
            DirectStart::banner(1, true)
                .len()
                .checked_add(1)
                .or_abort("bounded banner writes"),
        )
        .or_abort("nonempty banner")
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "construct replay admission independently of the single-HEAD gate world"
)]
fn admission(
    repo: &Path,
    selected: &NonEmpty<CommitSha>,
    root: bool,
    start_head: &CommitSha,
) -> ObservedRunner {
    let mut runner = ObservedRunner::default().with_output(
        "git",
        &["rev-parse", "--verify", "HEAD"],
        repo,
        &format!("{start_head}\n"),
    );
    for sha in selected {
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
    runner
        .with_status(
            "git",
            &[
                "rev-parse",
                "--quiet",
                "--verify",
                &format!("{}^", selected.first()),
            ],
            &[],
            true,
            repo,
            if root { 1 << 8 } else { 0 },
        )
        .with_output(
            "git",
            &["rev-parse", "--short", selected.last().as_str()],
            repo,
            "abcdef0\n",
        )
        .with_output(
            "git",
            &[
                "show",
                "--format=%B",
                "--no-patch",
                selected.last().as_str(),
            ],
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
}

#[expect(
    clippy::single_call_fn,
    reason = "post-rebase queries depend on the selected recovery boundary"
)]
fn after_replay(
    mut runner: ObservedRunner,
    repo: &Path,
    selected: &NonEmpty<CommitSha>,
    root: bool,
    case: &ReplayCase,
) -> ObservedRunner {
    if matches!(
        *case,
        ReplayCase::FinishedWithoutPause
            | ReplayCase::FailedWithoutPause
            | ReplayCase::Waiting
            | ReplayCase::WaitingAfterBegin
    ) {
        return runner;
    }
    runner = runner.with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    if matches!(*case, ReplayCase::InvalidCommits | ReplayCase::InvalidPhase) {
        return runner;
    }
    runner = runner.with_output("git", &["status", "--porcelain=v1"], repo, "");
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
        format!("{}^", selected.first())
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
        .with_output(
            "git",
            &[
                "show",
                "--format=%B",
                "--no-patch",
                selected.last().as_str(),
            ],
            repo,
            "subject\n",
        )
        .with_output(
            "git",
            &["rev-parse", "--short", selected.last().as_str()],
            repo,
            "abcdef0\n",
        )
        .with_output("git", &["diff", "--stat"], repo, "")
        .with_output(
            "git",
            &["ls-files", "--others", "--exclude-standard"],
            repo,
            "",
        );
    if matches!(*case, ReplayCase::Paused)
        || matches!(*case, ReplayCase::BannerFailure(at) if at.get() > 9)
    {
        runner = runner.with_output(
            "git",
            &["rev-parse", "--show-toplevel"],
            repo,
            &format!("{}\n", repo.display()),
        );
    }
    runner
}

fn direct_files(repo: &Path, state: &StateDir) -> BTreeMap<PathBuf, DirectEntry> {
    let native_flag = repo.join(".git/rebase-merge");
    // Observe only direct writes outside the owned journal and scripted native-rebase flag.
    DirectStart::direct_files(repo, state.as_path())
        .into_iter()
        .filter(|entry| !entry.0.starts_with(&native_flag))
        .collect()
}

pub(in crate::git_factor) fn direct_start(
    selected: &NonEmpty<CommitSha>,
    root: bool,
    case: &ReplayCase,
) -> ReplayStart {
    let dir = TempDir::new().or_abort("replay fixture tempdir");
    let repo = dir.path();
    fs::create_dir_all(repo.join(".git")).or_abort("replay journal parent");
    fs::write(repo.join("direct-write-sentinel"), b"sentinel\n").or_abort("existing sentinel");
    // A bounded span leaves an unused SHA for the scripted descendant HEAD.
    // Choose it before admission; never rewrite a queued query response.
    let head_text = "0123456789abcdef"
        .chars()
        .map(|digit| digit.to_string().repeat(SHA_LEN))
        .find(|head| selected.iter().all(|sha| sha.as_str() != head))
        .or_abort("bounded selection leaves an admitted descendant SHA");
    let head = CommitSha::new(head_text).or_abort("descendant SHA");
    let admitted = admission(repo, selected, root, &head);
    let (rebased, command) = rebase_script(admitted, repo, selected, head.as_str(), root, case);
    let runner = after_replay(rebased, repo, selected, root, case);
    let state = StateDir::new(repo.join(".git/factor"));
    let input_journal = if matches!(*case, ReplayCase::Waiting) {
        None
    } else {
        Some(pending_journal(selected, root, &head, case))
    };
    let native_flag = case
        .has_native_flag()
        .then(|| repo.join(".git/rebase-merge"));
    let expected_journal = match *case {
        ReplayCase::FinishedWithoutPause | ReplayCase::FailedWithoutPause | ReplayCase::Waiting => {
            None
        }
        ReplayCase::InvalidCommits | ReplayCase::InvalidPhase | ReplayCase::WaitingAfterBegin => {
            input_journal.clone()
        }
        ReplayCase::Paused | ReplayCase::BannerFailure(_) => {
            let mut journal = pending_journal(selected, root, &head, &ReplayCase::Paused);
            journal.insert(OsString::from("phase"), "splitting\n".to_owned());
            Some(journal)
        }
    };
    let fail_at = match *case {
        ReplayCase::BannerFailure(at) => Some(at),
        ReplayCase::FailedWithoutPause
        | ReplayCase::FinishedWithoutPause
        | ReplayCase::InvalidCommits
        | ReplayCase::InvalidPhase
        | ReplayCase::Paused
        | ReplayCase::Waiting
        | ReplayCase::WaitingAfterBegin => None,
    };
    let expected_stdout = if matches!(*case, ReplayCase::Paused | ReplayCase::BannerFailure(_)) {
        DirectStart::streams("", "", DirectStart::banner(selected.len(), true), fail_at).0
    } else {
        String::new()
    };
    let expected_result = case.expected_result(selected.len());
    let expected_calls = runner.expectations();
    let direct_files_before = direct_files(repo, &state);
    ReplayStart {
        env: TestEnv {
            cwd: repo.to_path_buf(),
        },
        state,
        selected: selected.clone(),
        exec: NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("replay gate")),
        io: CapturedIo {
            stdout: RefCell::new(String::new()),
            stderr: RefCell::new(String::new()),
            writes: Cell::new(0),
            fail_at,
        },
        runner: ReplayRunner {
            bootstrap: ReplayBootstrap {
                command,
                journal: input_journal,
                native_flag,
                state: repo.join(".git/factor"),
            },
            inner: runner,
        },
        expected_calls,
        expected_journal,
        expected_result,
        expected_stdout,
        direct_files_before,
        _dir: dir,
    }
}

fn pending_journal(
    selected: &NonEmpty<CommitSha>,
    root: bool,
    head: &CommitSha,
    case: &ReplayCase,
) -> BTreeMap<OsString, String> {
    // Literal state arranged by the hidden begin command; never use the SUT state writer.
    let commits = if matches!(*case, ReplayCase::InvalidCommits) {
        "bad\n".to_owned()
    } else {
        format!(
            "{}\n",
            selected
                .iter()
                .map(CommitSha::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    [
        ("commits", commits),
        ("current_index", format!("{}\n", selected.tail.len())),
        ("exec", "true\n".to_owned()),
        ("expected_tree", TREE_EXPECTED_NL.to_owned()),
        ("is_root", format!("{root}\n")),
        (
            "phase",
            if matches!(*case, ReplayCase::InvalidPhase) {
                "splitting\n"
            } else {
                "pending_start\n"
            }
            .to_owned(),
        ),
        ("requires_rebase", "true\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{head}\n")),
        ("started_rebase", "true\n".to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (OsString::from(name), text))
    .collect()
}

fn rebase_script(
    admitted: ObservedRunner,
    repo: &Path,
    selected: &NonEmpty<CommitSha>,
    head: &str,
    root: bool,
    case: &ReplayCase,
) -> (ObservedRunner, RecordedCall) {
    let first = selected.first().as_str();
    let tip = selected.last().as_str();
    fs::write(repo.join("git-factor"), "").or_abort("executable fixture");
    let executable = fs::canonicalize(repo.join("git-factor")).or_abort("canonical executable");
    let editor = executable.with_file_name("git-sequence-editor");
    let quoted_exe = format!(
        "'{}'",
        repo.join("git-factor")
            .to_str()
            .or_abort("UTF-8 executable")
            .replace('\'', "'\\''")
    );
    let quoted_editor = format!(
        "'{}'",
        editor
            .to_str()
            .or_abort("UTF-8 editor")
            .replace('\'', "'\\''")
    );
    let index = selected.len().checked_sub(1).or_abort("nonempty selection");
    let preflight = format!("{quoted_exe} 'rebase-exec-preflight' '{index}' 'true'");
    let commits = selected
        .iter()
        .map(CommitSha::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let begin =
        format!("{quoted_exe} 'rebase-exec-begin' '{index}' '{head}' '{root}' 'true' '{commits}'");
    let sequence = format!(
        "{quoted_editor} '--factor-target' 'abcdef0' '--factor-preflight' '{}' '--factor-begin' '{}'",
        preflight.replace('\'', "'\\''"),
        begin.replace('\'', "'\\''")
    );
    let parent = format!("{first}^");
    let args = [
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
        if root { "--root" } else { &parent },
    ];
    let envs = [
        ("GIT_EDITOR", "false"),
        ("GIT_SEQUENCE_EDITOR", sequence.as_str()),
    ];
    let command = RecordedCall::status("git", &args, &envs, false, repo);
    let runner = admitted
        .with_output("git", &["rev-parse", "--short", tip], repo, "abcdef0\n")
        .with_status(
            "git",
            &args,
            &envs,
            false,
            repo,
            if matches!(
                *case,
                ReplayCase::Waiting
                    | ReplayCase::WaitingAfterBegin
                    | ReplayCase::FailedWithoutPause
            ) {
                1 << 8
            } else {
                0
            },
        )
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    (runner, command)
}
