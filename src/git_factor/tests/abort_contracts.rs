use super::*;
use crate::git_factor::tests::start_contracts::RecordedCall;

/// Records the actual request order independently of the keyed response scripts.
struct RecordingRunner<'runner> {
    calls: RefCell<Vec<RecordedCall>>,
    inner: &'runner ScriptedRunner,
}

impl Runner for RecordingRunner<'_> {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        self.calls.borrow_mut().push(output_call(bin, args, cwd));
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
            .push(status_call(bin, args, envs, quiet, cwd));
        self.inner.status(bin, args, envs, quiet, cwd)
    }
}

/// Captures the public streams while refusing one selected stdout write.
struct OutputFailure {
    calls: RefCell<usize>,
    fail_at: usize,
    inner: TestIo,
}

impl Io for OutputFailure {
    fn err(&self, text: &str) -> io::Result<()> {
        self.inner.err(text)
    }
    fn errln(&self, line: &str) -> io::Result<()> {
        self.inner.errln(line)
    }
    fn out(&self, text: &str) -> io::Result<()> {
        let call = self
            .calls
            .borrow()
            .checked_add(1)
            .or_abort("bounded output count");
        *self.calls.borrow_mut() = call;
        if call == self.fail_at {
            return Err(io::Error::other("selected write refused"));
        }
        self.inner.out(text)
    }
    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }
}

fn output_call(bin: &str, args: &[&str], cwd: &Path) -> RecordedCall {
    RecordedCall::Output {
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        bin: bin.to_owned(),
        cwd: cwd.to_path_buf(),
    }
}

fn status_call(
    bin: &str,
    args: &[&str],
    envs: &[(&str, &str)],
    quiet: bool,
    cwd: &Path,
) -> RecordedCall {
    RecordedCall::Status {
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        bin: bin.to_owned(),
        cwd: cwd.to_path_buf(),
        envs: envs
            .iter()
            .map(|&(key, value)| (key.to_owned(), value.to_owned()))
            .collect(),
        quiet,
    }
}

fn directory_query(repo: &Path) -> RecordedCall {
    output_call("git", &["rev-parse", "--git-dir"], repo)
}

/// These attempted diagnostic queries are required even when their replies refuse.
fn diagnostic_requests(repo: &Path) -> Vec<RecordedCall> {
    vec![
        directory_query(repo),
        directory_query(repo),
        output_call("git", &["rev-parse", "--verify", "HEAD"], repo),
        output_call("git", &["rev-parse", "--verify", "HEAD^{tree}"], repo),
        directory_query(repo),
        output_call("git", &["rev-parse", "--show-toplevel"], repo),
        output_call(
            "git",
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

fn rebase_abort(repo: &Path) -> RecordedCall {
    status_call(
        "git",
        &["rebase", "--abort"],
        &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
        false,
        repo,
    )
}

fn reset(repo: &Path, target: &str) -> RecordedCall {
    status_call(
        "git",
        &["reset", "--hard", "--quiet", target],
        &[],
        false,
        repo,
    )
}

fn clean(repo: &Path) -> RecordedCall {
    status_call(
        "git",
        &["clean", "--force", "--quiet", "-d"],
        &[],
        false,
        repo,
    )
}

/// The expected frontier comes from the selected ingress and output boundary, not scripts.
#[expect(
    clippy::single_call_fn,
    reason = "keeps the independently specified successful request frontier separate from response scripts and public stream expectations"
)]
fn successful_requests(
    repo: &Path,
    target: &str,
    started: bool,
    rebase: bool,
    fail_at: usize,
) -> Vec<RecordedCall> {
    let mut expected = vec![directory_query(repo), directory_query(repo)];
    if started {
        expected.push(directory_query(repo));
        if rebase {
            expected.push(rebase_abort(repo));
        }
    }
    expected.extend([reset(repo, target), clean(repo), directory_query(repo)]);
    if fail_at > 2 {
        expected.push(directory_query(repo));
    }
    expected
}

#[expect(
    clippy::single_call_fn,
    reason = "keeps the independently specified first failing native request frontier separate from its keyed response arrangement"
)]
fn native_refusal_requests(repo: &Path, sha: &str, operation: &str) -> Vec<RecordedCall> {
    let mut expected = vec![
        directory_query(repo),
        directory_query(repo),
        directory_query(repo),
        rebase_abort(repo),
    ];
    if operation != "rebase" {
        expected.push(reset(repo, sha));
    }
    if operation == "clean" {
        expected.push(clean(repo));
    }
    expected.extend(diagnostic_requests(repo));
    expected
}

fn journal(state: &Path) -> Vec<(OsString, Vec<u8>)> {
    let mut files = fs::read_dir(state)
        .or_abort("journal inventory")
        .map(|entry| {
            let file = entry.or_abort("journal entry");
            (
                file.file_name(),
                fs::read(file.path()).or_abort("journal bytes"),
            )
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn assert_retained(state: &Path, before: &[(OsString, Vec<u8>)], expected_error: &str) {
    let diagnostic =
        fs::read_to_string(state.join("error.log")).or_abort("public abort diagnostic");
    assert!(
        diagnostic
            .lines()
            .any(|line| line == "argv=git-factor --abort")
    );
    assert!(
        diagnostic
            .lines()
            .any(|line| line == format!("error={expected_error}"))
    );
    let mut after = journal(state);
    let entry = after
        .iter()
        .position(|entry| entry.0 == "error.log")
        .or_abort("error diagnostic");
    after.remove(entry);
    assert_eq!(after.as_slice(), before);
}

/// Exact saved-head selection and rebase routes reach public cleanup output.
#[expect(
    clippy::too_many_lines,
    reason = "keeps one public abort Act with its ingress, independently ordered requests, exact streams, cleanup, and unrelated-byte conservation visible together"
)]
pub(in crate::git_factor) fn successful(
    sha: &str,
    started: bool,
    rebase: bool,
    saved_head: bool,
    fail_at: usize,
) {
    let directory = TempDir::new().or_abort("public abort fixture");
    let repo = directory.path();
    let state = setup_factor_state(repo, sha, "1\n", Some("false\n"), Some(TREE_EXPECTED_NL));
    fs::write(repo.join(".git/unrelated"), b"user bytes\n").or_abort("unrelated Git metadata");
    let original = "b".repeat(SHA_LEN);
    fs::write(state.join("started_rebase"), format!("{started}\n"))
        .or_abort("saved replay ownership");
    if saved_head {
        fs::write(state.join("start_head"), format!("{original}\n")).or_abort("saved start head");
    }
    // The scripted abort is inert: existing rebase metadata remains observable.
    if rebase {
        fs::create_dir_all(repo.join(".git/rebase-merge")).or_abort("native metadata fixture");
    }
    let mut runner = ScriptedRunner::default();
    if started && rebase {
        runner = runner.with_status(
            "git",
            &["rebase", "--abort"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        );
    }
    runner = runner
        .with_status(
            "git",
            &[
                "reset",
                "--hard",
                "--quiet",
                if saved_head { &original } else { sha },
            ],
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
    let lines = if rebase {
        vec![
            "FACTOR: Session aborted for current commit step.",
            "FACTOR: Rebase still active. To abort full rebase, run: git rebase --abort",
        ]
    } else {
        vec!["FACTOR: Session aborted for current commit step."]
    };
    let writes = lines
        .iter()
        .flat_map(|line| [*line, "\n"])
        .collect::<Vec<_>>();
    let expected = if fail_at == 0 {
        writes.concat()
    } else {
        writes
            .get(..fail_at.checked_sub(1).or_abort("positive selected write"))
            .or_abort("selected frame write")
            .concat()
    };
    let io = OutputFailure {
        calls: RefCell::new(0),
        fail_at,
        inner: TestIo::default(),
    };
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let recording = RecordingRunner {
        calls: RefCell::new(Vec::new()),
        inner: &runner,
    };

    let code = main_entry_with_vec(
        &io,
        ctx_from_parts(&environment, &recording, &io, &REAL_FS),
        &[OsString::from("git-factor"), OsString::from("--abort")],
    );

    assert_eq!(
        *recording.calls.borrow(),
        successful_requests(
            repo,
            if saved_head { &original } else { sha },
            started,
            rebase,
            fail_at
        )
    );

    assert_eq!(code, if fail_at == 0 { EXIT_OK } else { EXIT_SOFTWARE });
    assert_eq!(io.inner.stdout(), expected);
    assert_eq!(
        io.inner.stderr(),
        if fail_at == 0 {
            ""
        } else {
            "failed to write output: selected write refused\n"
        }
    );
    assert!(!state.exists());
    assert!(runner.statuses.borrow().values().all(VecDeque::is_empty));
    assert_eq!(repo.join(".git/rebase-merge").exists(), rebase);
    assert_eq!(
        fs::read(repo.join(".git/unrelated")).or_abort("preserved unrelated Git metadata"),
        b"user bytes\n"
    );
}

/// No session refuses before cleanup, retaining arbitrary unrelated input.
#[expect(
    clippy::single_call_fn,
    reason = "keeps the public no-session refusal and user-input oracle together for its generated contract"
)]
pub(in crate::git_factor) fn inactive(path: &str) {
    let directory = TempDir::new().or_abort("inactive public abort");
    let repo = directory.path();
    fs::create_dir_all(repo.join(".git")).or_abort("Git discovery fixture");
    fs::write(repo.join(path), b"unrelated user bytes\n").or_abort("unrelated input");
    let runner = ScriptedRunner::default();
    let io = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let recording = RecordingRunner {
        calls: RefCell::new(Vec::new()),
        inner: &runner,
    };

    let code = main_entry_with_vec(
        &io,
        ctx_from_parts(&environment, &recording, &io, &REAL_FS),
        &[OsString::from("git-factor"), OsString::from("--abort")],
    );

    assert_eq!(*recording.calls.borrow(), vec![directory_query(repo)]);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(io.stdout(), "");
    assert_eq!(io.stderr(), "no active factor session\n");
    assert!(!repo.join(".git/factor").exists());
    assert_eq!(
        fs::read(repo.join(path)).or_abort("preserved unrelated input"),
        b"unrelated user bytes\n"
    );
}

/// Saved-field failures conserve authority and expose their exact public error.
#[expect(
    clippy::single_call_fn,
    reason = "keeps required-field refusal and saved-journal conservation together for its generated contract"
)]
pub(in crate::git_factor) fn read_refusal(sha: &str, key: &'static str) {
    let directory = TempDir::new().or_abort("public abort read refusal");
    let repo = directory.path();
    let state = setup_factor_state(repo, sha, "1\n", Some("false\n"), Some(TREE_EXPECTED_NL));
    fs::write(repo.join(".git/unrelated"), b"user bytes\n").or_abort("unrelated Git metadata");
    fs::write(state.join("started_rebase"), "false\n").or_abort("saved replay ownership");
    fs::write(state.join("start_head"), format!("{sha}\n")).or_abort("saved head");
    let before = journal(&state);
    let filesystem = NthReadFailureFs {
        fail_at: 1,
        file_name: key,
        message: "selected read refused",
        reads: Mutex::new(0),
    };
    let runner = ScriptedRunner::default();
    let io = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let recording = RecordingRunner {
        calls: RefCell::new(Vec::new()),
        inner: &runner,
    };

    let code = main_entry_with_vec(
        &io,
        ctx_from_parts(&environment, &recording, &io, &filesystem),
        &[OsString::from("git-factor"), OsString::from("--abort")],
    );

    let mut expected_requests = vec![directory_query(repo), directory_query(repo)];
    expected_requests.extend(diagnostic_requests(repo));
    assert_eq!(*recording.calls.borrow(), expected_requests);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(io.stdout(), "");
    assert_eq!(io.stderr(), "failed to read state: selected read refused\n");
    assert_retained(
        &state,
        &before,
        "failed to read state: selected read refused",
    );
    assert_eq!(
        fs::read(repo.join(".git/unrelated")).or_abort("preserved unrelated Git metadata"),
        b"user bytes\n"
    );
}

/// A selected failing native operation cannot report completed cleanup.
#[expect(
    clippy::single_call_fn,
    reason = "keeps selected native failure, exact public error, and journal conservation together"
)]
pub(in crate::git_factor) fn native_refusal(sha: &str, operation: &str, exit: i32) {
    // ScriptedRunner accepts the Unix wait-status representation, not a decoded exit.
    let wait_status = exit
        .checked_mul(256)
        .or_abort("generated exits are bounded by 255");
    let directory = TempDir::new().or_abort("public abort native refusal");
    let repo = directory.path();
    let state = setup_factor_state(repo, sha, "1\n", Some("false\n"), Some(TREE_EXPECTED_NL));
    fs::write(repo.join(".git/unrelated"), b"user bytes\n").or_abort("unrelated Git metadata");
    fs::write(state.join("started_rebase"), "true\n").or_abort("saved replay ownership");
    fs::write(state.join("start_head"), format!("{sha}\n")).or_abort("saved head");
    fs::create_dir_all(repo.join(".git/rebase-merge")).or_abort("native metadata fixture");
    let before = journal(&state);
    let mut runner = ScriptedRunner::default().with_status(
        "git",
        &["rebase", "--abort"],
        &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
        false,
        repo,
        if operation == "rebase" {
            wait_status
        } else {
            0
        },
    );
    if operation != "rebase" {
        runner = runner.with_status(
            "git",
            &["reset", "--hard", "--quiet", sha],
            &[],
            false,
            repo,
            if operation == "reset" { wait_status } else { 0 },
        );
    }
    if operation == "clean" {
        runner = runner.with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            wait_status,
        );
    }
    let io = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let recording = RecordingRunner {
        calls: RefCell::new(Vec::new()),
        inner: &runner,
    };

    let code = main_entry_with_vec(
        &io,
        ctx_from_parts(&environment, &recording, &io, &REAL_FS),
        &[OsString::from("git-factor"), OsString::from("--abort")],
    );

    assert_eq!(
        *recording.calls.borrow(),
        native_refusal_requests(repo, sha, operation)
    );

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(io.stdout(), "");
    assert_eq!(
        io.stderr(),
        format!("git command failed: git {operation} failed (exit {exit})\n")
    );
    assert_retained(
        &state,
        &before,
        &format!("git command failed: git {operation} failed (exit {exit})"),
    );
    assert!(runner.statuses.borrow().values().all(VecDeque::is_empty));
    assert_eq!(
        fs::read(repo.join(".git/unrelated")).or_abort("preserved unrelated Git metadata"),
        b"user bytes\n"
    );
}

/// Both cleanup failures preserve the saved session rather than acknowledge abort.
#[expect(
    clippy::single_call_fn,
    reason = "keeps cleanup refusal, native script exhaustion, and saved-journal conservation together"
)]
pub(in crate::git_factor) fn cleanup_refusal(sha: &str, leaves_path: bool) {
    let directory = TempDir::new().or_abort("public abort cleanup refusal");
    let repo = directory.path();
    let state = setup_factor_state(repo, sha, "1\n", Some("false\n"), Some(TREE_EXPECTED_NL));
    fs::write(repo.join(".git/unrelated"), b"user bytes\n").or_abort("unrelated Git metadata");
    let before = journal(&state);
    let runner = ScriptedRunner::default()
        .with_status(
            "git",
            &["reset", "--hard", "--quiet", sha],
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
    let filesystem: &dyn Fs = if leaves_path {
        &StickyStatePathFs
    } else {
        &FailingRemoveDirAllFs
    };
    let io = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let expected = if leaves_path {
        format!(
            "git command failed: factor state path '{}' still exists after cleanup\n",
            state.display()
        )
    } else {
        format!(
            "git command failed: failed to remove factor state path '{}': injected remove_dir_all failure\n",
            state.display()
        )
    };

    let recording = RecordingRunner {
        calls: RefCell::new(Vec::new()),
        inner: &runner,
    };

    let code = main_entry_with_vec(
        &io,
        ctx_from_parts(&environment, &recording, &io, filesystem),
        &[OsString::from("git-factor"), OsString::from("--abort")],
    );

    let mut expected_requests = vec![
        directory_query(repo),
        directory_query(repo),
        reset(repo, sha),
        clean(repo),
    ];
    expected_requests.extend(diagnostic_requests(repo));
    assert_eq!(*recording.calls.borrow(), expected_requests);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(io.stdout(), "");
    assert_eq!(io.stderr(), expected);
    assert_retained(
        &state,
        &before,
        expected
            .strip_suffix('\n')
            .or_abort("expected error has one reporting newline"),
    );
    assert!(runner.statuses.borrow().values().all(VecDeque::is_empty));
    assert_eq!(
        fs::read(repo.join(".git/unrelated")).or_abort("preserved unrelated Git metadata"),
        b"user bytes\n"
    );
}
