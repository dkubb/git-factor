use super::*;

#[derive(Default)]
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "the canonical property observes the narrowly shared creation ledger"
)]
pub(in crate::git_factor) struct RefusingStateCreationFs {
    pub(in crate::git_factor) attempts: RefCell<Vec<PathBuf>>,
}

impl Fs for RefusingStateCreationFs {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        REAL_FS.canonicalize(path)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.attempts.borrow_mut().push(path.to_path_buf());
        Err(io::Error::other("factor state creation refused"))
    }

    fn exists(&self, path: &Path) -> bool {
        REAL_FS.exists(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        REAL_FS.is_dir(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        REAL_FS.read_to_string(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_file(path)
    }

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        REAL_FS.write_string(path, content)
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "the closed bootstrap fault world supplies only consumed Git prerequisites"
)]
pub(in crate::git_factor) fn direct_start(
    selected: &NonEmpty<CommitSha>,
    root: bool,
    stdout: &str,
    stderr: &str,
) -> DirectStart {
    let dir = TempDir::new().or_abort("bootstrap fixture tempdir");
    let repo = dir.path();
    fs::create_dir_all(repo.join(".git")).or_abort("journal parent fixture");
    fs::write(repo.join("direct-write-sentinel"), b"sentinel\n").or_abort("existing sentinel");
    let runner = ObservedRunner::admission(repo, selected, root, true, stdout, stderr).with_output(
        "git",
        &["status", "--porcelain=v1"],
        repo,
        "",
    );
    let expected_calls = runner.expectations();
    let direct_files_before = DirectStart::direct_files(repo, &repo.join(".git/factor"));
    DirectStart {
        env: TestEnv {
            cwd: repo.to_path_buf(),
        },
        state: StateDir::new(repo.join(".git/factor")),
        selected: selected.clone(),
        exec: NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("bootstrap gate")),
        io: CapturedIo {
            stdout: RefCell::new(String::new()),
            stderr: RefCell::new(String::new()),
            writes: Cell::new(0),
            fail_at: None,
        },
        _dir: dir,
        runner,
        expected_result: Err("failed to write state: factor state creation refused".to_owned()),
        expected_calls,
        expected_journal: None,
        expected_stdout: stdout.to_owned(),
        expected_stderr: stderr.to_owned(),
        direct_files_before,
    }
}
