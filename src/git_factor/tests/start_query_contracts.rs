use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::git_factor) enum QueryTarget {
    Ancestor(QueryPosition),
    Diff,
    EmptyRoot,
    Gate,
    Head,
    MergeParent(QueryPosition),
    Message,
    Parent,
    Reset,
    Short,
    Syntax,
    TopLevel,
    Tree,
    Untracked,
    Worktree,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::git_factor) enum QueryPosition {
    First,
    Last,
}

impl QueryPosition {
    fn index(self, selected: &NonEmpty<CommitSha>) -> usize {
        match self {
            Self::First => 0,
            Self::Last => selected.tail.len(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor) enum QueryReply {
    Io,
    Rejected,
}

#[derive(Clone, Debug)]
pub(in crate::git_factor) enum QueryCase {
    EmptyShort,
    Failure {
        target: QueryTarget,
        reply: QueryReply,
    },
}

impl QueryCase {
    fn expected_result(&self, selected: &NonEmpty<CommitSha>) -> Result<i32, String> {
        match *self {
            Self::EmptyShort => Err("git command failed: empty short SHA".to_owned()),
            Self::Failure {
                target: QueryTarget::Head,
                ..
            } => {
                // Baseline collapses HEAD query IO/spawn failure into InvalidCommit.
                Err("invalid commit: HEAD".to_owned())
            }
            Self::Failure {
                target: QueryTarget::Ancestor(position),
                reply,
            } => {
                let sha = selected
                    .iter()
                    .nth(position.index(selected))
                    .or_abort("admitted target");
                Err(match reply {
                    QueryReply::Io => concat!(
                        "git command failed: git command failed: ",
                        "git merge-base: selected query IO failure"
                    )
                    .to_owned(),
                    QueryReply::Rejected => format!("commit {sha} is not an ancestor of HEAD"),
                })
            }
            Self::Failure {
                target: QueryTarget::Syntax,
                reply,
            } => Err(match reply {
                // These double-wrapped diagnostics are characterization, not an endorsement.
                QueryReply::Io => concat!(
                    "git command failed: bash syntax check: ",
                    "git command failed: bash --norc: selected query IO failure"
                )
                .to_owned(),
                QueryReply::Rejected => "invalid exec syntax: true".to_owned(),
            }),
            Self::Failure {
                target: QueryTarget::Gate,
                reply: QueryReply::Rejected,
            } => Err("exec gate failed: true (exit code 128)".to_owned()),
            Self::Failure {
                target: QueryTarget::Worktree,
                reply: QueryReply::Rejected,
            } => Err(concat!(
                "git command failed: git status --porcelain=v1 produced unexpected ",
                "output (exit 128)\nSTDERR:\nselected query refused"
            )
            .to_owned()),
            Self::Failure {
                target: QueryTarget::Reset,
                reply: QueryReply::Rejected,
            } => Err("git command failed: git reset failed (exit 128)".to_owned()),
            Self::Failure { target, reply } => Err(match reply {
                QueryReply::Io => format!(
                    "git command failed: {}: selected query IO failure",
                    target.command_label()
                ),
                QueryReply::Rejected => "git command failed: selected query refused".to_owned(),
            }),
        }
    }

    fn is_io_output(&self, is_output: bool) -> bool {
        matches!(
            *self,
            Self::Failure {
                reply: QueryReply::Io,
                ..
            }
        ) && is_output
    }

    fn root(&self, requested: bool) -> bool {
        requested
            || matches!(
                *self,
                Self::Failure {
                    target: QueryTarget::EmptyRoot,
                    ..
                }
            )
    }

    fn target(&self) -> QueryTarget {
        match *self {
            Self::EmptyShort => QueryTarget::Short,
            Self::Failure { target, .. } => target,
        }
    }
}

impl QueryTarget {
    fn command_label(self) -> &'static str {
        match self {
            Self::Ancestor(_) => "git merge-base",
            Self::Diff => "git diff",
            Self::EmptyRoot => "git commit-tree",
            Self::Gate => "bash -c",
            Self::Head | Self::Short | Self::TopLevel | Self::Tree => "git rev-parse",
            Self::Parent | Self::MergeParent(_) => "git cat-file",
            Self::Message => "git show",
            Self::Reset => "git reset",
            Self::Syntax => "bash --norc",
            Self::Untracked => "git ls-files",
            Self::Worktree => "git status",
        }
    }
    fn site(self, selected: &NonEmpty<CommitSha>) -> QuerySite {
        match self {
            Self::Ancestor(position) => QuerySite::Ancestor(position.index(selected)),
            Self::MergeParent(position) => QuerySite::MergeParent(position.index(selected)),
            Self::Diff
            | Self::EmptyRoot
            | Self::Gate
            | Self::Head
            | Self::Message
            | Self::Parent
            | Self::Reset
            | Self::Short
            | Self::Syntax
            | Self::TopLevel
            | Self::Tree
            | Self::Untracked
            | Self::Worktree => QuerySite::Query(self),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum QuerySite {
    Ancestor(usize),
    MergeParent(usize),
    Query(QueryTarget),
}

enum QueryStep {
    Output {
        args: Vec<String>,
        bin: String,
        cwd: PathBuf,
        target: QuerySite,
        text: String,
    },
    Status {
        args: Vec<String>,
        bin: String,
        code: i32,
        cwd: PathBuf,
        quiet: bool,
        target: QuerySite,
    },
}

impl QueryStep {
    fn call(self) -> RecordedCall {
        match self {
            Self::Output { bin, args, cwd, .. } => RecordedCall::Output { bin, args, cwd },
            Self::Status {
                bin,
                args,
                cwd,
                quiet,
                ..
            } => RecordedCall::Status {
                bin,
                args,
                cwd,
                quiet,
                envs: Vec::new(),
            },
        }
    }

    fn is_output(&self) -> bool {
        matches!(*self, Self::Output { .. })
    }

    fn output(target: QueryTarget, bin: &str, args: &[&str], repo: &Path, text: &str) -> Self {
        Self::Output {
            bin: bin.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            cwd: repo.to_path_buf(),
            target: QuerySite::Query(target),
            text: text.to_owned(),
        }
    }

    fn status(
        target: QueryTarget,
        bin: &str,
        args: &[&str],
        quiet: bool,
        repo: &Path,
        code: i32,
    ) -> Self {
        Self::status_site(QuerySite::Query(target), bin, args, quiet, repo, code)
    }

    fn status_site(
        target: QuerySite,
        bin: &str,
        args: &[&str],
        quiet: bool,
        repo: &Path,
        code: i32,
    ) -> Self {
        Self::Status {
            bin: bin.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            cwd: repo.to_path_buf(),
            quiet,
            code,
            target,
        }
    }

    fn target(&self) -> QuerySite {
        match *self {
            Self::Output { target, .. } | Self::Status { target, .. } => target,
        }
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "linear pre-gate query script with explicit ancestor and merge-query sites"
)]
fn admission_steps(
    repo: &Path,
    selected: &NonEmpty<CommitSha>,
    root: bool,
    stdout: &str,
) -> Vec<QueryStep> {
    let first = selected.first().as_str();
    let tip = selected.last().as_str();
    let mut steps = vec![QueryStep::output(
        QueryTarget::Head,
        "git",
        &["rev-parse", "--verify", "HEAD"],
        repo,
        &format!("{tip}\n"),
    )];
    for (index, sha) in selected.iter().enumerate() {
        steps.push(QueryStep::status_site(
            QuerySite::Ancestor(index),
            "git",
            &["merge-base", "--is-ancestor", sha.as_str(), "HEAD"],
            true,
            repo,
            0,
        ));
    }
    steps.extend([
        QueryStep::output(
            QueryTarget::Parent,
            "git",
            &["cat-file", "commit", first],
            repo,
            &format!(
                "tree {}\n{}author Example <example@example.com> 1 +0000\n\nsubject\n",
                "c".repeat(SHA_LEN),
                if root {
                    String::new()
                } else {
                    format!("parent {}\n", "d".repeat(SHA_LEN))
                },
            ),
        ),
        QueryStep::output(
            QueryTarget::Short,
            "git",
            &["rev-parse", "--short", tip],
            repo,
            "abcdef0\n",
        ),
        QueryStep::output(
            QueryTarget::Message,
            "git",
            &["show", "--format=%B", "--no-patch", tip],
            repo,
            "subject\n",
        ),
        QueryStep::status(
            QueryTarget::Syntax,
            "bash",
            &["--norc", "--noprofile", "-n", "-c", "true"],
            true,
            repo,
            0,
        ),
        QueryStep::output(QueryTarget::Gate, "bash", &["-c", "true"], repo, stdout),
    ]);
    steps
}

fn after_output_io(mut runner: ObservedRunner, repo: &Path, tip: &str) -> ObservedRunner {
    // Baseline command_output snapshots even with tracing disabled; keep answers valid.
    for (args, text) in [
        (vec!["rev-parse", "--verify", "HEAD"], format!("{tip}\n")),
        (
            vec!["rev-parse", "--verify", "HEAD^{tree}"],
            TREE_EXPECTED_NL.to_owned(),
        ),
        (vec!["rev-parse", "--git-dir"], ".git\n".to_owned()),
        (
            vec!["rev-parse", "--show-toplevel"],
            format!("{}\n", repo.display()),
        ),
        (
            vec![
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
            ],
            String::new(),
        ),
    ] {
        runner = runner.with_output("git", &args, repo, &text);
    }
    runner
}

fn append_query_step(
    mut runner: ObservedRunner,
    step: QueryStep,
    selected: bool,
    case: &QueryCase,
) -> ObservedRunner {
    if selected
        && matches!(
            *case,
            QueryCase::Failure {
                reply: QueryReply::Io,
                ..
            }
        )
    {
        runner.expected.push(step.call());
        runner.io_fault_at =
            Some(NonZeroUsize::new(runner.expected.len()).or_abort("nonempty ledger"));
        return runner;
    }
    match step {
        QueryStep::Output {
            bin,
            args,
            cwd,
            text,
            ..
        } => {
            let borrowed = args.iter().map(String::as_str).collect::<Vec<_>>();
            if selected
                && matches!(
                    case,
                    QueryCase::Failure {
                        reply: QueryReply::Rejected,
                        ..
                    }
                )
            {
                runner.with_output_status(
                    &bin,
                    &borrowed,
                    &cwd,
                    128 << 8,
                    "",
                    "selected query refused",
                )
            } else {
                runner.with_output(
                    &bin,
                    &borrowed,
                    &cwd,
                    if selected && matches!(*case, QueryCase::EmptyShort) {
                        ""
                    } else {
                        &text
                    },
                )
            }
        }
        QueryStep::Status {
            bin,
            args,
            cwd,
            code,
            quiet,
            ..
        } => {
            let borrowed = args.iter().map(String::as_str).collect::<Vec<_>>();
            runner.with_status(
                &bin,
                &borrowed,
                &[],
                quiet,
                &cwd,
                if selected { 128 << 8 } else { code },
            )
        }
    }
}

/// Arranges explicit span admission, before resolved-start gate execution.
pub(in crate::git_factor) fn selected_start(
    selected: &NonEmpty<CommitSha>,
    case: &QueryCase,
) -> DirectStart {
    let dir = TempDir::new().or_abort("selected-object fixture tempdir");
    let repo = dir.path();
    fs::create_dir_all(repo.join(".git")).or_abort("state parent fixture");
    fs::write(repo.join("direct-write-sentinel"), b"sentinel\n").or_abort("existing sentinel");
    let mut runner = ObservedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output("git", &["status", "--porcelain=v1"], repo, "")
        .with_output(
            "git",
            &["rev-list", "--reverse", "--ancestry-path", "base..tip"],
            repo,
            &format!(
                "{}\n",
                selected
                    .iter()
                    .map(CommitSha::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        );
    let target = case.target().site(selected);
    let mut seen = false;
    for (index, sha) in selected.iter().enumerate() {
        let step = QueryStep::Output {
            bin: "git".to_owned(),
            args: vec!["cat-file".to_owned(), "commit".to_owned(), sha.to_string()],
            cwd: repo.to_path_buf(),
            text: format!(
                "tree {}\nparent {}\nauthor Example <example@example.com> 1 +0000\n\nsubject\n",
                "c".repeat(SHA_LEN),
                "d".repeat(SHA_LEN),
            ),
            target: QuerySite::MergeParent(index),
        };
        let is_target = step.target() == target;
        runner = append_query_step(runner, step, is_target, case);
        if is_target {
            seen = true;
            if case.is_io_output(true) {
                runner = after_output_io(runner, repo, selected.last().as_str());
            }
            break;
        }
    }
    assert!(
        seen,
        "selected object target must belong to this admitted span"
    );
    let expected_calls = runner.expectations();
    let direct_files_before = DirectStart::direct_files(repo, &repo.join(".git/factor"));
    DirectStart {
        env: TestEnv {
            cwd: repo.to_path_buf(),
        },
        state: StateDir::new(repo.join(".git/factor")),
        selected: selected.clone(),
        exec: NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("query gate")),
        io: CapturedIo {
            stdout: RefCell::new(String::new()),
            stderr: RefCell::new(String::new()),
            writes: Cell::new(0),
            fail_at: None,
        },
        _dir: dir,
        runner,
        expected_result: case.expected_result(selected),
        expected_calls,
        expected_journal: None,
        expected_stdout: String::new(),
        expected_stderr: String::new(),
        direct_files_before,
    }
}

pub(in crate::git_factor) fn direct_start(
    selected: &NonEmpty<CommitSha>,
    requested_root: bool,
    case: &QueryCase,
    stdout: &str,
    stderr: &str,
) -> DirectStart {
    let dir = TempDir::new().or_abort("query fixture tempdir");
    let repo = dir.path();
    fs::create_dir_all(repo.join(".git")).or_abort("state parent fixture");
    fs::write(repo.join("direct-write-sentinel"), b"sentinel\n").or_abort("existing sentinel");
    let root = case.root(requested_root);
    let mut steps = admission_steps(repo, selected, root, stdout);
    steps.extend(opened_steps(repo, selected, root));
    let target = case.target().site(selected);
    let mut runner = ObservedRunner::default();
    let mut seen = false;
    for step in steps {
        let is_target = step.target() == target;
        let snapshots = is_target && case.is_io_output(step.is_output());
        let is_gate = matches!(step.target(), QuerySite::Query(QueryTarget::Gate));
        if is_gate && !is_target {
            runner = runner.with_output_status("bash", &["-c", "true"], repo, 0, stdout, stderr);
        } else {
            runner = append_query_step(runner, step, is_target, case);
        }
        if is_target {
            seen = true;
            if snapshots {
                runner = after_output_io(runner, repo, selected.last().as_str());
            }
            break;
        }
    }
    assert!(seen, "query target must belong to this admitted span");
    let expected_journal = query_journal(selected, root, case);
    let (expected_stdout, expected_stderr) = query_streams(selected.len(), case, stdout, stderr);
    let expected_calls = runner.expectations();
    let direct_files_before = DirectStart::direct_files(repo, &repo.join(".git/factor"));
    DirectStart {
        env: TestEnv {
            cwd: repo.to_path_buf(),
        },
        state: StateDir::new(repo.join(".git/factor")),
        selected: selected.clone(),
        exec: NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("query gate")),
        io: CapturedIo {
            stdout: RefCell::new(String::new()),
            stderr: RefCell::new(String::new()),
            writes: Cell::new(0),
            fail_at: None,
        },
        _dir: dir,
        runner,
        expected_result: case.expected_result(selected),
        expected_calls,
        expected_journal,
        expected_stdout,
        expected_stderr,
        direct_files_before,
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "linear post-gate query script names the already-open recovery boundaries"
)]
fn opened_steps(repo: &Path, selected: &NonEmpty<CommitSha>, root: bool) -> Vec<QueryStep> {
    let first = selected.first().as_str();
    let mut steps = vec![
        QueryStep::output(
            QueryTarget::Worktree,
            "git",
            &["status", "--porcelain=v1"],
            repo,
            "",
        ),
        QueryStep::output(
            QueryTarget::Tree,
            "git",
            &["rev-parse", "HEAD^{tree}"],
            repo,
            TREE_EXPECTED_NL,
        ),
    ];
    let reset = if root {
        steps.push(QueryStep::output(
            QueryTarget::EmptyRoot,
            "git",
            &[
                "commit-tree",
                "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
                "-m",
                "empty",
            ],
            repo,
            TREE_DIFFERENT_NL,
        ));
        TREE_DIFFERENT.to_owned()
    } else {
        format!("{first}^")
    };
    steps.extend([
        QueryStep::status(
            QueryTarget::Reset,
            "git",
            &["reset", "--quiet", &reset],
            false,
            repo,
            0,
        ),
        QueryStep::output(QueryTarget::Diff, "git", &["diff", "--stat"], repo, ""),
        QueryStep::output(
            QueryTarget::Untracked,
            "git",
            &["ls-files", "--others", "--exclude-standard"],
            repo,
            "",
        ),
        QueryStep::output(
            QueryTarget::TopLevel,
            "git",
            &["rev-parse", "--show-toplevel"],
            repo,
            &format!("{}\n", repo.display()),
        ),
    ]);
    steps
}

#[expect(
    clippy::single_call_fn,
    reason = "complete or partial legacy journal literals at selected query boundaries"
)]
fn query_journal(
    selected: &NonEmpty<CommitSha>,
    root: bool,
    case: &QueryCase,
) -> Option<BTreeMap<OsString, String>> {
    match case.target() {
        // Characterize partial legacy recovery state after admission; not a new recovery guarantee.
        QueryTarget::Tree => {
            let mut journal = DirectStart::journal(selected, root);
            journal.remove(&OsString::from("expected_tree"));
            Some(journal)
        }
        QueryTarget::EmptyRoot
        | QueryTarget::Reset
        | QueryTarget::Diff
        | QueryTarget::Untracked
        | QueryTarget::TopLevel => Some(DirectStart::journal(selected, root)),
        QueryTarget::Ancestor(_)
        | QueryTarget::Gate
        | QueryTarget::Head
        | QueryTarget::MergeParent(_)
        | QueryTarget::Message
        | QueryTarget::Parent
        | QueryTarget::Short
        | QueryTarget::Syntax
        | QueryTarget::Worktree => None,
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "independent stream prefix for each selected legacy query boundary"
)]
fn query_streams(count: usize, case: &QueryCase, stdout: &str, stderr: &str) -> (String, String) {
    match *case {
        QueryCase::Failure {
            target: QueryTarget::Gate,
            reply: QueryReply::Rejected,
        } => (
            concat!(
                "FACTOR: Start gate failed.\nEXEC: true\nCODE: 128\n\n",
                "NEXT: Fix the current commit, amend it, then rerun git factor.\n"
            )
            .to_owned(),
            "selected query refused".to_owned(),
        ),
        QueryCase::Failure {
            target: QueryTarget::TopLevel,
            ..
        } => DirectStart::streams(
            stdout,
            stderr,
            DirectStart::banner(count, true)
                .into_iter()
                .take(9)
                .collect(),
            None,
        ),
        QueryCase::Failure {
            target:
                QueryTarget::Worktree
                | QueryTarget::Tree
                | QueryTarget::EmptyRoot
                | QueryTarget::Reset
                | QueryTarget::Diff
                | QueryTarget::Untracked,
            ..
        } => (stdout.to_owned(), stderr.to_owned()),
        QueryCase::EmptyShort
        | QueryCase::Failure {
            target:
                QueryTarget::Ancestor(_)
                | QueryTarget::Head
                | QueryTarget::MergeParent(_)
                | QueryTarget::Message
                | QueryTarget::Parent
                | QueryTarget::Short
                | QueryTarget::Syntax,
            ..
        }
        | QueryCase::Failure {
            target: QueryTarget::Gate,
            reply: QueryReply::Io,
        } => (String::new(), String::new()),
    }
}
