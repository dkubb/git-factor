extern crate alloc;

use crate::git_factor::Runner;
use alloc::collections::VecDeque;
use core::cell::RefCell;
use std::io;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};

/// Exact output requests observed by the injected runner.
pub(in crate::git_factor::git) type Request = (String, Vec<String>, PathBuf);

/// A reply queue and request ledger shared by the direct unit/property providers.
pub(in crate::git_factor::git) struct RecordingRunner {
    replies: RefCell<VecDeque<io::Result<Output>>>,
    requests: RefCell<Vec<Request>>,
}

impl RecordingRunner {
    /// Arranges the replies without executing a Git-output producer.
    pub(in crate::git_factor::git) fn new(replies: Vec<io::Result<Output>>) -> Self {
        Self {
            replies: RefCell::new(VecDeque::from(replies)),
            requests: RefCell::new(Vec::new()),
        }
    }

    /// Returns the exact observed output requests for assertions in each body.
    pub(in crate::git_factor::git) fn requests(&self) -> Vec<Request> {
        self.requests.borrow().clone()
    }
}

impl Runner for RecordingRunner {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        self.requests.borrow_mut().push((
            bin.to_owned(),
            args.iter().map(|arg| (*arg).to_owned()).collect(),
            cwd.to_path_buf(),
        ));
        self.replies
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Err(io::Error::other("unarranged output request")))
    }

    fn status(
        &self,
        _bin: &str,
        _args: &[&str],
        _envs: &[(&str, &str)],
        _quiet: bool,
        _cwd: &Path,
    ) -> io::Result<ExitStatus> {
        Err(io::Error::other("output contracts cannot request status"))
    }
}

/// Arranges a successful native output response, preserving the supplied bytes.
pub(in crate::git_factor::git) fn successful(stdout: &[u8], stderr: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.to_vec(),
        stderr: stderr.to_vec(),
    }
}

/// Arranges five successful snapshot responses in the actual query order.
pub(in crate::git_factor::git) fn snapshot(
    head: &str,
    tree: &str,
    cwd: &Path,
) -> Vec<io::Result<Output>> {
    vec![
        Ok(successful(format!("{head}\n").as_bytes(), b"")),
        Ok(successful(format!("{tree}\n").as_bytes(), b"")),
        Ok(successful(b".git\n", b"")),
        Ok(successful(format!("{}\n", cwd.display()).as_bytes(), b"")),
        Ok(successful(b"", b"")),
    ]
}
