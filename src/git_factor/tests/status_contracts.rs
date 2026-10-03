use super::*;

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
        fs::read_to_string(state.join("error.log")).or_abort("public status diagnostic");
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
    let mut after = journal(state);
    let entry = after
        .iter()
        .position(|entry| entry.0 == "error.log")
        .or_abort("error diagnostic");
    after.remove(entry);
    assert_eq!(after.as_slice(), before);
}

/// Exercises exact public JSON status frames, including every output write.
pub(in crate::git_factor) fn active(
    sha: &str,
    split: u8,
    requires: bool,
    root: bool,
    phase: &str,
    rebase: bool,
    fail_at: usize,
) {
    let directory = TempDir::new().or_abort("public status fixture");
    let repo = directory.path();
    fs::write(repo.join("unrelated"), b"user bytes\n").or_abort("unrelated status input");
    let state = setup_factor_state(
        repo,
        sha,
        &format!("{split}\n"),
        Some(if requires { "true\n" } else { "false\n" }),
        Some(TREE_EXPECTED_NL),
    );
    fs::write(state.join("is_root"), format!("{root}\n")).or_abort("saved root");
    fs::write(state.join("phase"), format!("{phase}\n")).or_abort("saved phase");
    if rebase {
        fs::create_dir_all(repo.join(".git/rebase-merge")).or_abort("native metadata fixture");
    }
    let before = journal(&state);
    let json = format!(
        concat!(
            "{{\"operation\":\"status\",\"session\":{{\"phase\":\"{}\",",
            "\"rebase\":{{\"in_progress\":{},\"required\":{}}},\"split_count\":{},",
            "\"target\":{{\"commit\":\"{}\",\"index\":0,\"span_starts_at_root\":{}}}}}}}"
        ),
        phase, rebase, requires, split, sha, root,
    );
    let writes = [json.as_str(), "\n"];
    let expected = if fail_at == 0 {
        writes.concat()
    } else {
        writes
            .get(..fail_at.checked_sub(1).or_abort("positive selected write"))
            .or_abort("selected JSON write")
            .concat()
    };
    let runner = ScriptedRunner::default();
    let io = OutputFailure {
        calls: RefCell::new(0),
        fail_at,
        inner: TestIo::default(),
    };
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let code = main_entry_with_vec(
        &io,
        ctx_from_parts(&environment, &runner, &io, &REAL_FS),
        &[OsString::from("git-factor"), OsString::from("--status")],
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
    if fail_at == 0 {
        assert_eq!(journal(&state), before);
    } else {
        assert_retained(
            &state,
            &before,
            "failed to write output: selected write refused",
        );
    }
    assert_eq!(repo.join(".git/rebase-merge").exists(), rebase);
    assert_eq!(
        fs::read(repo.join("unrelated")).or_abort("preserved unrelated status input"),
        b"user bytes\n"
    );
}

/// Inactive observations never create a factor session, even on output refusal.
#[expect(
    clippy::single_call_fn,
    reason = "keeps the inactive public frame and conservation oracle together for its generated contract"
)]
pub(in crate::git_factor) fn inactive(path: &str, fail_at: usize) {
    let directory = TempDir::new().or_abort("inactive public status fixture");
    let repo = directory.path();
    fs::create_dir_all(repo.join(".git")).or_abort("Git discovery fixture");
    fs::write(repo.join(path), b"unrelated user bytes\n").or_abort("unrelated input");
    let expected = *[
        "{\"operation\":\"status\",\"session\":null}\n",
        "",
        "{\"operation\":\"status\",\"session\":null}",
    ]
    .get(fail_at)
    .or_abort("generator admits only success or two frame writes");
    let runner = ScriptedRunner::default();
    let io = OutputFailure {
        calls: RefCell::new(0),
        fail_at,
        inner: TestIo::default(),
    };
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let code = main_entry_with_vec(
        &io,
        ctx_from_parts(&environment, &runner, &io, &REAL_FS),
        &[OsString::from("git-factor"), OsString::from("--status")],
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
    assert!(!repo.join(".git/factor").exists());
    assert_eq!(
        fs::read(repo.join(path)).or_abort("preserved unrelated input"),
        b"unrelated user bytes\n"
    );
}

/// Every required saved-field read refuses before any successful status frame.
#[expect(
    clippy::single_call_fn,
    reason = "keeps the saved-field refusal and journal oracle together for its generated contract"
)]
pub(in crate::git_factor) fn read_refusal(sha: &str, key: &'static str, occurrence: usize) {
    let directory = TempDir::new().or_abort("public status read refusal");
    let repo = directory.path();
    fs::write(repo.join("unrelated"), b"user bytes\n").or_abort("unrelated status input");
    let state = setup_factor_state(repo, sha, "1\n", Some("false\n"), Some(TREE_EXPECTED_NL));
    fs::write(state.join("is_root"), "false\n").or_abort("saved root");
    fs::write(state.join("phase"), "splitting\n").or_abort("saved phase");
    let before = journal(&state);
    let filesystem = NthReadFailureFs {
        fail_at: occurrence,
        file_name: key,
        message: "selected read refused",
        reads: Mutex::new(0),
    };
    let runner = ScriptedRunner::default();
    let io = TestIo::default();
    let environment = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let code = main_entry_with_vec(
        &io,
        ctx_from_parts(&environment, &runner, &io, &filesystem),
        &[OsString::from("git-factor"), OsString::from("--status")],
    );

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(io.stdout(), "");
    assert_eq!(io.stderr(), "failed to read state: selected read refused\n");
    assert_retained(
        &state,
        &before,
        "failed to read state: selected read refused",
    );
    assert_eq!(
        fs::read(repo.join("unrelated")).or_abort("preserved unrelated status input"),
        b"user bytes\n"
    );
}
