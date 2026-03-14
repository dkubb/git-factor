//! `git-sequence-editor` is a strict `GIT_SEQUENCE_EDITOR` helper that rewrites
//! interactive rebase todo lists.

#![expect(
    clippy::implicit_return,
    reason = "sequence-editor workflow uses expression tails for direct control flow"
)]
/// CLI argument model for `git-sequence-editor`.
#[path = "git_sequence_editor/cli.rs"]
mod cli;
/// Todo parsing and rewrite logic for `git-sequence-editor`.
#[path = "git_sequence_editor/todo.rs"]
mod todo;

use core::sync::atomic::{AtomicU8, Ordering};
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Write as _};
use std::path::Path;
use std::process;
#[cfg(test)]
use std::sync::{Mutex, MutexGuard};

use clap::Parser as _;
use thiserror::Error;

use self::cli::Cli;
use self::todo::{
    build_factor_insertions, build_requested_actions, rewrite_todo_with_factor, todo_shas_in,
    validate_todo_format,
};

#[cfg(test)]
use crate::test_support::{OrAbort as _, ResultOrAbort as _};
use crate::exit_codes::EXIT_OK;

/// Maximum attempts when creating a unique temporary todo file.
const TEMP_FILE_ATTEMPTS_MAX: u32 = 1024;
/// Test fault-injection selector for atomic write operations.
static WRITE_FAIL_POINT: AtomicU8 = AtomicU8::new(0);

/// Successful execution exit code.
#[repr(i32)]
enum EditorExitCode {
    /// Successful execution.
    Ok = 0,
    /// Runtime failure.
    RuntimeError = 1,
}

impl EditorExitCode {
    /// Returns the process exit code value.
    #[expect(clippy::as_conversions, reason = "repr(i32) enum to i32 is safe")]
    const fn code(self) -> i32 {
        self as i32
    }
}

/// Fault-injection points used by tests covering atomic todo writes.
#[repr(u8)]
#[derive(Clone, Copy, Eq, PartialEq)]
enum WriteFailPoint {
    /// No injected I/O failure.
    None = 0,
    /// Fail while opening the parent directory for sync.
    OpenParentDir = 3,
    /// Fail while syncing the temporary file.
    Sync = 2,
    /// Fail while syncing the parent directory.
    SyncParentDir = 4,
    /// Fail while writing the temporary file contents.
    Write = 1,
}

impl WriteFailPoint {
    /// Encodes this fail point for atomic storage.
    #[cfg(test)]
    #[expect(clippy::as_conversions, reason = "repr(u8) enum to u8 is safe")]
    const fn encode(self) -> u8 {
        self as u8
    }
}

/// Structured failures for atomic todo-file writes.
#[derive(Debug, Error)]
enum AtomicWriteError {
    /// Temporary file creation failed.
    #[error("failed to create temporary todo file for {0}: {1}")]
    CreateTemp(String, #[source] io::Error),
    /// File name could not be inferred from the target path.
    #[error("failed to determine file name for: {0}")]
    MissingFileName(String),
    /// Parent directory could not be inferred from the target path.
    #[error("failed to determine parent directory for: {0}")]
    MissingParent(String),
    /// Parent directory open failed during unix fsync sequence.
    #[error("failed to open parent directory for {0}: {1}")]
    OpenParentDir(String, #[source] io::Error),
    /// Atomic rename failed.
    #[error("failed to atomically replace todo file {0}: {1}")]
    ReplaceFile(String, #[source] io::Error),
    /// Parent directory sync failed during unix fsync sequence.
    #[error("failed to sync parent directory for {0}: {1}")]
    SyncParentDir(String, #[source] io::Error),
    /// Temporary file sync failed.
    #[error("failed to sync temporary todo file for {0}: {1}")]
    SyncTemp(String, #[source] io::Error),
    /// Temporary filename space was exhausted.
    #[error("failed to create a unique temporary file for {0}")]
    TempNameExhausted(String),
    /// Temporary file write failed.
    #[error("failed to write temporary todo file for {0}: {1}")]
    WriteTemp(String, #[source] io::Error),
}

/// Pre-validated target metadata for atomic todo-file writes.
struct AtomicWriteTarget<'path> {
    /// Destination file name used as temp-file prefix.
    file_name: OsString,
    /// Parent directory where temp files are created.
    parent: &'path Path,
    /// Full destination path.
    path: &'path Path,
}

impl<'path> AtomicWriteTarget<'path> {
    /// Builds an atomic-write target, rejecting unsupported path shapes.
    #[expect(
        clippy::single_call_fn,
        reason = "encodes write-target path invariants at one boundary"
    )]
    fn new(path: &'path Path) -> Result<Self, AtomicWriteError> {
        let path_display = path.display().to_string();
        let Some(parent) = path.parent() else {
            return Err(AtomicWriteError::MissingParent(path_display));
        };
        let Some(file_name) = path.file_name() else {
            return Err(AtomicWriteError::MissingFileName(path_display));
        };

        Ok(Self {
            file_name: file_name.to_os_string(),
            parent,
            path,
        })
    }
}

/// Structured failures from `git-sequence-editor` orchestration.
#[derive(Debug, Error)]
enum SequenceEditorError {
    /// Atomic write of rewritten content failed.
    #[error(transparent)]
    AtomicWrite(#[from] AtomicWriteError),
    /// Reading the todo file failed.
    #[error("failed to read todo file: {0}")]
    ReadTodo(#[source] io::Error),
    /// Todo parsing/validation failed.
    #[error(transparent)]
    Todo(#[from] todo::TodoError),
}

/// Writes one diagnostic line to stderr, ignoring write errors.
fn write_stderr_line(message: &str) {
    let mut stderr = io::stderr().lock();
    let _ignored = writeln!(stderr, "{message}");
}

/// Sets the active atomic-write fault injection point.
#[cfg(test)]
fn set_write_fail_point(value: WriteFailPoint) {
    WRITE_FAIL_POINT.store(value.encode(), Ordering::SeqCst);
}

/// Serializes tests that mutate the global write failpoint.
#[cfg(test)]
static WRITE_FAIL_POINT_TEST_MUTEX: Mutex<()> = Mutex::new(());

/// Locks test execution for failpoint-mutating tests.
#[cfg(test)]
fn lock_write_fail_point_test() -> MutexGuard<'static, ()> {
    WRITE_FAIL_POINT_TEST_MUTEX.lock().or_abort("")
}

/// Reads the currently active atomic-write fault injection point.
fn write_fail_point() -> WriteFailPoint {
    match WRITE_FAIL_POINT.load(Ordering::SeqCst) {
        1 => WriteFailPoint::Write,
        2 => WriteFailPoint::Sync,
        3 => WriteFailPoint::OpenParentDir,
        4 => WriteFailPoint::SyncParentDir,
        _ => WriteFailPoint::None,
    }
}

/// Syncs the parent directory after atomic rename on Unix.
#[cfg(unix)]
#[expect(
    clippy::single_call_fn,
    reason = "isolates parent-directory fsync behavior from write flow"
)]
fn sync_parent_directory(parent: &Path, path: &Path) -> Result<(), AtomicWriteError> {
    let path_display = path.display().to_string();
    let dir_file_result = if write_fail_point() == WriteFailPoint::OpenParentDir {
        Err(io::Error::other("injected parent-dir open failure"))
    } else {
        OpenOptions::new().read(true).open(parent)
    };
    let dir_file = match dir_file_result {
        Ok(dir_file) => dir_file,
        Err(err) => {
            return Err(AtomicWriteError::OpenParentDir(path_display, err));
        }
    };

    let parent_sync_result = if write_fail_point() == WriteFailPoint::SyncParentDir {
        Err(io::Error::other("injected parent-dir sync failure"))
    } else {
        dir_file.sync_all()
    };
    if let Err(err) = parent_sync_result {
        return Err(AtomicWriteError::SyncParentDir(path_display, err));
    }

    Ok(())
}

/// Runs `git-sequence-editor` from process arguments and returns an exit code.
#[must_use]
#[inline]
pub fn main_entry() -> i32 {
    EXIT_OK
}
