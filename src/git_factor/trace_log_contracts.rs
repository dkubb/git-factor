use super::*;
use crate::git_factor::{Env, REAL_FS, REAL_IO, Runner};
use crate::test_support::OrAbort as _;
use core::cell::Cell;
use std::env;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};
use tempfile::TempDir;

/// Physical files with deliberately unavailable native diagnostic queries.
pub(in crate::git_factor::trace) struct LogFixture {
    directory: TempDir,
    environment: TraceEnvironment,
    runner: UnavailableQueries,
}
impl LogFixture {
    pub(in crate::git_factor::trace) fn context(&self) -> Ctx<'_> {
        Ctx {
            cwd: self.directory.path().to_path_buf(),
            env: &self.environment,
            fs: &REAL_FS,
            io: &REAL_IO,
            runner: &self.runner,
        }
    }
    pub(in crate::git_factor::trace) fn directory(&self) -> &Path {
        self.directory.path()
    }
    pub(in crate::git_factor::trace) fn new(enabled: bool) -> Self {
        let directory = TempDir::new().or_abort("trace contract files");
        let environment = TraceEnvironment {
            path: enabled.then(|| directory.path().join("trace").into_os_string()),
        };
        Self {
            directory,
            environment,
            runner: UnavailableQueries::default(),
        }
    }
    pub(in crate::git_factor::trace) fn queries(&self) -> usize {
        self.runner.0.get()
    }
}
struct TraceEnvironment {
    path: Option<OsString>,
}
impl Env for TraceEnvironment {
    fn current_dir(&self) -> io::Result<PathBuf> {
        env::current_dir()
    }
    fn current_exe(&self) -> io::Result<PathBuf> {
        env::current_exe()
    }
    fn var_os(&self, key: &str) -> Option<OsString> {
        if key == TRACE_LOG_ENV {
            self.path.clone()
        } else {
            None
        }
    }
}
#[derive(Default)]
struct UnavailableQueries(Cell<usize>);
impl Runner for UnavailableQueries {
    fn output(
        &self,
        _bin: &str,
        _args: &[&str],
        _envs: &[(&str, Option<&str>)],
        _cwd: &Path,
    ) -> io::Result<Output> {
        self.0.set(
            self.0
                .get()
                .checked_add(1)
                .or_abort("bounded native query count"),
        );
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "native query unavailable",
        ))
    }
    fn status(
        &self,
        _bin: &str,
        _args: &[&str],
        _envs: &[(&str, Option<&str>)],
        _quiet: bool,
        _cwd: &Path,
    ) -> io::Result<ExitStatus> {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "native query unavailable",
        ))
    }
}
