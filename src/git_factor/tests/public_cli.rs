//! Arrange public CLI observations without changing the production API.
use super::*;
use core::cell::Cell;
use std::process::Command;

#[derive(Default)]
struct PublicRunner {
    calls: RefCell<Vec<String>>,
}
impl Runner for PublicRunner {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        self.calls
            .borrow_mut()
            .push(format!("output {bin} {args:?} {envs:?} {}", cwd.display()));
        REAL_RUNNER.output(bin, args, envs, cwd)
    }
    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        self.calls.borrow_mut().push(format!(
            "status {bin} {args:?} {envs:?} quiet={quiet} {}",
            cwd.display()
        ));
        REAL_RUNNER.status(bin, args, envs, quiet, cwd)
    }
}

/// Only arrangement and readback; the public command Act remains in each provider.
pub(in crate::git_factor) struct PublicCli {
    directory: TempDir,
    environment: TestEnv,
    output: TestIo,
    runner: PublicRunner,
}
impl PublicCli {
    pub(in crate::git_factor) fn calls(&self) -> Vec<String> {
        self.runner.calls.borrow().clone()
    }
    pub(in crate::git_factor) fn context(&self) -> Ctx<'_> {
        Ctx {
            cwd: self.directory.path().to_path_buf(),
            env: &self.environment,
            runner: &self.runner,
            io: &self.output,
            fs: &REAL_FS,
        }
    }
    pub(in crate::git_factor) fn expected_diagnostic_calls(&self) -> Vec<String> {
        vec![format!(
            "output git [\"rev-parse\", \"--git-dir\"] [] {}",
            self.directory.path().display()
        )]
    }
    pub(in crate::git_factor) fn expected_inactive_calls(&self) -> Vec<String> {
        let cwd = self.directory.path().display();
        vec![
            format!("output git [\"version\"] [] {cwd}"),
            format!("output git [\"rev-parse\", \"--git-dir\"] [] {cwd}"),
            format!("output git [\"rev-parse\", \"--git-dir\"] [] {cwd}"),
        ]
    }
    pub(in crate::git_factor) fn facts(&self) -> Vec<Option<Vec<u8>>> {
        [
            "unrelated",
            ".git/factor/commits",
            ".git/factor-journal.json",
            ".git/index",
            ".git/rebase-merge/done",
            ".git/rebase-merge/git-rebase-todo",
            ".git/factor/error.log",
        ]
        .iter()
        .map(|path| fs::read(self.directory.path().join(path)).ok())
        .collect()
    }
    pub(in crate::git_factor) fn inactive() -> Self {
        let directory = TempDir::new().or_abort("inactive CLI fixture");
        let cwd = directory.path().to_path_buf();
        let output = Command::new("git")
            .args(["init", "--quiet", "--initial-branch=main"])
            .current_dir(&cwd)
            .output()
            .or_abort("native inactive repository");
        assert!(output.status.success());
        fs::write(cwd.join("unrelated"), b"user bytes\0\n").or_abort("user fixture bytes");
        Self {
            directory,
            environment: TestEnv { cwd },
            runner: PublicRunner::default(),
            output: TestIo::default(),
        }
    }
    pub(in crate::git_factor) fn pure() -> Self {
        let directory = TempDir::new().or_abort("pure CLI fixture");
        let cwd = directory.path().to_path_buf();
        fs::create_dir_all(cwd.join(".git/factor")).or_abort("saved legacy fixture");
        fs::write(cwd.join(".git/factor/commits"), b"saved session identity\n")
            .or_abort("saved fixture bytes");
        fs::write(cwd.join("unrelated"), b"user bytes\0\n").or_abort("user fixture bytes");
        Self {
            directory,
            environment: TestEnv { cwd },
            runner: PublicRunner::default(),
            output: TestIo::default(),
        }
    }
    pub(in crate::git_factor) fn stderr(&self) -> String {
        self.output.stderr()
    }
    pub(in crate::git_factor) fn stdout(&self) -> String {
        self.output.stdout()
    }
}

/// Refuse only the first real canonical journal read, delegating all other filesystem work.
pub(in crate::git_factor) struct JournalReadRefusal<'fixture> {
    calls: Cell<usize>,
    inner: &'fixture dyn Fs,
    path: PathBuf,
}
impl<'fixture> JournalReadRefusal<'fixture> {
    pub(in crate::git_factor) fn calls(&self) -> usize {
        self.calls.get()
    }
    pub(in crate::git_factor) fn new(inner: &'fixture dyn Fs, cwd: &Path) -> Self {
        Self {
            calls: Cell::new(0),
            inner,
            path: cwd.join(".git/factor-journal.json"),
        }
    }
}
impl Fs for JournalReadRefusal<'_> {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.inner.canonicalize(path)
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.inner.create_dir_all(path)
    }
    fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path)
    }
    fn is_dir(&self, path: &Path) -> bool {
        self.inner.is_dir(path)
    }
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        if path == self.path {
            let calls = self
                .calls
                .get()
                .checked_add(1)
                .ok_or_else(|| io::Error::other("journal observation count overflow"))?;
            self.calls.set(calls);
            if calls == 1 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "journal read denied",
                ));
            }
        }
        self.inner.read_to_string(path)
    }
    fn remove_atomic_file(&self, path: &Path) -> io::Result<()> {
        self.inner.remove_atomic_file(path)
    }
    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        self.inner.remove_dir_all(path)
    }
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        self.inner.symlink_metadata(path)
    }
    fn write_atomic_string(&self, path: &Path, content: &str) -> io::Result<()> {
        self.inner.write_atomic_string(path, content)
    }
    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        self.inner.write_string(path, content)
    }
}

/// Fail one actual output write while retaining all later diagnostic writes.
pub(in crate::git_factor) struct OutputRefusal<'fixture> {
    fired: Cell<bool>,
    inner: &'fixture dyn Io,
    newline: bool,
}
impl<'fixture> OutputRefusal<'fixture> {
    pub(in crate::git_factor) fn fired(&self) -> bool {
        self.fired.get()
    }
    pub(in crate::git_factor) fn new(inner: &'fixture dyn Io, newline: bool) -> Self {
        Self {
            inner,
            newline,
            fired: Cell::new(false),
        }
    }
    fn observe(&self, text: &str) -> io::Result<()> {
        if !self.fired.get() && (!self.newline || text == "\n") {
            self.fired.set(true);
            Err(io::Error::other("output denied"))
        } else {
            Ok(())
        }
    }
}
impl Io for OutputRefusal<'_> {
    fn err(&self, text: &str) -> io::Result<()> {
        self.observe(text)?;
        self.inner.err(text)
    }
    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(line)?;
        self.err("\n")
    }
    fn out(&self, text: &str) -> io::Result<()> {
        self.observe(text)?;
        self.inner.out(text)
    }
    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }
}
