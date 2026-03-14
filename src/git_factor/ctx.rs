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

#[cfg(test)]
mod tests {
    use super::{
        Ctx, Env as _, Fs as _, Io as _, REAL_ENV, REAL_FS, REAL_IO, REAL_RUNNER, Runner as _,
    };
    use crate::test_support::OrAbort as _;
    use core::cell::RefCell;
    use std::env;
    use std::io;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    #[derive(Default)]
    struct RecordingIo {
        err: RefCell<String>,
        out: RefCell<String>,
    }

    impl super::Io for RecordingIo {
        fn err(&self, text: &str) -> io::Result<()> {
            self.err.borrow_mut().push_str(text);
            Ok(())
        }

        fn errln(&self, line: &str) -> io::Result<()> {
            self.err.borrow_mut().push_str(line);
            self.err.borrow_mut().push('\n');
            Ok(())
        }

        fn out(&self, text: &str) -> io::Result<()> {
            self.out.borrow_mut().push_str(text);
            Ok(())
        }

        fn outln(&self, line: &str) -> io::Result<()> {
            self.out.borrow_mut().push_str(line);
            self.out.borrow_mut().push('\n');
            Ok(())
        }
    }

    #[test]
    fn real_io_accepts_empty_writes() {
        REAL_IO.out("").or_abort("stdout write should succeed");
        REAL_IO.err("").or_abort("stderr write should succeed");
        REAL_IO
            .outln("")
            .or_abort("stdout line write should succeed");
        REAL_IO
            .errln("")
            .or_abort("stderr line write should succeed");
    }

    #[test]
    fn real_env_reports_process_state() {
        let cwd = REAL_ENV.current_dir().or_abort("cwd should resolve");
        assert_eq!(cwd, env::current_dir().or_abort("std cwd should resolve"));

        let current_exe = REAL_ENV
            .current_exe()
            .or_abort("current exe should resolve");
        assert!(current_exe.is_absolute());
        assert!(current_exe.exists());

        assert_eq!(REAL_ENV.var_os("PATH"), env::var_os("PATH"));
    }

    #[test]
    fn real_fs_supports_directory_and_file_lifecycle() {
        let dir = TempDir::new().or_abort("tempdir");
        let nested = dir.path().join("nested");
        let file = nested.join("note.txt");

        REAL_FS
            .create_dir_all(&nested)
            .or_abort("create dir should succeed");
        assert!(REAL_FS.exists(&nested));
        assert!(REAL_FS.is_dir(&nested));

        REAL_FS
            .write_string(&file, "hello\n")
            .or_abort("write should succeed");
        assert_eq!(
            REAL_FS
                .read_to_string(&file)
                .or_abort("read should succeed"),
            "hello\n"
        );

        let canonical = REAL_FS
            .canonicalize(&file)
            .or_abort("canonicalize should succeed");
        assert!(canonical.ends_with(Path::new("note.txt")));

        REAL_FS
            .remove_file(&file)
            .or_abort("remove file should succeed");
        assert!(!REAL_FS.exists(&file));

        REAL_FS
            .remove_dir_all(&nested)
            .or_abort("remove dir should succeed");
        assert!(!REAL_FS.exists(&nested));
    }

    #[cfg(unix)]
    #[test]
    fn real_runner_executes_commands_with_expected_behavior() {
        let dir = TempDir::new().or_abort("tempdir");

        let output = REAL_RUNNER
            .output("sh", &["-c", "printf runner"], dir.path())
            .or_abort("runner output should succeed");
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).or_abort("stdout utf8"),
            "runner"
        );
        assert_eq!(String::from_utf8(output.stderr).or_abort("stderr utf8"), "");

        let status = REAL_RUNNER
            .status(
                "sh",
                &["-c", "test \"$CTX_RUNNER_TEST\" = expected"],
                &[("CTX_RUNNER_TEST", "expected")],
                true,
                dir.path(),
            )
            .or_abort("runner status should succeed");
        assert!(status.success());
    }

    #[test]
    fn ctx_carries_runtime_dependencies() {
        let dir = TempDir::new().or_abort("tempdir");
        let ctx = Ctx {
            cwd: dir.path().to_path_buf(),
            env: &REAL_ENV,
            fs: &REAL_FS,
            io: &REAL_IO,
            runner: &REAL_RUNNER,
        };

        assert_eq!(ctx.cwd, PathBuf::from(dir.path()));
        assert_eq!(
            ctx.env.current_dir().or_abort("cwd should resolve"),
            env::current_dir().or_abort("std cwd should resolve")
        );
        assert!(ctx.fs.exists(dir.path()));
        let _: &dyn super::Io = ctx.io;
        let _: &dyn super::Runner = ctx.runner;
    }

    #[test]
    fn ctx_io_helpers_delegate_to_the_inner_io() {
        let dir = TempDir::new().or_abort("tempdir");
        let io = RecordingIo::default();
        let ctx = Ctx {
            cwd: dir.path().to_path_buf(),
            env: &REAL_ENV,
            fs: &REAL_FS,
            io: &io,
            runner: &REAL_RUNNER,
        };

        ctx.out("out").or_abort("stdout write should succeed");
        ctx.outln("line")
            .or_abort("stdout line write should succeed");
        ctx.err("err").or_abort("stderr write should succeed");
        ctx.errln("warn")
            .or_abort("stderr line write should succeed");

        assert_eq!(io.out.borrow().as_str(), "outline\n");
        assert_eq!(io.err.borrow().as_str(), "errwarn\n");
    }
}
