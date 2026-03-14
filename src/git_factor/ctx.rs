use std::env;
use std::ffi::OsString;
pub(in crate::git_factor) use std::fs;
use std::io;
use std::path::{Path, PathBuf};
pub(in crate::git_factor) use std::process::{Command, ExitStatus, Output, Stdio};

use super::error::FactorError;

/// Shared `Io` for the real CLI.
pub(in crate::git_factor) static REAL_IO: RealIo = RealIo;
/// Shared `Env` for the real CLI.
pub(in crate::git_factor) static REAL_ENV: RealEnv = RealEnv;
/// Shared `Fs` for the real CLI.
pub(in crate::git_factor) static REAL_FS: RealFs = RealFs;
/// Shared `Runner` for the real CLI.
pub(in crate::git_factor) static REAL_RUNNER: RealRunner = RealRunner;

/// Handles all user-facing IO (stdout/stderr) for the CLI.
///
/// Tests can inject a capturing implementation to assert on exact messages.
pub(in crate::git_factor) trait Io {
    /// Writes raw text to stderr.
    fn err(&self, text: &str) -> io::Result<()>;

    /// Writes a line to stderr.
    fn errln(&self, line: &str) -> io::Result<()>;

    /// Writes raw text to stdout.
    fn out(&self, text: &str) -> io::Result<()>;

    /// Writes a line to stdout.
    fn outln(&self, line: &str) -> io::Result<()>;
}

/// Production [`Io`] implementation.
pub(in crate::git_factor) struct RealIo;

impl Io for RealIo {
    fn err(&self, text: &str) -> io::Result<()> {
        use io::Write as _;
        let mut err = io::stderr().lock();
        err.write_all(text.as_bytes())
    }

    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(&format!("{line}\n"))
    }

    fn out(&self, text: &str) -> io::Result<()> {
        use io::Write as _;
        let mut out = io::stdout().lock();
        out.write_all(text.as_bytes())
    }

    fn outln(&self, line: &str) -> io::Result<()> {
        use io::Write as _;
        let mut out = io::stdout().lock();
        out.write_all(format!("{line}\n").as_bytes())
    }
}

/// Environment access (current directory, current executable, etc.).
pub(in crate::git_factor) trait Env {
    /// Returns the current working directory.
    fn current_dir(&self) -> io::Result<PathBuf>;

    /// Returns the path of the currently running executable.
    fn current_exe(&self) -> io::Result<PathBuf>;

    /// Returns the value of an environment variable, if present.
    fn var_os(&self, key: &str) -> Option<OsString>;
}

/// Production [`Env`] implementation.
pub(in crate::git_factor) struct RealEnv;

impl Env for RealEnv {
    fn current_dir(&self) -> io::Result<PathBuf> {
        env::current_dir()
    }

    fn current_exe(&self) -> io::Result<PathBuf> {
        env::current_exe()
    }

    fn var_os(&self, key: &str) -> Option<OsString> {
        env::var_os(key)
    }
}

/// Filesystem access.
pub(in crate::git_factor) trait Fs {
    /// Canonicalizes a path.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;

    /// Creates a directory and all missing parent components.
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;

    /// Returns true when the path exists.
    fn exists(&self, path: &Path) -> bool;

    /// Returns true when the path exists and is a directory.
    fn is_dir(&self, path: &Path) -> bool;

    /// Reads a UTF-8 text file into a string.
    fn read_to_string(&self, path: &Path) -> io::Result<String>;

    /// Removes a directory tree.
    fn remove_dir_all(&self, path: &Path) -> io::Result<()>;

    /// Removes a file.
    fn remove_file(&self, path: &Path) -> io::Result<()>;

    /// Writes a UTF-8 text file from a string.
    fn write_string(&self, path: &Path, content: &str) -> io::Result<()>;
}

/// Production [`Fs`] implementation.
pub(in crate::git_factor) struct RealFs;

impl Fs for RealFs {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        fs::canonicalize(path)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        fs::write(path, content)
    }
}

/// Runs external processes (git, bash, etc.).
///
/// Production uses [`RealRunner`]. Tests can inject a fake runner to force
/// specific internal behaviors without relying on environment variables.
pub(in crate::git_factor) trait Runner {
    /// Runs a process and returns its captured output.
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output>;

    /// Runs a process and returns its exit status.
    ///
    /// `quiet=true` discards stdout/stderr to avoid noisy subprocess output.
    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus>;
}

/// Production [`Runner`] implementation.
pub(in crate::git_factor) struct RealRunner;

impl Runner for RealRunner {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        Command::new(bin).args(args).current_dir(cwd).output()
    }

    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        let mut command = Command::new(bin);
        command.args(args).current_dir(cwd);
        for &(key, value) in envs {
            command.env(key, value);
        }
        if quiet {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }
        command.status()
    }
}

/// Execution context for git-factor operations.
#[derive(Clone)]
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "fields must be visible to sibling modules within git_factor"
)]
pub(in crate::git_factor) struct Ctx<'runner> {
    /// Working directory for command execution.
    pub(in crate::git_factor) cwd: PathBuf,
    /// Environment access.
    pub(in crate::git_factor) env: &'runner dyn Env,
    /// Filesystem access.
    pub(in crate::git_factor) fs: &'runner dyn Fs,
    /// User-facing IO (stdout/stderr).
    pub(in crate::git_factor) io: &'runner dyn Io,
    /// Command runner implementation (real or test double).
    pub(in crate::git_factor) runner: &'runner dyn Runner,
}

impl Ctx<'_> {
    /// Writes raw text to stderr.
    pub(in crate::git_factor) fn err(&self, text: &str) -> Result<(), FactorError> {
        self.io.err(text).map_err(FactorError::Io)
    }

    /// Writes a line to stderr.
    pub(in crate::git_factor) fn errln(&self, line: &str) -> Result<(), FactorError> {
        self.io.errln(line).map_err(FactorError::Io)
    }

    /// Writes raw text to stdout.
    pub(in crate::git_factor) fn out(&self, text: &str) -> Result<(), FactorError> {
        self.io.out(text).map_err(FactorError::Io)
    }

    /// Writes a line to stdout.
    pub(in crate::git_factor) fn outln(&self, line: &str) -> Result<(), FactorError> {
        self.io.outln(line).map_err(FactorError::Io)
    }
}
