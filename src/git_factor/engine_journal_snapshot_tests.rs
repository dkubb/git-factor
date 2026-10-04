//! Non-authorizing journal observations through the actual diagnostic API.
use super::*;
use crate::git_factor::{CommitSha, REAL_ENV, TreeHash};

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::engine) enum PhaseInput {
    Closing,
    Opening,
    Preparing,
    Replaying,
    Selecting,
    Verified,
}
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::engine) enum Input {
    Absent,
    Current(PhaseInput),
    Malformed,
    Unreadable,
}

impl PhaseInput {
    fn recorded(self, commit: &str) -> (&'static str, Option<&str>, String) {
        // The caller supplies checked hexadecimal object identifiers, so no JSON escaping is required.
        match self {
            Self::Closing => (
                "closing",
                None,
                format!(
                    r#"{{"phase":"closing","lease":"{commit}","outcome":{{"result":"complete","atom":"{commit}"}}}}"#
                ),
            ),
            Self::Opening => (
                "opening",
                Some(commit),
                format!(
                    r#"{{"phase":"opening","lease":"{commit}","source":"{commit}","anchor":"{commit}","base":null,"head":"{commit}"}}"#
                ),
            ),
            Self::Preparing => (
                "preparing",
                None,
                format!(
                    r#"{{"phase":"preparing","tip":"{commit}","base":null,"previous_lease":null}}"#
                ),
            ),
            Self::Replaying => (
                "replaying",
                Some(commit),
                format!(
                    r#"{{"phase":"replaying","lease":"{commit}","source":"{commit}","anchor":"{commit}","base":null,"head":"{commit}","atom":"{commit}","remainder":{{"state":"absent"}},"accepted":[]}}"#
                ),
            ),
            Self::Selecting => (
                "selecting",
                Some(commit),
                format!(
                    r#"{{"phase":"selecting","lease":"{commit}","source":"{commit}","anchor":"{commit}","base":null,"head":"{commit}"}}"#
                ),
            ),
            Self::Verified => (
                "verified",
                None,
                format!(
                    r#"{{"phase":"verified","lease":"{commit}","atom":"{commit}","anchor":"{commit}","base":null,"tip":"{commit}","remainder":{{"state":"absent"}}}}"#
                ),
            ),
        }
    }
}

pub(in crate::git_factor::engine) struct NoNativeQueries {
    calls: Cell<usize>,
}
impl Runner for NoNativeQueries {
    fn output(
        &self,
        _bin: &str,
        _args: &[&str],
        _envs: &[(&str, Option<&str>)],
        _cwd: &Path,
    ) -> io::Result<Output> {
        self.calls.set(
            self.calls
                .get()
                .checked_add(1)
                .or_abort("bounded observation count"),
        );
        Err(io::Error::other("journal projection must not query Git"))
    }
    fn status(
        &self,
        _bin: &str,
        _args: &[&str],
        _envs: &[(&str, Option<&str>)],
        _quiet: bool,
        _cwd: &Path,
    ) -> io::Result<ExitStatus> {
        self.calls.set(
            self.calls
                .get()
                .checked_add(1)
                .or_abort("bounded observation count"),
        );
        Err(io::Error::other("journal projection must not mutate Git"))
    }
}

pub(in crate::git_factor::engine) struct Fixture {
    directory: TempDir,
    expected: Option<super::super::JournalFacts>,
    metadata: Option<Vec<u8>>,
    output: Capture,
    runner: NoNativeQueries,
}
impl Fixture {
    pub(in crate::git_factor::engine) fn arrange(input: Input, commit: &str, tree: &str) -> Self {
        let directory = TempDir::new().or_abort("journal observation fixture");
        let git_dir = directory.path().join(".git");
        fs::create_dir_all(&git_dir).or_abort("Git observation directory");
        fs::write(directory.path().join("user"), b"protected user bytes\n").or_abort("user bytes");
        let path = git_dir.join("factor-journal.json");
        let expected = match input {
            Input::Absent => None,
            Input::Malformed => {
                fs::write(&path, b"{malformed}").or_abort("malformed journal");
                None
            }
            Input::Unreadable => {
                fs::create_dir_all(&path).or_abort("unreadable journal path");
                None
            }
            Input::Current(phase) => {
                let (name, source, state) = phase.recorded(commit);
                let journal = format!(
                    r#"{{"branch":"refs/heads/main","checkpoint":"{commit}","final_tree":"{tree}","format":"checkpoint_v2","gates":[],"original_base":null,"original_tip":"{commit}","session":"{commit}","state":{state}}}"#
                );
                fs::write(&path, journal).or_abort("recorded journal");
                Some(super::super::JournalFacts {
                    checkpoint: CommitSha::new(commit.to_owned())
                        .or_abort("valid generated commit"),
                    final_tree: TreeHash::new(tree).or_abort("valid generated tree"),
                    phase: name,
                    source: source.map(|value| {
                        CommitSha::new(value.to_owned()).or_abort("valid recorded source")
                    }),
                })
            }
        };
        let metadata = match input {
            Input::Absent | Input::Unreadable => None,
            Input::Current(_) | Input::Malformed => {
                Some(fs::read(&path).or_abort("recorded metadata"))
            }
        };
        Self {
            directory,
            expected,
            metadata,
            output: Capture::default(),
            runner: NoNativeQueries {
                calls: Cell::new(0),
            },
        }
    }
    pub(in crate::git_factor::engine) fn calls(&self) -> usize {
        self.runner.calls.get()
    }
    pub(in crate::git_factor::engine) fn context(&self) -> Ctx<'_> {
        Ctx {
            cwd: self.directory.path().to_path_buf(),
            fs: &REAL_FS,
            env: &REAL_ENV,
            io: &self.output,
            runner: &self.runner,
        }
    }
    pub(in crate::git_factor::engine) fn expected(&self) -> Option<&super::super::JournalFacts> {
        self.expected.as_ref()
    }
    pub(in crate::git_factor::engine) fn metadata(&self) -> Option<&Vec<u8>> {
        self.metadata.as_ref()
    }

    pub(in crate::git_factor::engine) fn stderr(&self) -> String {
        self.output.stderr.borrow().clone()
    }
    pub(in crate::git_factor::engine) fn stdout(&self) -> String {
        self.output.stdout.borrow().clone()
    }
}
