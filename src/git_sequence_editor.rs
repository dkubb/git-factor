//! `git-sequence-editor` is a strict `GIT_SEQUENCE_EDITOR` helper that rewrites
//! interactive rebase todo lists.

#![expect(
    clippy::implicit_return,
    reason = "sequence-editor workflow uses expression tails for direct control flow"
)]
/// CLI argument model for `git-sequence-editor`.
#[path = "git_sequence_editor/cli.rs"]
mod cli;
#[cfg(test)]
#[path = "git_sequence_editor/proptests.rs"]
mod proptests;
/// Todo parsing and rewrite logic for `git-sequence-editor`.
#[path = "git_sequence_editor/todo.rs"]
mod todo;

use core::sync::atomic::{AtomicU8, Ordering};
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::process;
#[cfg(test)]
use std::sync::{Mutex, MutexGuard};

use clap::Parser as _;
use thiserror::Error;

use self::cli::Cli;
use self::todo::{
    build_factor_insertion, build_requested_actions, rewrite_todo_with_factor, todo_shas_in,
    validate_todo_format,
};

#[cfg(test)]
use crate::test_support::{OrAbort as _, ResultOrAbort as _};

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

/// Writes `content` to `path` atomically via a same-directory temp file and rename.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "isolates atomic write behavior from todo transform logic"
    )
)]
fn write_file_atomic(path: &Path, content: &str) -> Result<(), AtomicWriteError> {
    let target = match AtomicWriteTarget::new(path) {
        Ok(target) => target,
        Err(err) => return Err(err),
    };
    let pid = process::id();

    for attempt in 0..TEMP_FILE_ATTEMPTS_MAX {
        let mut temp_name = target.file_name.clone();
        temp_name.push(format!(".tmp{pid}.{attempt}"));
        let temp_path = target.parent.join(temp_name);

        let mut file = match OpenOptions::new()
            .create_new(true)
            .truncate(false)
            .write(true)
            .open(&temp_path)
        {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(AtomicWriteError::CreateTemp(
                    target.path.display().to_string(),
                    err,
                ));
            }
        };

        let write_result = if write_fail_point() == WriteFailPoint::Write {
            Err(io::Error::other("injected write failure"))
        } else {
            file.write_all(content.as_bytes())
        };
        if let Err(err) = write_result {
            let _ignored = fs::remove_file(&temp_path);
            return Err(AtomicWriteError::WriteTemp(
                target.path.display().to_string(),
                err,
            ));
        }

        let sync_result = if write_fail_point() == WriteFailPoint::Sync {
            Err(io::Error::other("injected sync failure"))
        } else {
            file.sync_all()
        };
        if let Err(err) = sync_result {
            let _ignored = fs::remove_file(&temp_path);
            return Err(AtomicWriteError::SyncTemp(
                target.path.display().to_string(),
                err,
            ));
        }

        drop(file);

        if let Err(err) = fs::rename(&temp_path, target.path) {
            let _ignored = fs::remove_file(&temp_path);
            return Err(AtomicWriteError::ReplaceFile(
                target.path.display().to_string(),
                err,
            ));
        }

        #[cfg(unix)]
        if let Err(err) = sync_parent_directory(target.parent, target.path) {
            return Err(err);
        }

        return Ok(());
    }

    Err(AtomicWriteError::TempNameExhausted(
        target.path.display().to_string(),
    ))
}

/// Runs the editor logic.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "preserves one orchestrator for parse-validate-rewrite flow"
    )
)]
fn run_for(cli: &Cli) -> Result<(), SequenceEditorError> {
    let content = match fs::read_to_string(cli.file()) {
        Ok(content) => content,
        Err(err) => return Err(SequenceEditorError::ReadTodo(err)),
    };
    if let Err(err) = validate_todo_format(&content) {
        return Err(SequenceEditorError::Todo(err));
    }
    let todo_shas = todo_shas_in(&content);
    let requested = match build_requested_actions(cli, &todo_shas) {
        Ok(requested) => requested,
        Err(err) => return Err(SequenceEditorError::Todo(err)),
    };
    let factor_insertion = match build_factor_insertion(cli, &todo_shas) {
        Ok(factor_insertion) => factor_insertion,
        Err(err) => return Err(SequenceEditorError::Todo(err)),
    };

    let rewrite = rewrite_todo_with_factor(&content, &requested, factor_insertion.as_ref());
    if let Err(err) = write_file_atomic(cli.file(), rewrite.output()) {
        return Err(SequenceEditorError::AtomicWrite(err));
    }

    for warning in rewrite.warnings() {
        write_stderr_line(warning.as_str());
    }

    Ok(())
}

/// Runs `git-sequence-editor` from parsed CLI arguments and returns an exit code.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "centralizes clap parse and exit-code behavior in one helper"
    )
)]
fn run_with_args_vec(args: Vec<OsString>) -> i32 {
    match Cli::try_parse_from(args) {
        Ok(cli) => run_for(&cli).map_or_else(
            |error| {
                let message = error.to_string();
                write_stderr_line(message.as_str());
                EditorExitCode::RuntimeError.code()
            },
            |()| EditorExitCode::Ok.code(),
        ),
        Err(error) => {
            let _ignored = error.print();
            error.exit_code()
        }
    }
}

/// Runs `git-sequence-editor` from parsed CLI arguments and returns an exit code.
#[cfg(test)]
fn main_entry_with_args_vec(args: Vec<OsString>) -> i32 {
    run_with_args_vec(args)
}

/// Runs `git-sequence-editor` from process arguments and returns an exit code.
#[must_use]
#[inline]
pub fn main_entry() -> i32 {
    let args = env::args_os().collect::<Vec<OsString>>();
    run_with_args_vec(args)
}

#[cfg(test)]
fn atomic_write_fixture(name: &str) -> (MutexGuard<'static, ()>, tempfile::TempDir, PathBuf) {
    let guard = lock_write_fail_point_test();
    set_write_fail_point(WriteFailPoint::None);
    let directory = tempfile::TempDir::new().or_abort("writer fixture");
    let path = directory.path().join(name);
    (guard, directory, path)
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "preserve the established inline test layout"
)]
mod tests {
    #[path = "write_file_atomic.rs"]
    mod main_entry;

    use alloc::collections::{BTreeMap, BTreeSet};
    use std::path::Path;

    use tempfile::TempDir;

    use crate::non_empty_string::NonEmptyString;

    use super::todo::{
        Action, TodoSha, is_hex40, parse_todo_action, parse_todo_sha, resolve_requested_sha,
        rewrite_todo, validate_todo_format,
    };
    use super::*;

    const EXIT_OK: i32 = 0;
    const EXIT_ERROR: i32 = 1;
    const EXIT_USAGE: i32 = 2;

    #[test]
    fn rewrite_todo_rewrites_action_for_matching_sha_only() {
        let requested = BTreeMap::from([(TodoSha::new("def5678").or_abort(""), Action::Edit)]);
        let input = "\
pick abc1234 first\n\
pick def5678 second\n\
exec echo hi\n\
";

        let (output, warnings) = rewrite_todo(input, &requested).into_parts();
        assert_eq!(warnings, Vec::<String>::new());
        assert_eq!(
            output,
            "\
pick abc1234 first\n\
edit def5678 second\n\
exec echo hi\n\
"
        );
    }

    #[test]
    fn rewrite_todo_warns_when_action_is_already_set() {
        let requested = BTreeMap::from([(TodoSha::new("abc1234").or_abort(""), Action::Pick)]);
        let input = "pick abc1234 first\n";
        let (output, warnings) = rewrite_todo(input, &requested).into_parts();
        assert_eq!(output, "pick abc1234 first\n");
        assert_eq!(
            warnings,
            vec!["WARN: abc1234: requested 'pick', but todo already had 'pick'".to_owned()]
        );
    }

    #[test]
    fn resolve_requested_sha_requires_exact_match_for_short_sha() {
        let todo_shas = BTreeSet::from([TodoSha::new("abc1234").or_abort("")]);
        let err = resolve_requested_sha("abc", &todo_shas).err_or_abort("");
        assert_eq!(err.to_string(), "sha not present in todo: abc");
    }

    #[test]
    fn is_hex40_accepts_uppercase_hex() {
        let sha = "A".repeat(todo::FULL_HEX_SHA_LEN);
        assert!(is_hex40(sha.as_str()));
    }

    #[test]
    fn validate_no_duplicates_rejects_duplicates() {
        let shas = vec![
            TodoSha::new("abc1234").or_abort(""),
            TodoSha::new("abc1234").or_abort(""),
        ];
        let err = todo::build_requested_actions(
            &Cli::for_tests(shas, vec![], Path::new("todo").to_path_buf(), vec![]),
            &BTreeSet::from([TodoSha::new("abc1234").or_abort("")]),
        )
        .err_or_abort("");
        assert_eq!(err.to_string(), "duplicate drop sha: abc1234");
    }

    #[test]
    fn build_requested_actions_rejects_cross_action_duplicates() {
        let content = "pick abc1234 first\n";
        let todo_shas = todo_shas_in(content);
        let cli = Cli::for_tests(
            vec![TodoSha::new("abc1234").or_abort("")],
            vec![],
            Path::new("todo").to_path_buf(),
            vec![TodoSha::new("abc1234").or_abort("")],
        );
        let err = build_requested_actions(&cli, &todo_shas).err_or_abort("");
        assert_eq!(err.to_string(), "sha specified multiple times: abc1234");
    }

    #[test]
    fn todo_sha_new_rejects_empty_token() {
        let err = TodoSha::new("").err_or_abort("");
        assert_eq!(
            err.to_string(),
            "internal error: todo sha was unexpectedly empty"
        );
    }

    #[test]
    fn parse_todo_action_ignores_blank_and_comment_lines() {
        assert_eq!(parse_todo_action(""), None);
        assert_eq!(parse_todo_action("   "), None);
        assert_eq!(parse_todo_action("# comment"), None);
        assert_eq!(parse_todo_action("   # comment"), None);
    }

    #[test]
    fn validate_todo_format_accepts_supported_actions() {
        let content = [
            "pick abc1234 first",
            "r abc1234 reword subject",
            "e abc1234 edit subject",
            "s abc1234 squash subject",
            "f abc1234 fixup subject",
            "d abc1234 drop subject",
            "x echo hi",
            "b",
            "l topic",
            "t topic",
            "m -C deadbeef topic",
            "noop",
            "u refs/heads/main",
            "# comment",
            "   # indented comment",
            "",
        ]
        .join("\n");

        validate_todo_format(&content).or_abort("");
    }

    #[test]
    fn validate_todo_format_rejects_unsupported_actions() {
        let content = "unknown abc1234 subject\n";
        let err = validate_todo_format(content).err_or_abort("");
        assert_eq!(err.to_string(), "unsupported todo action: unknown");
    }

    #[test]
    fn parse_todo_sha_ignores_blank_comment_and_non_commit_actions() {
        assert_eq!(parse_todo_sha(""), None);
        assert_eq!(parse_todo_sha("   "), None);
        assert_eq!(parse_todo_sha("# comment"), None);
        assert_eq!(parse_todo_sha("   # comment"), None);
        assert_eq!(parse_todo_sha("exec echo hi"), None);
        assert_eq!(parse_todo_sha("break"), None);
        assert_eq!(parse_todo_sha("label topic"), None);
        assert_eq!(parse_todo_sha("reset topic"), None);
        assert_eq!(parse_todo_sha("merge -C deadbeef topic"), None);
        assert_eq!(parse_todo_sha("noop"), None);
        assert_eq!(parse_todo_sha("update-ref refs/heads/main"), None);
    }

    #[test]
    fn parse_todo_sha_accepts_short_commit_actions() {
        assert_eq!(parse_todo_sha("r abc1234 reword subject"), Some("abc1234"));
        assert_eq!(parse_todo_sha("s abc1234 squash subject"), Some("abc1234"));
        assert_eq!(parse_todo_sha("f abc1234 fixup subject"), Some("abc1234"));
    }

    #[test]
    fn run_for_reports_missing_todo_file() {
        let dir = TempDir::new().or_abort("");
        let todo_path = dir.path().join("missing-todo");
        let cli = Cli::for_tests(vec![], vec![], todo_path, vec![]);
        let err = run_for(&cli).err_or_abort("").to_string();
        assert!(
            err.starts_with("failed to read todo file:"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn run_for_propagates_requested_action_resolution_errors() {
        let _guard = lock_write_fail_point_test();
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().or_abort("");
        let todo_path = dir.path().join("git-rebase-todo");
        fs::write(&todo_path, "pick abc1234 first\n").or_abort("");
        let cli = Cli::for_tests(
            vec![TodoSha::new("deadbeef").or_abort("")],
            vec![],
            todo_path,
            vec![],
        );

        let err = run_for(&cli).err_or_abort("").to_string();
        assert_eq!(err, "sha not present in todo: deadbeef");
    }

    #[test]
    fn run_for_propagates_atomic_write_errors() {
        let _guard = lock_write_fail_point_test();
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().or_abort("");
        let todo_path = dir.path().join("git-rebase-todo");
        fs::write(&todo_path, "pick abc1234 first\n").or_abort("");
        let cli = Cli::for_tests(
            vec![],
            vec![],
            todo_path,
            vec![TodoSha::new("abc1234").or_abort("")],
        );

        set_write_fail_point(WriteFailPoint::Write);
        let err = run_for(&cli).err_or_abort("").to_string();
        assert!(
            err.contains("failed to write temporary todo file"),
            "unexpected error: {err}"
        );

        set_write_fail_point(WriteFailPoint::None);
    }

    #[test]
    fn run_for_writes_idempotent_action_warnings() {
        let _guard = lock_write_fail_point_test();
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().or_abort("");
        let todo_path = dir.path().join("git-rebase-todo");
        fs::write(&todo_path, "pick abc1234 first\n").or_abort("");
        let cli = Cli::for_tests(
            vec![],
            vec![],
            todo_path.clone(),
            vec![TodoSha::new("abc1234").or_abort("")],
        );

        run_for(&cli).or_abort("");
        let content = fs::read_to_string(&todo_path).or_abort("");
        assert_eq!(content, "pick abc1234 first\n");
    }

    #[test]
    fn write_file_atomic_rejects_paths_without_parent_or_file_name() {
        let _guard = lock_write_fail_point_test();
        set_write_fail_point(WriteFailPoint::None);
        let no_parent = write_file_atomic(Path::new(""), "content")
            .err_or_abort("")
            .to_string();
        assert!(
            no_parent.contains("failed to determine parent directory"),
            "unexpected error: {no_parent}"
        );

        let no_file_name = write_file_atomic(Path::new("."), "content")
            .err_or_abort("")
            .to_string();
        assert!(
            no_file_name.contains("failed to determine file name"),
            "unexpected error: {no_file_name}"
        );
    }

    #[test]
    fn write_file_atomic_reports_temp_create_failure_for_missing_parent() {
        let _guard = lock_write_fail_point_test();
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().or_abort("");
        let path = dir.path().join("missing").join("todo");
        let err = write_file_atomic(&path, "content")
            .err_or_abort("")
            .to_string();
        assert!(
            err.contains("failed to create temporary todo file"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn write_file_atomic_reports_write_and_sync_failpoints() {
        let _guard = lock_write_fail_point_test();
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().or_abort("");
        let path = dir.path().join("todo");

        set_write_fail_point(WriteFailPoint::Write);
        let write_err = write_file_atomic(&path, "content")
            .err_or_abort("")
            .to_string();
        assert!(
            write_err.contains("failed to write temporary todo file"),
            "unexpected error: {write_err}"
        );

        set_write_fail_point(WriteFailPoint::Sync);
        let sync_err = write_file_atomic(&path, "content")
            .err_or_abort("")
            .to_string();
        assert!(
            sync_err.contains("failed to sync temporary todo file"),
            "unexpected error: {sync_err}"
        );

        set_write_fail_point(WriteFailPoint::None);
    }

    #[cfg(unix)]
    #[test]
    fn write_file_atomic_reports_parent_directory_open_and_sync_failpoints() {
        let _guard = lock_write_fail_point_test();
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().or_abort("");
        let path = dir.path().join("todo");

        set_write_fail_point(WriteFailPoint::OpenParentDir);
        let open_err = write_file_atomic(&path, "pick abc1234 first\n")
            .err_or_abort("")
            .to_string();
        assert!(
            open_err.contains("failed to open parent directory"),
            "unexpected error: {open_err}"
        );

        set_write_fail_point(WriteFailPoint::SyncParentDir);
        let sync_err = write_file_atomic(&path, "pick abc1234 first\n")
            .err_or_abort("")
            .to_string();
        assert!(
            sync_err.contains("failed to sync parent directory"),
            "unexpected error: {sync_err}"
        );

        set_write_fail_point(WriteFailPoint::None);
    }

    #[test]
    fn write_file_atomic_reports_rename_failure_for_directory_target() {
        let _guard = lock_write_fail_point_test();
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().or_abort("");
        let target = dir.path().join("todo");
        fs::create_dir_all(&target).or_abort("");

        let err = write_file_atomic(&target, "content")
            .err_or_abort("")
            .to_string();
        assert!(
            err.contains("failed to atomically replace todo file"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn write_file_atomic_skips_existing_temp_slot_and_succeeds() {
        let _guard = lock_write_fail_point_test();
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().or_abort("");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 old\n").or_abort("");

        let pid = process::id();
        fs::write(dir.path().join(format!("todo.tmp{pid}.0")), "taken").or_abort("");

        write_file_atomic(&path, "pick abc1234 new\n").or_abort("");
        let actual = fs::read_to_string(&path).or_abort("");
        assert_eq!(actual, "pick abc1234 new\n");
    }

    #[test]
    fn write_file_atomic_reports_exhausted_temp_names() {
        let _guard = lock_write_fail_point_test();
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().or_abort("");
        let path = dir.path().join("todo");
        let pid = process::id();
        for attempt in u32::MIN..TEMP_FILE_ATTEMPTS_MAX {
            let name = format!("todo.tmp{pid}.{attempt}");
            fs::write(dir.path().join(name), "taken").or_abort("");
        }

        let err = write_file_atomic(&path, "content")
            .err_or_abort("")
            .to_string();
        assert!(
            err.contains("failed to create a unique temporary file"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn main_entry_with_args_vec_returns_usage_for_parse_errors() {
        let code = super::main_entry_with_args_vec(vec![OsString::from("git-sequence-editor")]);
        assert_eq!(code, EXIT_USAGE);
    }

    #[test]
    fn main_entry_with_args_vec_returns_usage_for_unknown_flag() {
        let code = super::main_entry_with_args_vec(vec![
            OsString::from("git-sequence-editor"),
            OsString::from("--unknown"),
        ]);
        assert_eq!(code, EXIT_USAGE);
    }

    #[test]
    fn main_entry_with_args_vec_returns_ok_for_help() {
        let code = super::main_entry_with_args_vec(vec![
            OsString::from("git-sequence-editor"),
            OsString::from("--help"),
        ]);
        assert_eq!(code, EXIT_OK);
    }

    #[test]
    fn main_entry_with_args_vec_returns_ok_for_version() {
        let code = super::main_entry_with_args_vec(vec![
            OsString::from("git-sequence-editor"),
            OsString::from("--version"),
        ]);
        assert_eq!(code, EXIT_OK);
    }

    #[test]
    fn main_entry_with_args_vec_returns_error_for_runtime_failures() {
        let code = super::main_entry_with_args_vec(vec![
            OsString::from("git-sequence-editor"),
            OsString::from("missing-todo"),
        ]);
        assert_eq!(code, EXIT_ERROR);
    }

    #[test]
    fn main_entry_with_args_vec_returns_ok_for_valid_invocation() {
        let _guard = lock_write_fail_point_test();
        let dir = TempDir::new().or_abort("");
        let todo_path = dir.path().join("git-rebase-todo");
        fs::write(&todo_path, "pick abc1234 first\n").or_abort("");
        set_write_fail_point(WriteFailPoint::None);

        let code = super::main_entry_with_args_vec(vec![
            OsString::from("git-sequence-editor"),
            todo_path.into_os_string(),
        ]);
        assert_eq!(code, EXIT_OK);
    }

    #[test]
    fn main_entry_reports_usage_under_test_harness_arguments() {
        assert_eq!(super::main_entry(), EXIT_USAGE);
    }

    #[test]
    fn run_for_cli_returns_error_for_invalid_todo_action() {
        let _guard = lock_write_fail_point_test();
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().or_abort("");
        let todo_path = dir.path().join("git-rebase-todo");
        fs::write(&todo_path, "unknown abc1234 first\n").or_abort("");
        let cli = Cli::for_tests(vec![], vec![], todo_path, vec![]);
        let err = run_for(&cli).err_or_abort("").to_string();
        assert_eq!(err, "unsupported todo action: unknown".to_owned());
    }

    #[test]
    fn run_for_cli_rewrites_todo_with_factor_insertions() {
        let _guard = lock_write_fail_point_test();
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().or_abort("");
        let todo_path = dir.path().join("git-rebase-todo");
        fs::write(
            &todo_path,
            "\
pick abc1234 first\n\
pick def5678 second\n\
",
        )
        .or_abort("");
        let cli = Cli::for_tests_with_factor(
            Some(NonEmptyString::try_from("echo begin".to_owned()).or_abort("")),
            Some(NonEmptyString::try_from("echo preflight".to_owned()).or_abort("")),
            Some(TodoSha::new("abc1234").or_abort("")),
            vec![],
            vec![],
            todo_path.clone(),
            vec![],
        );

        run_for(&cli).or_abort("");

        let actual = fs::read_to_string(&todo_path).or_abort("");
        assert_eq!(
            actual,
            "\
pick abc1234 first\n\
exec echo preflight\n\
exec echo begin\n\
break\n\
pick def5678 second\n\
"
        );
    }

    #[test]
    fn run_for_cli_returns_error_for_incomplete_factor_args() {
        let _guard = lock_write_fail_point_test();
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().or_abort("");
        let todo_path = dir.path().join("git-rebase-todo");
        fs::write(&todo_path, "pick abc1234 first\n").or_abort("");
        let cli = Cli::for_tests_with_factor(
            Some(NonEmptyString::try_from("echo begin".to_owned()).or_abort("")),
            None,
            Some(TodoSha::new("abc1234").or_abort("")),
            vec![],
            vec![],
            todo_path,
            vec![],
        );

        let err = run_for(&cli).err_or_abort("").to_string();
        assert_eq!(
            err,
            "invalid factor arguments: factor-target, factor-preflight, and factor-begin must all be provided"
        );
    }

    #[test]
    fn run_for_cli_returns_error_for_factor_target_missing_from_todo() {
        let _guard = lock_write_fail_point_test();
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().or_abort("");
        let todo_path = dir.path().join("git-rebase-todo");
        fs::write(&todo_path, "pick abc1234 first\n").or_abort("");
        let cli = Cli::for_tests_with_factor(
            Some(NonEmptyString::try_from("echo begin".to_owned()).or_abort("")),
            Some(NonEmptyString::try_from("echo preflight".to_owned()).or_abort("")),
            Some(TodoSha::new("def5678").or_abort("")),
            vec![],
            vec![],
            todo_path,
            vec![],
        );

        let err = run_for(&cli).err_or_abort("").to_string();
        assert_eq!(err, "sha not present in todo: def5678");
    }

    #[test]
    fn option_or_abort_returns_inner_value() {
        assert_eq!(Some("value").or_abort(""), "value");
    }

    #[test]
    fn result_or_abort_returns_inner_value() {
        let value: Result<&str, &str> = Ok("value");
        assert_eq!(value.or_abort(""), "value");
    }

    #[test]
    fn result_err_or_abort_returns_inner_error() {
        let value: Result<&str, &str> = Err("error");
        assert_eq!(value.err_or_abort(""), "error");
    }

    #[test]
    fn proptest_run_unit_suite() {
        build_requested_actions_rejects_cross_action_duplicates();
        is_hex40_accepts_uppercase_hex();
        main_entry_reports_usage_under_test_harness_arguments();
        main_entry_with_args_vec_returns_error_for_runtime_failures();
        main_entry_with_args_vec_returns_ok_for_help();
        main_entry_with_args_vec_returns_ok_for_valid_invocation();
        main_entry_with_args_vec_returns_ok_for_version();
        main_entry_with_args_vec_returns_usage_for_parse_errors();
        main_entry_with_args_vec_returns_usage_for_unknown_flag();
        option_or_abort_returns_inner_value();
        result_err_or_abort_returns_inner_error();
        result_or_abort_returns_inner_value();
        parse_todo_action_ignores_blank_and_comment_lines();
        parse_todo_sha_accepts_short_commit_actions();
        parse_todo_sha_ignores_blank_comment_and_non_commit_actions();
        resolve_requested_sha_requires_exact_match_for_short_sha();
        rewrite_todo_rewrites_action_for_matching_sha_only();
        rewrite_todo_warns_when_action_is_already_set();
        run_for_cli_returns_error_for_invalid_todo_action();
        run_for_propagates_atomic_write_errors();
        run_for_propagates_requested_action_resolution_errors();
        run_for_reports_missing_todo_file();
        run_for_writes_idempotent_action_warnings();
        todo_sha_new_rejects_empty_token();
        validate_no_duplicates_rejects_duplicates();
        validate_todo_format_accepts_supported_actions();
        validate_todo_format_rejects_unsupported_actions();
        write_file_atomic_rejects_paths_without_parent_or_file_name();
        write_file_atomic_reports_exhausted_temp_names();
        write_file_atomic_reports_parent_directory_open_and_sync_failpoints();
        write_file_atomic_reports_rename_failure_for_directory_target();
        write_file_atomic_reports_temp_create_failure_for_missing_parent();
        write_file_atomic_reports_write_and_sync_failpoints();
        write_file_atomic_skips_existing_temp_slot_and_succeeds();
    }
}
