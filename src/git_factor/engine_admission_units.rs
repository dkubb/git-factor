#[derive(Debug, Eq, PartialEq)]
struct AdmissionFrame {
    atom: Option<Vec<u8>>,
    config: Vec<u8>,
    done: Option<Vec<u8>>,
    head: String,
    index: Vec<u8>,
    journal: Option<Vec<u8>>,
    refs: String,
    todo: Option<Vec<u8>>,
    user: Option<Vec<u8>>,
}

struct FailedSourceMessage<'fixture> {
    before: super::RefCell<Option<AdmissionFrame>>,
    later_constructions: super::Cell<usize>,
    repository: &'fixture super::Repository,
    source: String,
    status: super::ExitStatus,
}

impl super::Runner for FailedSourceMessage<'_> {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &super::Path,
    ) -> super::io::Result<super::Output> {
        let mut output = super::REAL_RUNNER.output(bin, args, envs, cwd)?;
        if self.before.borrow().is_none()
            && self
                .repository
                .environment
                .cwd
                .join(".git/factor-journal.json")
                .is_file()
            && bin == "git"
            && args == ["cat-file", "commit", self.source.as_str()]
        {
            if !output.status.success() || !output.stdout.windows(2).any(|pair| pair == b"\n\n") {
                return Err(super::io::Error::other(
                    "fixture source must contain a successful complete native commit",
                ));
            }
            *self.before.borrow_mut() = Some(admission_frame(self.repository));
            output.status = self.status;
        }
        if self.before.borrow().is_some() && args.first() == Some(&"commit-tree") {
            self.later_constructions.set(
                self.later_constructions
                    .get()
                    .checked_add(1)
                    .or_abort("construction counter"),
            );
        }
        Ok(output)
    }
    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        quiet: bool,
        cwd: &super::Path,
    ) -> super::io::Result<super::ExitStatus> {
        super::REAL_RUNNER.status(bin, args, envs, quiet, cwd)
    }
}

fn admission_frame(repository: &super::Repository) -> AdmissionFrame {
    let root = &repository.environment.cwd;
    let index = repository.index();
    AdmissionFrame {
        index,
        journal: fs::read(root.join(".git/factor-journal.json")).ok(),
        head: repository.git(&["rev-parse", "HEAD"]),
        refs: repository.git(&["show-ref"]),
        config: fs::read(root.join(".git/config")).or_abort("native config"),
        done: fs::read(root.join(".git/rebase-merge/done")).ok(),
        todo: fs::read(root.join(".git/rebase-merge/git-rebase-todo")).ok(),
        atom: fs::read(root.join("atom")).ok(),
        user: fs::read(root.join("user")).ok(),
    }
}


#[test]
fn rejects_failed_source_message_before_checkpoint_construction() {
    use std::os::unix::process::ExitStatusExt as _;
    let repository = super::Repository::new();
    repository.write("atom", "original tree\n");
    repository.commit("Add original tree");
    repository.write("atom", "selected tree\n");
    let source = repository.commit("Change selected tree");
    repository.write("user", "protected bytes\0\n");
    let failed_code: i32 = 42;
    let runner = FailedSourceMessage {
        repository: &repository,
        source,
        status: super::ExitStatus::from_raw(
            failed_code
                .checked_shl(8)
                .or_abort("native status shift count is below i32 width"),
        ),
        before: super::RefCell::new(None),
        later_constructions: super::Cell::new(0),
    };
    let context = super::Ctx {
        cwd: repository.environment.cwd.clone(),
        env: &repository.environment,
        fs: &super::REAL_FS,
        io: &repository.output,
        runner: &runner,
    };
    let arguments = ["git-factor", "--exec", "true", "HEAD"].map(super::OsString::from);

    let code = super::main_entry_with_vec(context.io, Ok(context), &arguments);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(*repository.output.stdout.borrow(), "");
    assert_eq!(
        *repository.output.stderr.borrow(),
        "git command failed: cannot observe source message\n"
    );
    assert_eq!(runner.later_constructions.get(), 0);
    assert_eq!(
        runner.before.borrow().as_ref(),
        Some(&admission_frame(&repository))
    );
    let journal: super::super::StoredJournal =
        serde_json::from_slice(&repository.journal()).or_abort("actual preparing journal");
    assert!(matches!(
        journal.state,
        super::super::Phase::Preparing { .. }
    ));
}

#[test]
fn rejects_signaled_source_message_before_checkpoint_construction() {
    use std::os::unix::process::ExitStatusExt as _;
    let repository = super::Repository::new();
    repository.write("atom", "original tree\n");
    repository.commit("Add original tree");
    repository.write("atom", "selected tree\n");
    let source = repository.commit("Change selected tree");
    repository.write("user", "protected bytes\0\n");
    let runner = FailedSourceMessage {
        repository: &repository,
        source,
        status: super::ExitStatus::from_raw(15),
        before: super::RefCell::new(None),
        later_constructions: super::Cell::new(0),
    };
    let context = super::Ctx {
        cwd: repository.environment.cwd.clone(),
        env: &repository.environment,
        fs: &super::REAL_FS,
        io: &repository.output,
        runner: &runner,
    };
    let arguments = ["git-factor", "--exec", "true", "HEAD"].map(super::OsString::from);

    let code = super::main_entry_with_vec(context.io, Ok(context), &arguments);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(*repository.output.stdout.borrow(), "");
    assert_eq!(
        *repository.output.stderr.borrow(),
        "git command failed: cannot observe source message\n"
    );
    assert_eq!(runner.later_constructions.get(), 0);
    assert_eq!(
        runner.before.borrow().as_ref(),
        Some(&admission_frame(&repository))
    );
    let journal: super::super::StoredJournal =
        serde_json::from_slice(&repository.journal()).or_abort("actual preparing journal");
    assert!(matches!(
        journal.state,
        super::super::Phase::Preparing { .. }
    ));
}

#[test]
fn refuses_merge_descendant_before_publishing_a_session() {
    let repository = super::Repository::new();
    repository.write("atom", "original tree\n");
    repository.commit("Add original tree");
    repository.write("atom", "selected tree\n");
    let selected = repository.commit("Change selected tree");
    repository.git(&["checkout", "--quiet", "-b", "side"]);
    repository.write("side", "side branch\n");
    repository.commit("Add side file");
    repository.git(&["checkout", "--quiet", "main"]);
    repository.write("main", "main branch\n");
    repository.commit("Add main file");
    repository.git(&[
        "merge",
        "--no-ff",
        "--quiet",
        "--message",
        "Merge side branch",
        "side",
    ]);
    repository.write("user", "protected bytes\0\n");
    let marker = repository.directory.path().join("gate-ran");
    let gate = format!(
        "touch {}",
        super::shell_quote(marker.to_str().or_abort("gate observation path"))
    );
    let before = admission_frame(&repository);
    let context = super::Ctx {
        cwd: repository.environment.cwd.clone(),
        env: &repository.environment,
        fs: &super::REAL_FS,
        io: &repository.output,
        runner: &super::REAL_RUNNER,
    };
    let arguments = ["git-factor", "--exec", &gate, &selected].map(super::OsString::from);

    let code = super::main_entry_with_vec(context.io, Ok(context), &arguments);

    assert_eq!(admission_frame(&repository), before);
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(*repository.output.stdout.borrow(), "");
    assert_eq!(
        *repository.output.stderr.borrow(),
        "git command failed: attempt path corridor contains a merge\n"
    );
    assert!(!marker.exists());
    assert!(!repository.environment.cwd.join(".git/factor").exists());
}

#[test]
fn refuses_newline_callback_path_before_publishing_a_session() {
    let mut repository = super::Repository::new();
    repository.write("atom", "original tree\n");
    repository.commit("Add original tree");
    repository.write("atom", "selected tree\n");
    repository.commit("Change selected tree");
    repository.write("user", "protected bytes\0\n");
    let executable = repository.directory.path().join("factor\nchild");
    fs::rename(&repository.environment.executable, &executable).or_abort("real newline executable");
    repository.environment.executable = executable;
    let before = admission_frame(&repository);
    let context = super::Ctx {
        cwd: repository.environment.cwd.clone(),
        env: &repository.environment,
        fs: &super::REAL_FS,
        io: &repository.output,
        runner: &super::REAL_RUNNER,
    };
    let arguments = ["git-factor", "--exec", "true", "HEAD"].map(super::OsString::from);

    let code = super::main_entry_with_vec(context.io, Ok(context), &arguments);

    assert_eq!(admission_frame(&repository), before);
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(*repository.output.stdout.borrow(), "");
    assert_eq!(
        *repository.output.stderr.borrow(),
        "git command failed: executable path contains a newline\n"
    );
    assert!(!repository.environment.cwd.join(".git/factor").exists());
}

#[test]
fn refuses_relocated_selecting_continue_before_mutation() {
    let mut fixture = super::MessageSelection::arrange(true);
    let moved = fixture.repository.directory.path().join("moved-factor");
    fs::copy(&fixture.repository.environment.executable, &moved)
        .or_abort("actual relocated callback image");
    fixture.repository.environment.executable = moved;
    let before = admission_frame(&fixture.repository);
    let arguments = ["git-factor", "--continue"].map(super::OsString::from);

    let code = super::main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(admission_frame(&fixture.repository), before);
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: native replay contains a foreign exec payload\n"
    );
}

#[test]
fn refuses_relocated_selecting_retry_before_mutation() {
    let mut fixture = super::MessageSelection::arrange(true);
    let moved = fixture.repository.directory.path().join("moved-factor");
    fs::copy(&fixture.repository.environment.executable, &moved)
        .or_abort("actual relocated callback image");
    fixture.repository.environment.executable = moved;
    let before = admission_frame(&fixture.repository);
    let arguments = ["git-factor", "--retry"].map(super::OsString::from);

    let code = super::main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(admission_frame(&fixture.repository), before);
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: native replay contains a foreign exec payload\n"
    );
}

#[test]
fn refuses_relocated_selecting_submit_before_mutation() {
    let mut fixture = super::MessageSelection::arrange(true);
    let moved = fixture.repository.directory.path().join("moved-factor");
    fs::copy(&fixture.repository.environment.executable, &moved)
        .or_abort("actual relocated callback image");
    fixture.repository.environment.executable = moved;
    let before = admission_frame(&fixture.repository);
    let arguments = ["git-factor", "--message", "Add selected tree"].map(super::OsString::from);

    let code = super::main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(admission_frame(&fixture.repository), before);
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: native replay contains a foreign exec payload\n"
    );
}

#[test]
fn refuses_relocated_selecting_finish_before_mutation() {
    let mut fixture = super::MessageSelection::arrange(true);
    let moved = fixture.repository.directory.path().join("moved-factor");
    fs::copy(&fixture.repository.environment.executable, &moved)
        .or_abort("actual relocated callback image");
    fixture.repository.environment.executable = moved;
    let before = admission_frame(&fixture.repository);
    let arguments = ["git-factor", "--finish"].map(super::OsString::from);

    let code = super::main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(admission_frame(&fixture.repository), before);
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: native replay contains a foreign exec payload\n"
    );
}

#[test]
fn refuses_unconsumed_selecting_break_before_resume() {
    let fixture = super::MessageSelection::arrange(true);
    let directory = fixture.repository.environment.cwd.join(".git/rebase-merge");
    let done = fs::read_to_string(directory.join("done")).or_abort("actual native done");
    let mut completed = String::new();
    for line in done
        .lines()
        .filter(|line| !matches!(line.trim(), "break" | "b"))
    {
        completed.push_str(line);
        completed.push('\n');
    }
    let todo = fs::read_to_string(directory.join("git-rebase-todo")).or_abort("actual native todo");
    fs::write(directory.join("done"), completed).or_abort("external unconsumed break world");
    fs::write(directory.join("git-rebase-todo"), format!("break\n{todo}"))
        .or_abort("external pending break world");
    let before = admission_frame(&fixture.repository);
    let arguments = ["git-factor", "--continue"].map(super::OsString::from);

    let code = super::main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(admission_frame(&fixture.repository), before);
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: selection has not retained its consumed source break\n"
    );
}
