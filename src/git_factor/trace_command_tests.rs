use crate::git_factor::{Ctx, REAL_ENV, REAL_FS, REAL_IO, Runner};
use std::io;
use std::os::unix::process::ExitStatusExt as _;
use std::path::Path;
use std::process::{ExitStatus, Output};

#[derive(Clone)]
pub(in crate::git_factor::trace) enum CommandObservation {
    Failure(io::ErrorKind, String),
    Output(Output),
}

impl Runner for CommandObservation {
    fn output(&self, bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
        if bin != "git"
            || args
                != [
                    "--no-optional-locks",
                    "status",
                    "--porcelain=v1",
                    "--untracked-files=all",
                ]
        {
            return Err(io::Error::other("unexpected command observation"));
        }
        match self.clone() {
            Self::Failure(kind, message) => Err(io::Error::new(kind, message)),
            Self::Output(output) => Ok(output),
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
        Err(io::Error::other("trace observations must not mutate Git"))
    }
}

/// Arranges the bytes and exit status returned by one command observation.
pub(in crate::git_factor::trace) fn arrange_output(
    stdout: &[u8],
    stderr: &[u8],
    exit_code: i32,
) -> CommandObservation {
    CommandObservation::Output(Output {
        status: ExitStatus::from_raw(exit_code << 8),
        stdout: stdout.to_vec(),
        stderr: stderr.to_vec(),
    })
}

/// Arranges a context without querying or mutating Git.
pub(in crate::git_factor::trace) fn arrange_context(runner: &dyn Runner) -> Ctx<'_> {
    Ctx {
        cwd: "/trace-command-contract".into(),
        env: &REAL_ENV,
        fs: &REAL_FS,
        io: &REAL_IO,
        runner,
    }
}
