use super::*;

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor) enum NoPause {
    Failed,
    Successful,
}

struct RefusedJournalRemoval {
    attempts: RefCell<Vec<PathBuf>>,
    factor: PathBuf,
}

impl Fs for RefusedJournalRemoval {
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
        REAL_FS.read_to_string(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        self.attempts.borrow_mut().push(path.to_path_buf());
        if path == self.factor {
            Err(io::Error::other("selected factor removal denied"))
        } else {
            REAL_FS.remove_dir_all(path)
        }
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_file(path)
    }
    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        REAL_FS.write_string(path, content)
    }
}

#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "canonical contracts read arrangement facts; removal capability stays private"
)]
pub(in crate::git_factor) struct CleanupStart {
    fs: RefusedJournalRemoval,
    pub(in crate::git_factor) head: CommitSha,
    pub(in crate::git_factor) replay: ReplayStart,
    pub(in crate::git_factor) span: CommitSpan,
}

impl CleanupStart {
    pub(in crate::git_factor) fn ctx(&self) -> Ctx<'_> {
        let mut ctx = self.replay.ctx();
        ctx.fs = &self.fs;
        ctx
    }

    pub(in crate::git_factor) fn journal_at_begin(&self) -> BTreeMap<OsString, Vec<u8>> {
        self.replay
            .runner
            .bootstrap
            .journal
            .as_ref()
            .or_abort("populated begin journal arrangement")
            .iter()
            .map(|(name, text)| (name.clone(), text.as_bytes().to_vec()))
            .collect()
    }

    pub(in crate::git_factor) fn journal_bytes(&self) -> Option<BTreeMap<OsString, Vec<u8>>> {
        let entries = match fs::read_dir(self.replay.state.as_path()) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
            other => other.or_abort("cleanup journal observation"),
        };
        Some(
            entries
                .map(|item| {
                    let entry = item.or_abort("cleanup journal entry");
                    let bytes = fs::read(entry.path()).or_abort("cleanup journal bytes");
                    (entry.file_name(), bytes)
                })
                .collect(),
        )
    }

    pub(in crate::git_factor) fn removal_attempts(&self) -> Vec<PathBuf> {
        self.fs.attempts.borrow().clone()
    }
}

pub(in crate::git_factor) fn direct_launcher(
    selected: &NonEmpty<CommitSha>,
    root: bool,
    outcome: NoPause,
) -> CleanupStart {
    let dir = TempDir::new().or_abort("cleanup fixture tempdir");
    let repo = dir.path();
    fs::create_dir_all(repo.join(".git")).or_abort("cleanup journal parent");
    fs::write(repo.join("direct-write-sentinel"), b"sentinel\n")
        .or_abort("cleanup existing sentinel");
    let head_text = "0123456789abcdef"
        .chars()
        .map(|digit| digit.to_string().repeat(SHA_LEN))
        .find(|head| selected.iter().all(|sha| sha.as_str() != head))
        .or_abort("bounded selection leaves a distinct HEAD");
    let head = CommitSha::new(head_text).or_abort("admitted HEAD");
    let case = match outcome {
        NoPause::Failed => ReplayCase::FailedWithoutPause,
        NoPause::Successful => ReplayCase::FinishedWithoutPause,
    };
    // Only the direct launcher's requests are queued; no public admission or hidden SUT Act.
    let (inner, command) = rebase_script(
        ObservedRunner::default(),
        repo,
        selected,
        head.as_str(),
        root,
        &case,
    );
    let state = StateDir::new(repo.join(".git/factor"));
    let input_journal = pending_journal(selected, root, &head, &case);
    let expected_calls = inner.expectations();
    let direct_files_before = direct_files(repo, &state);
    let replay = ReplayStart {
        env: TestEnv {
            cwd: repo.to_path_buf(),
        },
        exec: NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("cleanup gate")),
        expected_calls,
        expected_journal: Some(input_journal.clone()),
        expected_result: Err(format!(
            "git command failed: failed to remove factor state path '{}': \
             selected factor removal denied",
            state.as_path().display(),
        )),
        expected_stdout: String::new(),
        io: CapturedIo {
            stdout: RefCell::new(String::new()),
            stderr: RefCell::new(String::new()),
            writes: Cell::new(0),
            fail_at: None,
        },
        runner: ReplayRunner {
            bootstrap: ReplayBootstrap {
                command,
                journal: Some(input_journal),
                native_flag: None,
                state: state.as_path().to_path_buf(),
            },
            inner,
        },
        direct_files_before,
        selected: selected.clone(),
        state,
        _dir: dir,
    };
    CleanupStart {
        fs: RefusedJournalRemoval {
            attempts: RefCell::new(Vec::new()),
            factor: replay.state.as_path().to_path_buf(),
        },
        head,
        span: CommitSpan::new(
            selected.clone(),
            if root {
                BaseParent::Root
            } else {
                BaseParent::Commit
            },
        ),
        replay,
    }
}
