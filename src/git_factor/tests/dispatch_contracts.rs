//! Public CLI refusals preserve the saved session and user bytes.
use super::*;
use crate::exit_codes::EXIT_DATAERR;
use alloc::collections::BTreeMap;
use core::cell::Cell;

/// Exact HEAD-resolution fault at the subprocess seam.
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor) enum HeadResolutionFault {
    LaunchFailure,
    NonzeroExit,
}

/// Failure while reopening the no-message continuation session.
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor) enum ReopenFault {
    GitDirectory,
    PhaseEncoding,
    PhaseValue,
}

struct DefaultHeadRunner {
    calls: RefCell<Vec<String>>,
    cwds: RefCell<Vec<PathBuf>>,
    inner: ScriptedRunner,
    resolution: Option<HeadResolutionFault>,
}

struct DirtyRunner<'repo> {
    calls: RefCell<Vec<String>>,
    cwd: &'repo Path,
}

struct FirstWriteFailure {
    failed: RefCell<bool>,
    output: TestIo,
}

/// Records any subprocess attempt so a refusal cannot hide a mutation route.
struct RefusalRunner<'repo> {
    calls: RefCell<Vec<String>>,
    cwd: &'repo Path,
}

struct ReopenRunner<'repo> {
    calls: RefCell<Vec<String>>,
    cwd: &'repo Path,
    directory_queries: Cell<usize>,
    fault: ReopenFault,
}

impl Runner for DefaultHeadRunner {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        self.cwds.borrow_mut().push(cwd.to_path_buf());
        self.calls
            .borrow_mut()
            .push(format!("output {bin} {args:?}"));
        if bin == "git" && args == ["rev-parse", "--verify", "HEAD"] {
            match self.resolution {
                Some(HeadResolutionFault::LaunchFailure) => {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "HEAD resolution launch denied",
                    ));
                }
                Some(HeadResolutionFault::NonzeroExit) => {
                    let status = exit_status(23 << 8);
                    return Ok(Output {
                        status,
                        stdout: b"rejected HEAD output\n".to_vec(),
                        stderr: b"resolution refused\n".to_vec(),
                    });
                }
                None => (),
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
        self.cwds.borrow_mut().push(cwd.to_path_buf());
        self.calls
            .borrow_mut()
            .push(format!("status {bin} {args:?} {envs:?} quiet={quiet}"));
        self.inner.status(bin, args, envs, quiet, cwd)
    }
}

impl Runner for DirtyRunner<'_> {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        if cwd != self.cwd {
            return Err(io::Error::other("dispatcher used an unexpected cwd"));
        }
        self.calls
            .borrow_mut()
            .push(format!("{bin} {}", args.join(" ")));
        let stdout = match (bin, args) {
            ("git", &["rev-parse", "--git-dir"]) => b".git\n".to_vec(),
            ("git", &["status", "--porcelain=v1"]) => b" M unrelated\n".to_vec(),
            _ => return Err(io::Error::other("unexpected dirty-route query")),
        };
        Ok(Output {
            status: exit_status(0),
            stdout,
            stderr: Vec::new(),
        })
    }

    fn status(
        &self,
        _bin: &str,
        _args: &[&str],
        _envs: &[(&str, &str)],
        _quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        if cwd != self.cwd {
            return Err(io::Error::other("dispatcher used an unexpected cwd"));
        }
        self.calls
            .borrow_mut()
            .push("unexpected mutation".to_owned());
        Err(io::Error::other("dirty route must not mutate"))
    }
}
impl Io for FirstWriteFailure {
    fn err(&self, text: &str) -> io::Result<()> {
        if self.failed.replace(true) {
            self.output.err(text)
        } else {
            Err(io::Error::other("parser stream unavailable"))
        }
    }
    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(line)?;
        self.err("\n")
    }
    fn out(&self, text: &str) -> io::Result<()> {
        if self.failed.replace(true) {
            self.output.out(text)
        } else {
            Err(io::Error::other("parser stream unavailable"))
        }
    }
    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }
}

impl Runner for RefusalRunner<'_> {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        if cwd != self.cwd {
            return Err(io::Error::other("dispatcher used an unexpected cwd"));
        }
        self.calls.borrow_mut().push(format!("{bin} {args:?}"));
        Err(io::Error::other("refusal must not launch a subprocess"))
    }

    fn status(
        &self,
        bin: &str,
        args: &[&str],
        _envs: &[(&str, &str)],
        _quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        if cwd != self.cwd {
            return Err(io::Error::other("dispatcher used an unexpected cwd"));
        }
        self.calls.borrow_mut().push(format!("{bin} {args:?}"));
        Err(io::Error::other("refusal must not launch a subprocess"))
    }
}

impl Runner for ReopenRunner<'_> {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        if cwd != self.cwd {
            return Err(io::Error::other("unexpected reopen cwd"));
        }
        self.calls.borrow_mut().push(format!("{bin} {args:?}"));
        if bin == "git" && args == ["rev-parse", "--git-dir"] {
            let ordinal = self
                .directory_queries
                .get()
                .checked_add(1)
                .ok_or_else(|| io::Error::other("directory query count overflow"))?;
            self.directory_queries.set(ordinal);
            if ordinal == 2 && matches!(self.fault, ReopenFault::GitDirectory) {
                return Err(io::Error::other("session reopen denied"));
            }
            return Ok(Output {
                status: exit_status(0),
                stdout: b".git\n".to_vec(),
                stderr: Vec::new(),
            });
        }
        Err(io::Error::other("snapshot query unavailable"))
    }

    fn status(
        &self,
        bin: &str,
        args: &[&str],
        _envs: &[(&str, &str)],
        _quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        if cwd != self.cwd {
            return Err(io::Error::other("unexpected reopen cwd"));
        }
        self.calls.borrow_mut().push(format!("{bin} {args:?}"));
        Err(io::Error::other("reopen refusal must not mutate"))
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "diagnostic log oracle is separate from the public refusal oracle"
)]
fn assert_reopen_log(state: &Path, repo: &Path, fault: ReopenFault, diagnostic: &str) {
    let log = fs::read_to_string(state.join("error.log")).or_abort("owned diagnostic log");
    let (timestamp, body) = log.split_once('\n').or_abort("timestamp separator");
    let timestamp_value = timestamp
        .strip_prefix("ts_unix_ms=")
        .or_abort("timestamp key");
    let _timestamp = timestamp_value
        .parse::<u128>()
        .or_abort("numeric timestamp");
    let source = match fault {
        ReopenFault::GitDirectory | ReopenFault::PhaseValue => "",
        ReopenFault::PhaseEncoding => "source_0=stream did not contain valid UTF-8\n",
    };
    assert_eq!(
        body,
        format!(
            "argv=git-factor --continue\ncwd={}\nerror={diagnostic}\n{source}git_dir={}\nstaged_paths=\nunstaged_paths=\nuntracked_paths=\n",
            repo.display(),
            repo.join(".git").display(),
        )
    );
}

/// Refusal at implicit HEAD resolution remains a public data error.
pub(in crate::git_factor) fn default_head_failure(resolution: HeadResolutionFault) {
    let directory = TempDir::new().or_abort("HEAD resolution failure fixture");
    let repo = directory.path();
    fs::write(repo.join("unrelated"), b"user bytes\n").or_abort("user fixture");
    let runner = DefaultHeadRunner {
        calls: RefCell::new(Vec::new()),
        cwds: RefCell::new(Vec::new()),
        inner: ScriptedRunner::default().with_output(
            "git",
            &["status", "--porcelain=v1"],
            repo,
            "",
        ),
        resolution: Some(resolution),
    };
    let output = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let arguments = [
        OsString::from("git-factor"),
        OsString::from("--exec"),
        OsString::from("true"),
    ];

    let code = main_entry_with_vec(
        &output,
        ctx_from_parts(&environment, &runner, &output, &REAL_FS),
        &arguments,
    );

    assert_eq!(code, EXIT_DATAERR);
    assert_eq!(output.stdout(), "");
    assert_eq!(output.stderr(), "invalid commit: HEAD\n");
    let expected_calls = vec![
        "output git [\"rev-parse\", \"--git-dir\"]",
        "output git [\"rev-parse\", \"--git-dir\"]",
        "output git [\"status\", \"--porcelain=v1\"]",
        "output git [\"rev-parse\", \"--verify\", \"HEAD\"]",
    ];
    assert_eq!(runner.calls.borrow().as_slice(), expected_calls.as_slice());
    for cwd in runner.cwds.borrow().iter() {
        assert_eq!(cwd, repo);
    }
    assert!(!repo.join(".git").exists());
    assert_eq!(
        fs::read(repo.join("unrelated")).or_abort("preserved user bytes"),
        b"user bytes\n"
    );
}

/// Resolves implicit HEAD through the actual dispatcher and opens its selected pool.
pub(in crate::git_factor) fn default_head_success(sha: &str) {
    let directory = TempDir::new().or_abort("default HEAD fixture");
    let repo = directory.path();
    fs::write(repo.join("unrelated"), b"user bytes\n").or_abort("user fixture");
    let tree = "0123456789abcdef0123456789abcdef01234567";
    let runner = DefaultHeadRunner {
        calls: RefCell::new(Vec::new()),
        cwds: RefCell::new(Vec::new()),
        inner: start_resolved_head_runner(
            repo,
            sha,
            tree,
            " file | 1 +\n 1 file changed, 1 insertion(+)\n",
        ),
        resolution: None,
    };
    let output = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let arguments = [
        OsString::from("git-factor"),
        OsString::from("--exec"),
        OsString::from("true"),
    ];

    let code = main_entry_with_vec(
        &output,
        ctx_from_parts(&environment, &runner, &output, &REAL_FS),
        &arguments,
    );

    assert_eq!(code, EXIT_OK);
    assert_eq!(output.stdout(), include_str!("main_entry/default-head.txt"));
    assert_eq!(output.stderr(), "");
    let expected_calls = include_str!("main_entry/default-head-calls.txt")
        .lines()
        .map(|line| line.replace("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", sha))
        .collect::<Vec<_>>();
    assert_eq!(*runner.calls.borrow(), expected_calls);
    for cwd in runner.cwds.borrow().iter() {
        assert_eq!(cwd, repo);
    }
    let state = repo.join(".git/factor");
    let journal = fs::read_dir(&state)
        .or_abort("journal inventory")
        .map(|result| {
            let entry = result.or_abort("journal entry");
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).or_abort("journal bytes"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let expected_journal = [
        ("commits", format!("{sha}\n")),
        ("current_index", "0\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("expected_tree", format!("{tree}\n")),
        ("is_root", "false\n".to_owned()),
        ("phase", "splitting\n".to_owned()),
        ("requires_rebase", "false\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{sha}\n")),
        ("started_rebase", "false\n".to_owned()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.into_bytes()))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(journal, expected_journal);
    assert!(!repo.join(".git/rebase-merge").exists());
    assert!(!repo.join(".git/rebase-apply").exists());
    assert_eq!(
        fs::read(repo.join("unrelated")).or_abort("preserved bytes"),
        b"user bytes\n"
    );
}

/// A deliberately dirty query response distinguishes dispatch routes without Git mutations.
pub(in crate::git_factor) fn dirty_route(
    arguments: &[&str],
    expected_code: i32,
    diagnostic: &str,
    expected_calls: &[&str],
) {
    let directory = TempDir::new().or_abort("dirty route fixture");
    let repo = directory.path();
    fs::create_dir_all(repo.join(".git")).or_abort("git directory fixture");
    fs::write(repo.join("unrelated"), b"user bytes\n").or_abort("user fixture");
    let runner = DirtyRunner {
        calls: RefCell::new(Vec::new()),
        cwd: repo,
    };
    let output = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let mut argv = vec![OsString::from("git-factor")];
    argv.extend(arguments.iter().map(OsString::from));

    let code = main_entry_with_vec(
        &output,
        ctx_from_parts(&environment, &runner, &output, &REAL_FS),
        &argv,
    );

    assert_eq!(code, expected_code);
    assert_eq!(output.stdout(), "");
    assert_eq!(output.stderr(), format!("{diagnostic}\n"));
    assert_eq!(runner.calls.borrow().as_slice(), expected_calls);
    assert_eq!(
        fs::read(repo.join("unrelated")).or_abort("user bytes remain"),
        b"user bytes\n"
    );
    assert!(!repo.join(".git/factor").exists());
    assert!(!repo.join(".git/rebase-merge").exists());
    assert!(!repo.join(".git/rebase-apply").exists());
}

/// Observes parser streams through the same public entrypoint as the binary.
pub(in crate::git_factor) fn parser(
    arguments: &[&str],
    expected_code: i32,
    expected_stdout: &str,
    expected_stderr: &str,
) {
    let directory = TempDir::new().or_abort("parser fixture");
    let repo = directory.path();
    fs::write(repo.join("unrelated"), b"user bytes\n").or_abort("user fixture");
    let runner = RefusalRunner {
        calls: RefCell::new(Vec::new()),
        cwd: repo,
    };
    let output = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let mut argv = vec![OsString::from("git-factor")];
    argv.extend(arguments.iter().map(OsString::from));

    let code = main_entry_with_vec(
        &output,
        ctx_from_parts(&environment, &runner, &output, &REAL_FS),
        &argv,
    );

    assert_eq!(code, expected_code);
    assert_eq!(output.stdout(), expected_stdout);
    assert_eq!(output.stderr(), expected_stderr);
    assert!(runner.calls.borrow().is_empty());
    assert_eq!(
        fs::read(repo.join("unrelated")).or_abort("preserved user bytes"),
        b"user bytes\n"
    );
    assert!(!repo.join(".git").exists());
}

/// Exercises a parser write error through the public error adapter.
pub(in crate::git_factor) fn parser_write_failure(arguments: &[&str]) {
    let directory = TempDir::new().or_abort("parser write failure fixture");
    let repo = directory.path();
    fs::write(repo.join("unrelated"), b"user bytes\n").or_abort("user fixture");
    let runner = RefusalRunner {
        calls: RefCell::new(Vec::new()),
        cwd: repo,
    };
    let output = FirstWriteFailure {
        failed: RefCell::new(false),
        output: TestIo::default(),
    };
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let mut argv = vec![OsString::from("git-factor")];
    argv.extend(arguments.iter().map(OsString::from));

    let code = main_entry_with_vec(
        &output,
        ctx_from_parts(&environment, &runner, &output, &REAL_FS),
        &argv,
    );

    assert_eq!(code, EXIT_SOFTWARE);
    assert!(*output.failed.borrow());
    assert_eq!(output.output.stdout(), "");
    assert_eq!(
        output.output.stderr(),
        "failed to write output: parser stream unavailable\n"
    );
    assert_eq!(
        runner.calls.borrow().as_slice(),
        &["git [\"rev-parse\", \"--git-dir\"]".to_owned()]
    );
    assert_eq!(
        fs::read(repo.join("unrelated")).or_abort("preserved user bytes"),
        b"user bytes\n"
    );
    assert!(!repo.join(".git").exists());
}

/// Drives the actual public adapter with a fixed expected diagnostic.
pub(in crate::git_factor) fn refusal(arguments: &[&str], diagnostic: &str) {
    let directory = TempDir::new().or_abort("dispatcher fixture");
    let repo = directory.path();
    let state = repo.join(".git/factor");
    fs::create_dir_all(&state).or_abort("journal fixture");
    fs::write(state.join("commits"), b"saved session identity\n").or_abort("journal identity");
    fs::write(repo.join("unrelated"), b"user bytes\0\n").or_abort("user fixture");
    let runner = RefusalRunner {
        calls: RefCell::new(Vec::new()),
        cwd: repo,
    };
    let output = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let mut argv = vec![OsString::from("git-factor")];
    argv.extend(arguments.iter().map(OsString::from));

    let code = main_entry_with_vec(
        &output,
        ctx_from_parts(&environment, &runner, &output, &REAL_FS),
        &argv,
    );

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(output.stdout(), "");
    assert_eq!(output.stderr(), format!("{diagnostic}\n"));
    assert!(runner.calls.borrow().is_empty());
    assert_eq!(
        fs::read(state.join("commits")).or_abort("journal remains"),
        b"saved session identity\n"
    );
    assert_eq!(
        fs::read(repo.join("unrelated")).or_abort("user bytes remain"),
        b"user bytes\0\n"
    );
    assert!(!repo.join(".git/rebase-merge").exists());
    assert!(!repo.join(".git/rebase-apply").exists());
    assert!(!state.join("error.log").exists());
    assert_eq!(
        fs::read_dir(&state).or_abort("journal inventory").count(),
        1
    );
    assert!(!repo.join(".git/index").exists());
}

/// Reopening errors preserve saved journal bytes and account for the owned diagnostic log.
pub(in crate::git_factor) fn reopen_failure(
    fault: ReopenFault,
    sha: &str,
    invalid_phase: &str,
    user_bytes: &[u8],
) {
    let directory = TempDir::new().or_abort("reopen fixture");
    let repo = directory.path();
    let state = repo.join(".git/factor");
    fs::create_dir_all(&state).or_abort("saved session directory");
    let commits = format!("{sha}\n");
    fs::write(state.join("commits"), commits.as_bytes()).or_abort("saved commits");
    let phase = match fault {
        ReopenFault::GitDirectory => b"splitting\n".to_vec(),
        ReopenFault::PhaseEncoding => vec![u8::MAX],
        ReopenFault::PhaseValue => format!("{invalid_phase}\n").into_bytes(),
    };
    fs::write(state.join("phase"), &phase).or_abort("saved phase");
    fs::write(repo.join("unrelated"), user_bytes).or_abort("saved user bytes");
    let runner = ReopenRunner {
        calls: RefCell::new(Vec::new()),
        cwd: repo,
        directory_queries: Cell::new(0),
        fault,
    };
    let output = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let argv = [OsString::from("git-factor"), OsString::from("--continue")];

    let code = main_entry_with_vec(
        &output,
        ctx_from_parts(&environment, &runner, &output, &REAL_FS),
        &argv,
    );

    let diagnostic = match fault {
        ReopenFault::GitDirectory => {
            "failed to determine git directory: session reopen denied".to_owned()
        }
        ReopenFault::PhaseEncoding => {
            "failed to read state: stream did not contain valid UTF-8".to_owned()
        }
        ReopenFault::PhaseValue => format!(
            "git command failed: corrupted state file 'phase': invalid value '{invalid_phase}'"
        ),
    };
    let expected_code = match fault {
        ReopenFault::GitDirectory => EXIT_DATAERR,
        ReopenFault::PhaseEncoding | ReopenFault::PhaseValue => EXIT_SOFTWARE,
    };
    assert_eq!(code, expected_code);
    assert_eq!(output.stdout(), "");
    assert_eq!(output.stderr(), format!("{diagnostic}\n"));
    let expected_calls: &[&str] = match fault {
        ReopenFault::GitDirectory => &[
            "git [\"rev-parse\", \"--git-dir\"]",
            "git [\"rev-parse\", \"--git-dir\"]",
        ],
        ReopenFault::PhaseEncoding | ReopenFault::PhaseValue => &[
            "git [\"rev-parse\", \"--git-dir\"]",
            "git [\"rev-parse\", \"--git-dir\"]",
            "git [\"rev-parse\", \"--git-dir\"]",
            "git [\"rev-parse\", \"--git-dir\"]",
            "git [\"rev-parse\", \"--verify\", \"HEAD\"]",
            "git [\"rev-parse\", \"--verify\", \"HEAD^{tree}\"]",
            "git [\"rev-parse\", \"--git-dir\"]",
            "git [\"rev-parse\", \"--show-toplevel\"]",
            "git [\"--no-optional-locks\", \"status\", \"--porcelain=v1\", \"--untracked-files=all\"]",
        ],
    };
    assert_eq!(runner.calls.borrow().as_slice(), expected_calls);
    assert_eq!(
        fs::read(state.join("commits")).or_abort("saved commits remain"),
        commits.as_bytes()
    );
    assert_eq!(
        fs::read(state.join("phase")).or_abort("saved phase remains"),
        phase
    );
    assert_eq!(
        fs::read(repo.join("unrelated")).or_abort("user bytes remain"),
        user_bytes
    );
    assert!(!repo.join(".git/index").exists());
    assert!(!repo.join(".git/rebase-merge").exists());
    assert!(!repo.join(".git/rebase-apply").exists());
    let expected_files = match fault {
        ReopenFault::GitDirectory => {
            assert!(!state.join("error.log").exists());
            2
        }
        ReopenFault::PhaseEncoding | ReopenFault::PhaseValue => {
            assert_reopen_log(&state, repo, fault, &diagnostic);
            3
        }
    };
    assert_eq!(
        fs::read_dir(&state).or_abort("journal inventory").count(),
        expected_files
    );
}
/// Refuses a supplied revision only after preparation has admitted a clean repository.
#[expect(
    clippy::single_call_fn,
    reason = "generated revision rejection oracle stays with private dispatcher arrangements"
)]
pub(in crate::git_factor) fn supplied_commit_failure(
    revision: &str,
    rejected_exit: u8,
    user_bytes: &[u8],
) {
    let directory = TempDir::new().or_abort("supplied rejection fixture");
    let repo = directory.path();
    fs::write(repo.join("unrelated"), user_bytes).or_abort("user fixture");
    let runner = DefaultHeadRunner {
        calls: RefCell::new(Vec::new()),
        cwds: RefCell::new(Vec::new()),
        inner: ScriptedRunner::default()
            .with_output("git", &["status", "--porcelain=v1"], repo, "")
            .with_output_status(
                "git",
                &["rev-parse", "--verify", revision],
                repo,
                i32::from(rejected_exit) << 8,
                "rejected output\n",
                "revision refused\n",
            ),
        resolution: None,
    };
    let output = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let argv = ["git-factor", "--exec", "true", revision].map(OsString::from);

    let code = main_entry_with_vec(
        &output,
        ctx_from_parts(&environment, &runner, &output, &REAL_FS),
        &argv,
    );

    assert_eq!(code, EXIT_DATAERR);
    assert_eq!(output.stdout(), "");
    assert_eq!(output.stderr(), format!("invalid commit: {revision}\n"));
    assert_eq!(
        runner.calls.borrow().as_slice(),
        &[
            "output git [\"rev-parse\", \"--git-dir\"]".to_owned(),
            "output git [\"rev-parse\", \"--git-dir\"]".to_owned(),
            "output git [\"status\", \"--porcelain=v1\"]".to_owned(),
            format!("output git [\"rev-parse\", \"--verify\", {revision:?}]"),
        ]
    );
    for cwd in runner.cwds.borrow().iter() {
        assert_eq!(cwd, repo);
    }
    assert_eq!(
        fs::read(repo.join("unrelated")).or_abort("user bytes remain"),
        user_bytes
    );
    assert!(!repo.join(".git").exists());
}

/// Resolves a supplied revision through the public dispatcher and preserves its selected pool.
#[expect(
    clippy::literal_string_with_formatting_args,
    reason = "HEAD^{tree} is a literal Git revision in the independent request oracle"
)]
#[expect(
    clippy::single_call_fn,
    reason = "generated public start oracle stays in the private dispatcher fixture owner"
)]
pub(in crate::git_factor) fn supplied_commit_success(sha: &str, revision: &str) {
    let directory = TempDir::new().or_abort("supplied commit fixture");
    let repo = directory.path();
    fs::write(repo.join("unrelated"), b"user bytes\n").or_abort("user fixture");
    let tree = "0123456789abcdef0123456789abcdef01234567";
    let runner = DefaultHeadRunner {
        calls: RefCell::new(Vec::new()),
        cwds: RefCell::new(Vec::new()),
        inner: start_resolved_head_runner(
            repo,
            sha,
            tree,
            " file | 1 +\n 1 file changed, 1 insertion(+)\n",
        )
        .with_output(
            "git",
            &["rev-parse", "--verify", revision],
            repo,
            &format!("{sha}\n"),
        )
        .with_commit_object(repo, sha, Some(&"d".repeat(SHA_LEN))),
        resolution: None,
    };
    let output = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let arguments = [
        OsString::from("git-factor"),
        OsString::from("--exec"),
        OsString::from("true"),
        OsString::from(revision),
    ];

    let code = main_entry_with_vec(
        &output,
        ctx_from_parts(&environment, &runner, &output, &REAL_FS),
        &arguments,
    );

    assert_eq!(code, EXIT_OK);
    assert_eq!(output.stdout(), include_str!("main_entry/default-head.txt"));
    assert_eq!(output.stderr(), "");
    let expected_calls = [
        "output git [\"rev-parse\", \"--git-dir\"]".to_owned(),
        "output git [\"rev-parse\", \"--git-dir\"]".to_owned(),
        "output git [\"status\", \"--porcelain=v1\"]".to_owned(),
        format!("output git [\"rev-parse\", \"--verify\", {revision:?}]"),
        format!("output git [\"cat-file\", \"commit\", {sha:?}]"),
        "output git [\"rev-parse\", \"--verify\", \"HEAD\"]".to_owned(),
        format!("status git [\"merge-base\", \"--is-ancestor\", {sha:?}, \"HEAD\"] [] quiet=true"),
        format!("output git [\"cat-file\", \"commit\", {sha:?}]"),
        format!("output git [\"rev-parse\", \"--short\", {sha:?}]"),
        format!("output git [\"show\", \"--format=%B\", \"--no-patch\", {sha:?}]"),
        "status bash [\"--norc\", \"--noprofile\", \"-n\", \"-c\", \"true\"] [] quiet=true"
            .to_owned(),
        "output bash [\"-c\", \"true\"]".to_owned(),
        "output git [\"status\", \"--porcelain=v1\"]".to_owned(),
        "output git [\"rev-parse\", \"HEAD^{tree}\"]".to_owned(),
        format!("status git [\"reset\", \"--quiet\", \"{sha}^\"] [] quiet=false"),
        "output git [\"diff\", \"--stat\"]".to_owned(),
        "output git [\"ls-files\", \"--others\", \"--exclude-standard\"]".to_owned(),
        "output git [\"rev-parse\", \"--show-toplevel\"]".to_owned(),
    ];
    assert_eq!(runner.calls.borrow().as_slice(), expected_calls.as_slice());
    for cwd in runner.cwds.borrow().iter() {
        assert_eq!(cwd, repo);
    }
    let state = repo.join(".git/factor");
    let journal = fs::read_dir(&state)
        .or_abort("journal inventory")
        .map(|result| {
            let entry = result.or_abort("journal entry");
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).or_abort("journal bytes"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let expected_journal = [
        ("commits", format!("{sha}\n")),
        ("current_index", "0\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("expected_tree", format!("{tree}\n")),
        ("is_root", "false\n".to_owned()),
        ("phase", "splitting\n".to_owned()),
        ("requires_rebase", "false\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{sha}\n")),
        ("started_rebase", "false\n".to_owned()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.into_bytes()))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(journal, expected_journal);
    assert!(!repo.join(".git/rebase-merge").exists());
    assert!(!repo.join(".git/rebase-apply").exists());
    assert_eq!(
        fs::read(repo.join("unrelated")).or_abort("preserved bytes"),
        b"user bytes\n"
    );
}
