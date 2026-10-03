mod aborted {
    use super::*;

    #[test]
    fn reports_each_observed_rebase_state_and_write_failure() {
        verify_aborted(false);
        verify_aborted(true);
    }
}

mod session_status {
    mod new {
        use super::super::*;

        #[test]
        fn preserves_checked_status_facts_at_numeric_boundaries() {
            for index in [0, usize::MAX] {
                for count in [0, u8::MAX] {
                    for phase in [SessionPhase::PendingStart, SessionPhase::Splitting] {
                        verify_status(Some((index, count, phase, true, false, true)));
                    }
                }
            }
        }
    }
}

mod status {
    use super::*;

    #[test]
    fn inactive_status_is_null_and_never_queries_git() {
        verify_status(None);
    }

    #[test]
    fn active_status_preserves_all_boolean_observations() {
        for required in [false, true] {
            for root in [false, true] {
                for progress in [false, true] {
                    verify_status(Some((
                        1,
                        2,
                        SessionPhase::Splitting,
                        required,
                        root,
                        progress,
                    )));
                }
            }
        }
    }
}

use core::cell::{Cell, RefCell};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};

use crate::git_factor::ctx::{Io, REAL_ENV, REAL_FS, Runner};
use crate::test_support::OrAbort as _;

use super::*;

#[derive(Default)]
struct Capture {
    bytes: RefCell<String>,
    calls: Cell<usize>,
    fail_at: Option<usize>,
}

impl Io for Capture {
    fn err(&self, _: &str) -> io::Result<()> {
        Err(io::Error::other("unexpected stderr"))
    }
    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(line)
    }
    fn out(&self, text: &str) -> io::Result<()> {
        let call = self.calls.get().checked_add(1).or_abort("write count");
        self.calls.set(call);
        if self.fail_at == Some(call) {
            return Err(io::Error::other("status write failure"));
        }
        self.bytes.borrow_mut().push_str(text);
        Ok(())
    }
    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }
}

#[derive(Default)]
struct NoQueries(Cell<usize>);

impl Runner for NoQueries {
    fn output(&self, _: &str, _: &[&str], _: &Path) -> io::Result<Output> {
        self.0
            .set(self.0.get().checked_add(1).or_abort("query count"));
        Err(io::Error::other("status emitter must not query Git"))
    }
    fn status(
        &self,
        _: &str,
        _: &[&str],
        _: &[(&str, &str)],
        _: bool,
        _: &Path,
    ) -> io::Result<ExitStatus> {
        self.0
            .set(self.0.get().checked_add(1).or_abort("query count"));
        Err(io::Error::other("status emitter must not mutate Git"))
    }
}

/// Direct serializer oracle through its real public constructor and emitter.
#[expect(
    clippy::unreachable,
    reason = "the local failure loop admits only literal None, Some(1), and Some(2); no caller supplies a write boundary"
)]
pub(in crate::git_factor::output) fn verify_status(
    facts: Option<(usize, u8, SessionPhase, bool, bool, bool)>,
) {
    let commit = CommitSha::new("a".repeat(40)).or_abort("admitted commit");
    let session = facts.map(|(index, count, phase, required, root, progress)| {
        SessionStatus::new(
            &commit,
            CurrentIndex(index),
            SplitCount(count),
            phase,
            StateBool::from_bool(required),
            StateBool::from_bool(root),
            progress,
        )
    });
    let expected = match facts {
        None => "{\"operation\":\"status\",\"session\":null}".to_owned(),
        Some((index, count, phase, required, root, progress)) => format!(
            "{{\"operation\":\"status\",\"session\":{{\"phase\":\"{}\",\"rebase\":{{\"in_progress\":{progress},\"required\":{required}}},\"split_count\":{count},\"target\":{{\"commit\":\"{}\",\"index\":{index},\"span_starts_at_root\":{root}}}}}}}",
            match phase {
                SessionPhase::PendingStart => "pending_start",
                SessionPhase::Splitting => "splitting",
            },
            "a".repeat(40),
        ),
    };
    for fail_at in [None, Some(1), Some(2)] {
        let capture = Capture {
            fail_at,
            ..Capture::default()
        };
        let runner = NoQueries::default();
        let ctx = Ctx {
            cwd: PathBuf::from("."),
            env: &REAL_ENV,
            fs: &REAL_FS,
            io: &capture,
            runner: &runner,
        };
        let result = status(&ctx, session);
        match fail_at {
            None => {
                result.or_abort("status result");
                assert_eq!(*capture.bytes.borrow(), format!("{expected}\n"));
                assert_eq!(capture.calls.get(), 2);
            }
            Some(1) => {
                assert!(
                    matches!(result, Err(FactorError::Io(error)) if error.to_string() == "status write failure")
                );
                assert_eq!(*capture.bytes.borrow(), "");
                assert_eq!(capture.calls.get(), 1);
            }
            Some(2) => {
                assert!(
                    matches!(result, Err(FactorError::Io(error)) if error.to_string() == "status write failure")
                );
                assert_eq!(*capture.bytes.borrow(), expected);
                assert_eq!(capture.calls.get(), 2);
            }
            Some(_) => unreachable!("closed write boundaries"),
        }
        assert_eq!(runner.0.get(), 0);
    }
}

/// Direct abort output oracle for both native observations and each write boundary.
#[expect(
    clippy::unreachable,
    reason = "the local failure loop admits only literal None, Some(1), and Some(2); no caller supplies a write boundary"
)]
pub(in crate::git_factor::output) fn verify_aborted(in_progress: bool) {
    let expected = if in_progress {
        "{\"operation\":\"abort\",\"rebase\":{\"in_progress\":true},\"actions\":{\"abort_rebase\":[\"git\",\"rebase\",\"--abort\"]}}"
    } else {
        "{\"operation\":\"abort\",\"rebase\":{\"in_progress\":false},\"actions\":{}}"
    };
    for fail_at in [None, Some(1), Some(2)] {
        let capture = Capture {
            fail_at,
            ..Capture::default()
        };
        let runner = NoQueries::default();
        let ctx = Ctx {
            cwd: PathBuf::from("."),
            env: &REAL_ENV,
            fs: &REAL_FS,
            io: &capture,
            runner: &runner,
        };
        let result = aborted(&ctx, in_progress);
        match fail_at {
            None => {
                result.or_abort("abort result");
                assert_eq!(*capture.bytes.borrow(), format!("{expected}\n"));
                assert_eq!(capture.calls.get(), 2);
            }
            Some(1) => {
                assert!(
                    matches!(result, Err(FactorError::Io(error)) if error.to_string() == "status write failure")
                );
                assert_eq!(*capture.bytes.borrow(), "");
                assert_eq!(capture.calls.get(), 1);
            }
            Some(2) => {
                assert!(
                    matches!(result, Err(FactorError::Io(error)) if error.to_string() == "status write failure")
                );
                assert_eq!(*capture.bytes.borrow(), expected);
                assert_eq!(capture.calls.get(), 2);
            }
            Some(_) => unreachable!("closed write boundaries"),
        }
        assert_eq!(runner.0.get(), 0);
    }
}
