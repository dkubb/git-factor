use super::*;
use core::cell::{Cell, RefCell};
use std::os::unix::process::ExitStatusExt as _;

#[derive(Clone)]
pub(in crate::git_factor::validation) enum Reply {
    IoFailure,
    Output { exit_code: u8, stdout: Vec<u8> },
}

pub(in crate::git_factor::validation) struct Observation<'calls> {
    calls: &'calls RefCell<Vec<String>>,
    consumed: Cell<bool>,
    reply: Reply,
}

impl<'calls> Observation<'calls> {
    pub(in crate::git_factor::validation) fn new(
        calls: &'calls RefCell<Vec<String>>,
        reply: Reply,
    ) -> Self {
        Self {
            calls,
            consumed: Cell::new(false),
            reply,
        }
    }

    fn refused<T>(&self, call: String) -> io::Result<T> {
        self.calls.borrow_mut().push(call);
        Err(io::Error::other("resolution must not use this IO class"))
    }
}

impl Runner for Observation<'_> {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        self.calls
            .borrow_mut()
            .push(format!("output {bin} {args:?} cwd={cwd:?}"));
        if self.consumed.replace(true) {
            return Err(io::Error::other("no snapshot reply"));
        }
        match self.reply.clone() {
            Reply::IoFailure => Err(io::Error::other("resolution launch failed")),
            Reply::Output { exit_code, stdout } => Ok(Output {
                status: ExitStatus::from_raw(i32::from(exit_code) << 8),
                stdout,
                stderr: b"query rejected\n".to_vec(),
            }),
        }
    }

    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        self.calls.borrow_mut().push(format!(
            "status {bin} {args:?} {envs:?} quiet={quiet} cwd={cwd:?}"
        ));
        Err(io::Error::other("resolution must capture output"))
    }
}

impl Env for Observation<'_> {
    fn current_dir(&self) -> io::Result<PathBuf> {
        self.calls.borrow_mut().push("current_dir".to_owned());
        Err(io::Error::other("resolution uses its context directory"))
    }

    fn current_exe(&self) -> io::Result<PathBuf> {
        self.calls.borrow_mut().push("current_exe".to_owned());
        Err(io::Error::other(
            "resolution must not inspect the executable",
        ))
    }

    fn var_os(&self, _key: &str) -> Option<OsString> {
        None
    }
}

impl Fs for Observation<'_> {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.refused(format!("canonicalize {path:?}"))
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.refused(format!("create_dir_all {path:?}"))
    }
    fn exists(&self, path: &Path) -> bool {
        self.calls.borrow_mut().push(format!("exists {path:?}"));
        false
    }
    fn is_dir(&self, path: &Path) -> bool {
        self.calls.borrow_mut().push(format!("is_dir {path:?}"));
        false
    }
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.refused(format!("read_to_string {path:?}"))
    }
    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        self.refused(format!("remove_dir_all {path:?}"))
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.refused(format!("remove_file {path:?}"))
    }
    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        self.refused(format!("write_string {path:?} {content:?}"))
    }
}

impl Io for Observation<'_> {
    fn err(&self, text: &str) -> io::Result<()> {
        self.refused(format!("stderr {text:?}"))
    }
    fn errln(&self, line: &str) -> io::Result<()> {
        self.refused(format!("stderr line {line:?}"))
    }
    fn out(&self, text: &str) -> io::Result<()> {
        self.refused(format!("stdout {text:?}"))
    }
    fn outln(&self, line: &str) -> io::Result<()> {
        self.refused(format!("stdout line {line:?}"))
    }
}
