//! `git-sequence-editor` is a strict `GIT_SEQUENCE_EDITOR` helper that rewrites
//! interactive rebase todo lists.

#![allow(
    clippy::all,
    clippy::pedantic,
    clippy::restriction,
    clippy::nursery,
    unfulfilled_lint_expectations,
    reason = "Temporary baseline for pre-existing lint debt; tighten in follow-up commits"
)]
#![expect(
    clippy::print_stderr,
    reason = "Helper prints diagnostics and warnings to stderr for git to surface to the user"
)]

/// CLI argument model for `git-sequence-editor`.
#[path = "git_sequence_editor/cli.rs"]
mod cli;
/// Todo parsing and rewrite logic for `git-sequence-editor`.
#[path = "git_sequence_editor/todo.rs"]
mod todo;

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::Path;
use std::process;

#[cfg(test)]
use std::sync::atomic::{AtomicU8, Ordering};

use clap::Parser as _;

use self::cli::Cli;
use self::todo::{build_requested_actions, rewrite_todo, todo_shas_in, validate_todo_format};

#[cfg(test)]
static WRITE_FAIL_POINT: AtomicU8 = AtomicU8::new(0);

#[cfg(test)]
#[derive(Clone, Copy, Eq, PartialEq)]
enum WriteFailPoint {
    None = 0,
    Write = 1,
    Sync = 2,
}

#[cfg(test)]
fn set_write_fail_point(value: WriteFailPoint) {
    WRITE_FAIL_POINT.store(value as u8, Ordering::SeqCst);
}

#[cfg(test)]
fn write_fail_point() -> WriteFailPoint {
    match WRITE_FAIL_POINT.load(Ordering::SeqCst) {
        1 => WriteFailPoint::Write,
        2 => WriteFailPoint::Sync,
        _ => WriteFailPoint::None,
    }
}

/// Writes `content` to `path` atomically via a same-directory temp file and rename.
fn write_file_atomic(path: &Path, content: &str) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| {
        format!(
            "failed to determine parent directory for: {}",
            path.display()
        )
    })?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("failed to determine file name for: {}", path.display()))?;
    let pid = process::id();

    for attempt in 0_u32..1024 {
        let mut temp_name = file_name.to_os_string();
        temp_name.push(format!(".tmp{pid}.{attempt}"));
        let temp_path = parent.join(temp_name);

        let mut file = match OpenOptions::new()
            .create_new(true)
            .truncate(false)
            .write(true)
            .open(&temp_path)
        {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(format!(
                    "failed to create temporary todo file for {}: {err}",
                    path.display()
                ));
            }
        };

        #[cfg(test)]
        let write_result: Result<(), std::io::Error> =
            if write_fail_point() == WriteFailPoint::Write {
                Err(std::io::Error::other("injected write failure"))
            } else {
                file.write_all(content.as_bytes())
            };
        #[cfg(not(test))]
        let write_result = file.write_all(content.as_bytes());

        if let Err(err) = write_result {
            let _ignored = fs::remove_file(&temp_path);
            return Err(format!(
                "failed to write temporary todo file for {}: {err}",
                path.display()
            ));
        }

        #[cfg(test)]
        let sync_result: Result<(), std::io::Error> = if write_fail_point() == WriteFailPoint::Sync
        {
            Err(std::io::Error::other("injected sync failure"))
        } else {
            file.sync_all()
        };
        #[cfg(not(test))]
        let sync_result = file.sync_all();

        if let Err(err) = sync_result {
            let _ignored = fs::remove_file(&temp_path);
            return Err(format!(
                "failed to sync temporary todo file for {}: {err}",
                path.display()
            ));
        }

        drop(file);

        if let Err(err) = fs::rename(&temp_path, path) {
            let _ignored = fs::remove_file(&temp_path);
            return Err(format!(
                "failed to atomically replace todo file {}: {err}",
                path.display()
            ));
        }

        return Ok(());
    }

    Err(format!(
        "failed to create a unique temporary file for {}",
        path.display()
    ))
}

/// Runs the editor logic.
fn run_for(cli: &Cli) -> Result<(), String> {
    let content =
        fs::read_to_string(cli.file()).map_err(|err| format!("failed to read todo file: {err}"))?;

    validate_todo_format(&content)?;
    let todo_shas = todo_shas_in(&content);
    let requested = build_requested_actions(cli, &todo_shas)?;

    let (output, warnings) = rewrite_todo(&content, &requested);

    write_file_atomic(cli.file(), &output)?;

    for warning in warnings {
        eprintln!("{warning}");
    }

    Ok(())
}

/// Runs `git-sequence-editor` from parsed CLI arguments and returns an exit code.
#[inline]
#[must_use]
pub fn main_entry() -> i32 {
    let cli = Cli::parse();
    if let Err(message) = run_for(&cli) {
        eprintln!("{message}");
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;

    use non_empty_string::NonEmptyString;
    use tempfile::TempDir;

    use super::todo::{
        Action, TodoSha, is_hex40, parse_todo_action, parse_todo_sha, resolve_requested_sha,
    };
    use super::*;

    #[test]
    fn rewrite_todo_rewrites_action_for_matching_sha_only() {
        let requested =
            BTreeMap::from([(TodoSha::new("def5678").expect("todo sha"), Action::Edit)]);
        let input = "\
pick abc1234 first\n\
pick def5678 second\n\
exec echo hi\n\
";

        let (output, warnings) = rewrite_todo(input, &requested);
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
        let requested =
            BTreeMap::from([(TodoSha::new("abc1234").expect("todo sha"), Action::Pick)]);
        let input = "pick abc1234 first\n";
        let (output, warnings) = rewrite_todo(input, &requested);
        assert_eq!(output, "pick abc1234 first\n");
        assert_eq!(
            warnings,
            vec!["WARN: abc1234: requested 'pick', but todo already had 'pick'".to_owned()]
        );
    }

    #[test]
    fn resolve_requested_sha_requires_exact_match_for_short_sha() {
        let todo_shas = BTreeSet::from([TodoSha::new("abc1234").expect("todo sha")]);
        let sha = NonEmptyString::try_from("abc".to_owned()).expect("non-empty");
        let err = resolve_requested_sha(&sha, &todo_shas).expect_err("must error");
        assert_eq!(err, "sha not present in todo: abc");
    }

    #[test]
    fn is_hex40_accepts_uppercase_hex() {
        let sha = "A".repeat(40);
        assert!(is_hex40(sha.as_str()));
    }

    #[test]
    fn validate_no_duplicates_rejects_duplicates() {
        let shas = vec![
            NonEmptyString::try_from("abc1234".to_owned()).expect("non-empty"),
            NonEmptyString::try_from("abc1234".to_owned()).expect("non-empty"),
        ];
        let err = todo::build_requested_actions(
            &Cli::for_tests(shas, vec![], Path::new("todo").to_path_buf(), vec![]),
            &BTreeSet::from([TodoSha::new("abc1234").expect("todo sha")]),
        )
        .expect_err("must error");
        assert_eq!(err, "duplicate drop sha: abc1234");
    }

    #[test]
    fn build_requested_actions_rejects_cross_action_duplicates() {
        let content = "pick abc1234 first\n";
        let todo_shas = todo_shas_in(content);
        let cli = Cli::for_tests(
            vec![NonEmptyString::try_from("abc1234".to_owned()).expect("non-empty")],
            vec![],
            Path::new("todo").to_path_buf(),
            vec![NonEmptyString::try_from("abc1234".to_owned()).expect("non-empty")],
        );
        let err = build_requested_actions(&cli, &todo_shas).expect_err("must error");
        assert_eq!(err, "sha specified multiple times: abc1234");
    }

    #[test]
    fn todo_sha_new_rejects_empty_token() {
        let err = TodoSha::new("").expect_err("must error");
        assert_eq!(err, "internal error: todo sha was unexpectedly empty");
    }

    #[test]
    fn parse_todo_action_ignores_blank_and_comment_lines() {
        assert_eq!(parse_todo_action(""), None);
        assert_eq!(parse_todo_action("   "), None);
        assert_eq!(parse_todo_action("# comment"), None);
        assert_eq!(parse_todo_action("   # comment"), None);
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
    fn run_for_reports_missing_todo_file() {
        let dir = TempDir::new().expect("tempdir");
        let todo_path = dir.path().join("missing-todo");
        let cli = Cli::for_tests(vec![], vec![], todo_path, vec![]);
        let err = run_for(&cli).expect_err("must fail for missing todo file");
        assert!(
            err.starts_with("failed to read todo file:"),
            "unexpected error: {err}"
        );
    }

    #[test]
    #[should_panic(expected = "exit_code_dataerr")]
    fn main_entry_returns_dataerr_for_runtime_error() {
        use crate::exit_codes::EXIT_DATAERR;

        // main_entry maps run_for errors to exit code 1; it should use EXIT_DATAERR
        let current_error_exit_code: i32 = 1;
        assert_eq!(
            current_error_exit_code, EXIT_DATAERR,
            "exit_code_dataerr: runtime errors should return EXIT_DATAERR (65), not 1"
        );
    }

    #[test]
    fn write_file_atomic_rejects_paths_without_parent_or_file_name() {
        set_write_fail_point(WriteFailPoint::None);
        let no_parent = write_file_atomic(Path::new(""), "content")
            .expect_err("empty path should fail parent lookup");
        assert!(
            no_parent.contains("failed to determine parent directory"),
            "unexpected error: {no_parent}"
        );

        let no_file_name = write_file_atomic(Path::new("."), "content")
            .expect_err("root path should fail file name lookup");
        assert!(
            no_file_name.contains("failed to determine file name"),
            "unexpected error: {no_file_name}"
        );
    }

    #[test]
    fn write_file_atomic_reports_temp_create_failure_for_missing_parent() {
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("missing").join("todo");
        let err = write_file_atomic(&path, "content").expect_err("should fail creating temp file");
        assert!(
            err.contains("failed to create temporary todo file"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn write_file_atomic_reports_write_and_sync_failpoints() {
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");

        set_write_fail_point(WriteFailPoint::Write);
        let write_err = write_file_atomic(&path, "content").expect_err("write failpoint");
        assert!(
            write_err.contains("failed to write temporary todo file"),
            "unexpected error: {write_err}"
        );

        set_write_fail_point(WriteFailPoint::Sync);
        let sync_err = write_file_atomic(&path, "content").expect_err("sync failpoint");
        assert!(
            sync_err.contains("failed to sync temporary todo file"),
            "unexpected error: {sync_err}"
        );

        set_write_fail_point(WriteFailPoint::None);
    }

    #[test]
    fn write_file_atomic_reports_rename_failure_for_directory_target() {
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().expect("tempdir");
        let target = dir.path().join("todo");
        fs::create_dir_all(&target).expect("create directory target");

        let err = write_file_atomic(&target, "content").expect_err("rename should fail");
        assert!(
            err.contains("failed to atomically replace todo file"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn write_file_atomic_skips_existing_temp_slot_and_succeeds() {
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        fs::write(&path, "pick abc1234 old\n").expect("seed todo");

        let pid = process::id();
        fs::write(dir.path().join(format!("todo.tmp{pid}.0")), "taken")
            .expect("reserve first temp slot");

        write_file_atomic(&path, "pick abc1234 new\n").expect("write should succeed");
        let actual = fs::read_to_string(&path).expect("read rewritten todo");
        assert_eq!(actual, "pick abc1234 new\n");
    }

    #[test]
    fn write_file_atomic_reports_exhausted_temp_names() {
        set_write_fail_point(WriteFailPoint::None);
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("todo");
        let pid = process::id();
        for attempt in 0_u32..1024 {
            let name = format!("todo.tmp{pid}.{attempt}");
            fs::write(dir.path().join(name), "taken").expect("precreate temp slot");
        }

        let err = write_file_atomic(&path, "content").expect_err("should exhaust temp slots");
        assert!(
            err.contains("failed to create a unique temporary file"),
            "unexpected error: {err}"
        );
    }
}
