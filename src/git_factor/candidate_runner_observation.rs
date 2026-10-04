use super::Runner;
use crate::git_factor::REAL_RUNNER;
use core::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};

/// Exactly what the actor runner delegated to its physical runner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::git_factor::candidate) struct Request {
    arguments: Vec<String>,
    command: String,
    directory: PathBuf,
    environment: Vec<(String, Option<String>)>,
    quiet: Option<bool>,
}

#[derive(Default)]
pub(in crate::git_factor::candidate) struct Observation {
    requests: RefCell<Vec<Request>>,
}
impl Observation {
    fn record(
        &self,
        command: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        quiet: Option<bool>,
        cwd: &Path,
    ) {
        self.requests
            .borrow_mut()
            .push(Request::new(command, args, envs, quiet, cwd));
    }
    pub(in crate::git_factor::candidate) fn requests(&self) -> Vec<Request> {
        self.requests.borrow().clone()
    }
}
impl Runner for Observation {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        self.record(bin, args, envs, None, cwd);
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
        self.record(bin, args, envs, Some(quiet), cwd);
        REAL_RUNNER.status(bin, args, envs, quiet, cwd)
    }
}

impl Request {
    pub(in crate::git_factor::candidate) fn new(
        command: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        quiet: Option<bool>,
        cwd: &Path,
    ) -> Self {
        Self {
            arguments: args.iter().map(|arg| (*arg).to_owned()).collect(),
            command: command.to_owned(),
            directory: cwd.to_path_buf(),
            environment: envs
                .iter()
                .map(|&(key, value)| (key.to_owned(), value.map(str::to_owned)))
                .collect(),
            quiet,
        }
    }
}
