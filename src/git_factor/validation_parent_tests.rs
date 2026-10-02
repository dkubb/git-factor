use super::*;
use core::cell::{Cell, RefCell};
use proptest::prelude::*;
use std::os::unix::process::ExitStatusExt as _;

const ENV: ParentEnv = ParentEnv;
const FS: RefusingFs = RefusingFs;

/// Closed process responses supplied by the arranged object query.
#[derive(Clone, Debug)]
pub(in crate::git_factor::validation) enum Reply {
    Output {
        code: i32,
        content: String,
        error: String,
    },
    SpawnError(String),
}

impl Reply {
    /// Arranges a successful object response.
    pub(in crate::git_factor::validation) fn text(content: String) -> Self {
        Self::Output {
            code: 0,
            content,
            error: String::new(),
        }
    }
}

/// Captures output without granting access to process stdout or stderr.
#[derive(Default)]
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "canonical sibling contracts observe captured streams"
)]
pub(in crate::git_factor::validation) struct TestIo {
    pub(in crate::git_factor::validation) err: RefCell<String>,
    pub(in crate::git_factor::validation) out: RefCell<String>,
}

impl Io for TestIo {
    fn err(&self, text: &str) -> io::Result<()> {
        self.err.borrow_mut().push_str(text);
        Ok(())
    }
    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(line)?;
        self.err("\n")
    }
    fn out(&self, text: &str) -> io::Result<()> {
        self.out.borrow_mut().push_str(text);
        Ok(())
    }
    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }
}

struct RefusingFs;

impl Fs for RefusingFs {
    fn canonicalize(&self, _path: &Path) -> io::Result<PathBuf> {
        Err(io::ErrorKind::NotFound.into())
    }
    fn create_dir_all(&self, _path: &Path) -> io::Result<()> {
        Err(io::ErrorKind::PermissionDenied.into())
    }
    fn exists(&self, _path: &Path) -> bool {
        false
    }
    fn is_dir(&self, _path: &Path) -> bool {
        false
    }
    fn read_to_string(&self, _path: &Path) -> io::Result<String> {
        Err(io::ErrorKind::NotFound.into())
    }
    fn remove_dir_all(&self, _path: &Path) -> io::Result<()> {
        Err(io::ErrorKind::PermissionDenied.into())
    }
    fn remove_file(&self, _path: &Path) -> io::Result<()> {
        Err(io::ErrorKind::PermissionDenied.into())
    }
    fn write_string(&self, _path: &Path, _content: &str) -> io::Result<()> {
        Err(io::ErrorKind::PermissionDenied.into())
    }
}

struct ParentEnv;

impl Env for ParentEnv {
    fn current_dir(&self) -> io::Result<PathBuf> {
        Err(io::ErrorKind::NotFound.into())
    }
    fn current_exe(&self) -> io::Result<PathBuf> {
        Err(io::ErrorKind::NotFound.into())
    }
    fn var_os(&self, _key: &str) -> Option<OsString> {
        None
    }
}

/// Arranges object bytes while observing the read-only Git boundary.
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "canonical sibling contracts observe arranged identity and dependency effects"
)]
pub(in crate::git_factor::validation) struct CommitObject {
    pub(in crate::git_factor::validation) commit: CommitSha,
    pub(in crate::git_factor::validation) io: TestIo,
    pub(in crate::git_factor::validation) mutations: Cell<usize>,
    pub(in crate::git_factor::validation) queries: Cell<usize>,
    reply: Reply,
}

impl CommitObject {
    /// Arranges the context without invoking ancestry admission.
    pub(in crate::git_factor::validation) fn context(&self) -> Ctx<'_> {
        Ctx {
            cwd: PathBuf::from("/parent-admission"),
            env: &ENV,
            fs: &FS,
            io: &self.io,
            runner: self,
        }
    }

    /// Arranges the selected identity and an explicitly supplied response.
    pub(in crate::git_factor::validation) fn new(commit: &str, reply: Reply) -> Self {
        Self {
            commit: CommitSha::new(commit.to_owned()).or_abort("selected commit identity"),
            io: TestIo::default(),
            mutations: Cell::new(0),
            queries: Cell::new(0),
            reply,
        }
    }
}

impl Runner for CommitObject {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        if bin != "git" || cwd != Path::new("/parent-admission") {
            return Err(io::Error::other("unexpected object query boundary"));
        }
        if args != ["cat-file", "commit", self.commit.as_str()] {
            return Err(io::Error::other("diagnostic snapshot unavailable"));
        }
        self.queries.set(
            self.queries
                .get()
                .checked_add(1)
                .or_abort("bounded object query count"),
        );
        match self.reply.clone() {
            Reply::Output {
                code,
                content,
                error,
            } => Ok(Output {
                status: ExitStatus::from_raw(code),
                stdout: content.as_bytes().to_vec(),
                stderr: error.as_bytes().to_vec(),
            }),
            Reply::SpawnError(message) => Err(io::Error::other(message)),
        }
    }

    fn status(
        &self,
        _bin: &str,
        _args: &[&str],
        _envs: &[(&str, &str)],
        _quiet: bool,
        _cwd: &Path,
    ) -> io::Result<ExitStatus> {
        self.mutations.set(
            self.mutations
                .get()
                .checked_add(1)
                .or_abort("bounded mutation count"),
        );
        Err(io::Error::other("parent admission must not mutate Git"))
    }
}

/// One complete arranged input and independently specified expected outcome.
pub(in crate::git_factor::validation) type ParentCase = (String, Reply, Result<BaseParent, String>);

/// Generates one object observation; no ancestry admission runs while arranging it.
pub(in crate::git_factor::validation) fn inputs() -> impl Strategy<Value = ParentCase> {
    let identities = || ("[0-9a-f]{40}", "[0-9a-f]{40}", "[0-9a-f]{40}");
    prop_oneof![
        identities().prop_map(|(commit, parent, tree)| {
            let object = format!("tree {tree}\nauthor Example <example@example.com> 1 +0000\ngpgsig signature\n parent {parent}\n\nparent {parent}");
            (commit, Reply::text(object), Ok(BaseParent::Root))
        }),
        identities().prop_map(|(commit, parent, tree)| {
            let object = format!("tree {tree}\nparent {parent}\nauthor Example <example@example.com> 1 +0000\n\nsubject");
            (commit, Reply::text(object), Ok(BaseParent::Commit))
        }),
        identities().prop_map(|(commit, parent, tree)| {
            let expected = Err(format!("commit {commit} is a merge commit and cannot be split"));
            let object = format!("tree {tree}\nparent {parent}\nparent {parent}\n\nsubject");
            (commit, Reply::text(object), expected)
        }),
        identities().prop_map(|(commit, parent, tree)| {
            let expected = Err(format!("git command failed: malformed commit object {commit}: invalid parent header 'invalid-{parent}'"));
            let object = format!("tree {tree}\nparent invalid-{parent}\n\nsubject");
            (commit, Reply::text(object), expected)
        }),
        ("[0-9a-f]{40}", "[0-9a-f]{40}").prop_map(|(commit, tree)| {
            let expected = Err(format!("git command failed: malformed commit object {commit}: invalid tree header 'invalid-{tree}'"));
            (commit, Reply::text(format!("tree invalid-{tree}\n\nsubject")), expected)
        }),
        "[0-9a-f]{40}".prop_map(|commit| {
            let expected = Err(format!("git command failed: malformed commit object {commit}: missing tree header"));
            (commit, Reply::text(String::new()), expected)
        }),
        ("[0-9a-f]{40}", "[0-9a-f]{40}").prop_map(|(commit, parent)| {
            let expected = Err(format!("git command failed: malformed commit object {commit}: missing tree header"));
            (commit, Reply::text(format!("parent {parent}\n\nsubject")), expected)
        }),
        ("[0-9a-f]{40}", "[a-z]{1,20}").prop_map(|(commit, message)| {
            let expected = Err(format!("git command failed: {message}"));
            let code: i32 = 256;
            (commit, Reply::Output { code, content: String::new(), error: message }, expected)
        }),
        ("[0-9a-f]{40}", "[a-z]{1,20}").prop_map(|(commit, message)| {
            let expected = Err(format!("git command failed: git cat-file: {message}"));
            (commit, Reply::SpawnError(message), expected)
        }),
    ]
}
