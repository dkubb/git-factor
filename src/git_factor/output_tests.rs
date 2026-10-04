mod file_path {
    mod from {
        mod vec {
            #[test]
            fn preserves_utf8_path_bytes_as_text() {
                let input = b" spaced\npath ".to_vec();
                let actual = super::super::super::super::FilePath::from(input);
                assert!(
                    matches!(actual, super::super::super::super::FilePath::Text(value) if value == " spaced\npath ")
                );
            }
            #[test]
            fn preserves_non_utf8_path_bytes() {
                let input = vec![b'p', 255, b' '];
                let actual = super::super::super::super::FilePath::from(input.clone());
                assert!(
                    matches!(actual, super::super::super::super::FilePath::Bytes { bytes } if bytes == input)
                );
            }
        }
    }
}
mod closing_status {
    use super::*;
    #[test]
    fn preserves_terminal_progress_and_original_identity_after_cleanup() {
        for count in [0, 256, u64::MAX] {
            for root in [false, true] {
                for in_progress in [false, true] {
                    verify_closing_status(ClosingOutcome::Aborted(count), root, in_progress);
                    if let Some(value) = NonZeroU64::new(count) {
                        verify_closing_status(ClosingOutcome::Complete(value), root, in_progress);
                    }
                }
            }
        }
    }
}
mod checkpoint_completed {
    use super::*;
    #[test]
    fn preserves_full_width_progress_and_output_failures() {
        for count in [
            NonZeroU64::MIN,
            NonZeroU64::new(256).or_abort("positive count"),
            NonZeroU64::MAX,
        ] {
            for operation in [
                CheckpointCompletionOperation::Abort,
                CheckpointCompletionOperation::Continue,
                CheckpointCompletionOperation::Finish,
            ] {
                verify_checkpoint_completed(operation, count);
            }
        }
    }
}
mod inactive_status {
    use super::*;
    #[test]
    fn preserves_null_session_and_output_failures() {
        verify_inactive_status();
    }
}
mod checkpoint_status {
    use super::*;
    #[test]
    fn reports_original_target_and_checkpoint_without_fabricated_index() {
        for phase in checkpoint_phases() {
            for count in [0, 256, u64::MAX] {
                for root in [false, true] {
                    for in_progress in [false, true] {
                        verify_checkpoint_status(&CheckpointFacts {
                            target: &"a".repeat(40),
                            checkpoint: &"b".repeat(40),
                            commit_count: NonZeroU64::MAX,
                            split_count: count,
                            phase,
                            root,
                            in_progress,
                        });
                    }
                }
            }
        }
    }
}
mod checkpoint_counts {
    mod new {
        use super::super::*;
        #[test]
        fn preserves_nonempty_range_and_full_width_progress() {
            for commit_count in [NonZeroU64::MIN, NonZeroU64::MAX] {
                for split_count in [0, u64::MAX] {
                    verify_checkpoint_counts(commit_count, split_count);
                }
            }
        }
    }
}
mod checkpoint_remaining {
    use super::*;
    #[test]
    fn preserves_full_width_transitions_paths_and_query_failures() {
        for count in [0, 256, u64::MAX] {
            verify_checkpoint_remaining(CheckpointTransition::Retried(count));
            if let Some(committed) = NonZeroU64::new(count) {
                verify_checkpoint_remaining(CheckpointTransition::Committed(committed));
            }
        }
    }
}
mod started_checkpoint {
    use super::*;
    #[test]
    fn preserves_metadata_paths_guidance_and_failure_boundaries() {
        for resumed in [false, true] {
            verify_started_details(
                "a\tb\nc",
                "Add combined changes",
                2,
                3,
                resumed,
                NonZeroUsize::new(257).or_abort("nonempty large range"),
            );
        }
        verify_started_details("file", "Add largest range", 0, 0, false, NonZeroUsize::MAX);
        verify_started_bytes();
        verify_started_guidance();
        verify_started_output_failures();
        verify_started_refusals();
    }
}

mod empty_selection {
    use super::*;

    #[test]
    fn emits_admission_refusal_and_preserves_output_failures() {
        verify_empty_selection();
    }
}

mod aborted {
    use super::*;

    #[test]
    fn preserves_observed_rebase_and_output_failures() {
        verify_aborted(false);
        verify_aborted(true);
    }
}

mod gate_failed {
    use super::*;

    #[test]
    fn preserves_gate_identity_exit_and_recovery_with_output_failures() {
        let failure: i32 = -1;
        for origin in gate_failure_origins() {
            for command in ["test", "a\n\"b\\"] {
                for exit_code in [
                    NonZeroI32::MIN,
                    NonZeroI32::new(failure).or_abort("failure"),
                    NonZeroI32::MAX,
                ] {
                    verify_gate_failed(command, exit_code, origin);
                }
            }
        }
    }
}

mod start_recovery {
    use super::*;

    #[test]
    fn supplies_exact_recovery_actions_and_propagates_output_failures() {
        for operation in [
            CommandOperation::Start,
            CommandOperation::Continue,
            CommandOperation::Finish,
        ] {
            verify_recovery(None, operation);
            verify_recovery(NonZeroUsize::new(1), operation);
            verify_recovery(NonZeroUsize::new(2), operation);
        }
    }
}

use core::cell::{Cell, RefCell};
use core::num::{NonZeroI32, NonZeroI64, NonZeroUsize};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};

use proptest::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

use super::*;
use crate::git_factor::{BaseParent, CommitSpan, Env, Io, REAL_FS, Runner};
use crate::test_support::{OrAbort as _, ResultOrAbort as _};
use nonempty::NonEmpty;

/// Independent expected checkpoint facts shared by direct and generated contracts.
pub(in crate::git_factor::output) struct CheckpointFacts<'id> {
    /// Last completed branch tip.
    pub checkpoint: &'id str,
    /// Nonempty original range size.
    pub commit_count: NonZeroU64,
    /// Current native rebase observation.
    pub in_progress: bool,
    /// Durable round phase.
    pub phase: CheckpointPhase,
    /// Root boundary of the original selection.
    pub root: bool,
    /// Completed atom count.
    pub split_count: u64,
    /// Original selected tip.
    pub target: &'id str,
}

#[derive(Default)]
struct Buffer(RefCell<String>);

impl Io for Buffer {
    fn err(&self, _text: &str) -> io::Result<()> {
        Ok(())
    }
    fn errln(&self, _text: &str) -> io::Result<()> {
        Ok(())
    }
    fn out(&self, text: &str) -> io::Result<()> {
        self.0.borrow_mut().push_str(text);
        Ok(())
    }
    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }
}

struct ChangeRunner {
    diff: Vec<u8>,
    failed_query: Option<&'static str>,
    paths: Vec<u8>,
    root: Vec<u8>,
}

impl Runner for ChangeRunner {
    fn output(
        &self,
        _bin: &str,
        args: &[&str],
        _envs: &[(&str, Option<&str>)],
        _cwd: &Path,
    ) -> io::Result<Output> {
        if self.failed_query == Some("spawn") {
            return Err(io::Error::other("cannot launch Git"));
        }
        let bytes = match *args {
            ["diff", "--numstat", "-z", "--no-renames", "--no-relative"] => self.diff.clone(),
            ["ls-files", "--others", "--exclude-standard", "-z"] => self.paths.clone(),
            ["rev-parse", "--show-toplevel"] => self.root.clone(),
            _ => return Err(io::Error::other("unexpected query")),
        };
        Ok(Output {
            status: ExitStatus::from_raw(if args.first().copied() == self.failed_query {
                256
            } else {
                0
            }),
            stdout: bytes,
            stderr: b"query failed".to_vec(),
        })
    }
    fn status(
        &self,
        _bin: &str,
        _args: &[&str],
        _envs: &[(&str, Option<&str>)],
        _quiet: bool,
        _cwd: &Path,
    ) -> io::Result<ExitStatus> {
        Err(io::Error::other("unexpected mutation"))
    }
}

struct AgentEnvironment(bool);

impl Env for AgentEnvironment {
    fn current_dir(&self) -> io::Result<PathBuf> {
        Err(io::Error::other("unexpected cwd lookup"))
    }
    fn current_exe(&self) -> io::Result<PathBuf> {
        Err(io::Error::other("unexpected executable lookup"))
    }
    fn var_os(&self, key: &str) -> Option<OsString> {
        (self.0 && key == "CLAUDECODE").then(|| OsString::from("1"))
    }
}

struct ResultOutputFailure {
    fail_at: usize,
    out: RefCell<String>,
    writes: Cell<usize>,
}

impl Io for ResultOutputFailure {
    fn err(&self, _text: &str) -> io::Result<()> {
        Ok(())
    }
    fn errln(&self, _text: &str) -> io::Result<()> {
        Ok(())
    }
    fn out(&self, text: &str) -> io::Result<()> {
        let write = self
            .writes
            .get()
            .checked_add(1)
            .or_abort("bounded output writes");
        self.writes.set(write);
        if write == self.fail_at {
            return Err(io::Error::other("result write failed"));
        }
        self.out.borrow_mut().push_str(text);
        Ok(())
    }
    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }
}

/// Observes the public status contract with deterministic IO capabilities.

#[test]
fn numstat_rejects_incomplete_or_invalid_records() {
    for bytes in [
        b"1\t2\tpath".as_slice(),
        b"1\0",
        b"1\t2\0",
        b"1\t2\t\0",
        b"x\t2\tpath\0",
        b"1\tx\tpath\0",
        b"-\t2\tpath\0",
        b"1\t-\tpath\0",
        b"18446744073709551616\t0\tpath\0",
        b"1\t2\tpath\0\0",
        b"\xff\t2\tpath\0",
        b"1\t\xff\tpath\0",
    ] {
        assert!(parse_numstat(bytes).is_err(), "accepted {bytes:?}");
    }
    assert!(parse_numstat(b"").or_abort("empty changes").is_empty());
}

#[test]
fn binary_changes_preserve_path_bytes() {
    let changes = parse_numstat(b"-\t-\t\xff\t\n\0").or_abort("binary change");
    let value = serde_json::to_value(changes).or_abort("serialize change");
    let expected: Value =
        serde_json::from_str(r#"[{"path":{"bytes":[255,9,10]},"kind":"binary"}]"#)
            .or_abort("expected JSON");
    assert_eq!(value, expected);
}

pub(in crate::git_factor::output) fn verify_started_refusals() {
    for (root, diff, paths, failed_query) in [
        (
            b"/test\n".as_slice(),
            b"".as_slice(),
            b"".as_slice(),
            Some("rev-parse"),
        ),
        (b"/test\n", b"", b"", Some("diff")),
        (b"/test\n", b"", b"", Some("ls-files")),
        (b"", b"", b"", None),
        (b"\n", b"", b"", None),
        (b"/test\n", b"", b"path", None),
        (b"/test\n", b"", b"\0", None),
        (b"/test\n", b"", b"path\0\0", None),
        (b"/test\n", b"", b"", Some("spawn")),
        (b"/test\n", b"malformed\0", b"", None),
        (b"/test\n", b"1\t2\tpath", b"", None),
        (b"/test\n", b"1\0", b"", None),
        (b"/test\n", b"1\t2\0", b"", None),
        (b"/test\n", b"1\t2\t\0", b"", None),
        (b"/test\n", b"x\t2\tpath\0", b"", None),
        (b"/test\n", b"1\tx\tpath\0", b"", None),
        (b"/test\n", b"-\t2\tpath\0", b"", None),
        (b"/test\n", b"1\t-\tpath\0", b"", None),
        (b"/test\n", b"18446744073709551616\t0\tpath\0", b"", None),
        (b"/test\n", b"1\t2\tpath\0\0", b"", None),
        (b"/test\n", b"\xff\t2\tpath\0", b"", None),
        (b"/test\n", b"1\t\xff\tpath\0", b"", None),
    ] {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = ChangeRunner {
            diff: diff.to_vec(),
            failed_query,
            paths: paths.to_vec(),
            root: root.to_vec(),
        };
        let buffer = Buffer::default();
        let ctx = Ctx {
            cwd: dir.path().to_path_buf(),
            env: &AgentEnvironment(false),
            fs: &REAL_FS,
            io: &buffer,
            runner: &runner,
        };
        let commit = CommitSha::new("a".repeat(40)).or_abort("valid commit");
        let span = CommitSpan::new(NonEmpty::new(commit), BaseParent::Commit);
        assert!(
            started_checkpoint(
                &ctx,
                span.tip_commit(),
                NonZeroUsize::new(1).or_abort("singleton range"),
                SelectionOperation::Start,
                "message",
                "aaaaaaa"
            )
            .is_err()
        );
        assert!(buffer.0.borrow().is_empty(), "partial result emitted");
    }
}

pub(in crate::git_factor::output) fn verify_started_guidance() {
    let dir = TempDir::new().or_abort("tempdir");
    let references = dir.path().join("references");
    fs::create_dir_all(&references).or_abort("create references");
    let reference = references.join("rust.md");
    fs::write(&reference, "Rust guidance").or_abort("write reference");
    let runner = ChangeRunner {
        diff: Vec::new(),
        failed_query: None,
        paths: Vec::new(),
        root: format!("{}\n", dir.path().display()).into_bytes(),
    };
    let buffer = Buffer::default();
    let ctx = Ctx {
        cwd: dir.path().to_path_buf(),
        env: &AgentEnvironment(true),
        fs: &REAL_FS,
        io: &buffer,
        runner: &runner,
    };
    let commit = CommitSha::new("a".repeat(40)).or_abort("valid commit");
    let span = CommitSpan::new(NonEmpty::new(commit), BaseParent::Commit);
    started_checkpoint(
        &ctx,
        span.tip_commit(),
        NonZeroUsize::new(1).or_abort("singleton range"),
        SelectionOperation::Start,
        "message",
        "aaaaaaa",
    )
    .or_abort("emit start");
    let value: Value = serde_json::from_str(&buffer.0.borrow()).or_abort("JSON result");
    assert_eq!(
        value.get("references"),
        Some(&serde_json::to_value([reference]).or_abort("reference JSON"))
    );
    assert_eq!(
        value.get("guidance"),
        Some(
            &serde_json::to_value([
                "Stage one independently valid atomic change.",
                "Use one concrete action in the commit message.",
                "Submit each atom through git factor so its gates run.",
                "Above 50% context, pause and ask the user to /compact.",
                "Continue splitting until the session is complete.",
            ])
            .or_abort("guidance JSON")
        )
    );
}

proptest! {
    #[test]
    fn abort_reports_observed_rebase_and_only_applicable_recovery(in_progress in any::<bool>()) {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = ChangeRunner { diff: Vec::new(), failed_query: None, paths: Vec::new(), root: Vec::new() };
        let buffer = Buffer::default();
        let ctx = Ctx { cwd: dir.path().to_path_buf(), env: &AgentEnvironment(false), fs: &REAL_FS, io: &buffer, runner: &runner };
        aborted(&ctx, in_progress).or_abort("emit abort");
        let text = buffer.0.borrow();
        let value: Value = serde_json::from_str(&text).or_abort("one JSON result");
        prop_assert_eq!(text.lines().count(), 1);
        prop_assert!(text.ends_with('\n'));
        prop_assert_eq!(value.get("operation").and_then(Value::as_str), Some("abort"));
        prop_assert_eq!(value.pointer("/rebase/in_progress").and_then(Value::as_bool), Some(in_progress));
        let recovery = value.pointer("/actions/abort_rebase");
        if in_progress {
            let expected = serde_json::to_value(["git", "rebase", "--abort"]).or_abort("recovery JSON");
            prop_assert_eq!(recovery, Some(&expected));
        } else {
            prop_assert!(recovery.is_none());
            prop_assert_eq!(value.get("actions").and_then(Value::as_object).map(serde_json::Map::len), Some(0));
        }
    }
}

#[test]
fn paused_start_supplies_all_recovery_commands_in_one_result() {
    let dir = TempDir::new().or_abort("tempdir");
    let runner = ChangeRunner {
        diff: Vec::new(),
        failed_query: None,
        paths: Vec::new(),
        root: Vec::new(),
    };
    let buffer = Buffer::default();
    let ctx = Ctx {
        cwd: dir.path().to_path_buf(),
        env: &AgentEnvironment(false),
        fs: &REAL_FS,
        io: &buffer,
        runner: &runner,
    };
    start_recovery(&ctx, CommandOperation::Start).or_abort("emit recovery");
    let expected = "{\"actions\":{\"amend\":[\"git\",\"commit\",\"--amend\",\"--no-edit\"],\"continue_factor\":[\"git\",\"factor\",\"--continue\"],\"stage\":[\"git\",\"add\",\"<paths>\"]},\"operation\":\"start\",\"result\":\"recovery_required\"}\n";
    assert_eq!(*buffer.0.borrow(), expected);
}

proptest! {
    #[test]
    fn gate_failure_preserves_command_exit_code_and_recovery_boundary(
        command in "[^\x00]{1,100}",
        exit_code in any::<NonZeroI32>(),
        candidate in any::<bool>(),
    ) {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = ChangeRunner { diff: Vec::new(), failed_query: None, paths: Vec::new(), root: Vec::new() };
        let buffer = Buffer::default();
        let ctx = Ctx { cwd: dir.path().to_path_buf(), env: &AgentEnvironment(false), fs: &REAL_FS, io: &buffer, runner: &runner };
        let configured_command = NonEmptyString::try_from(command.clone()).or_abort("nonempty command");
        let origin = if candidate { GateFailureOrigin::Candidate(CompletionOperation::Continue) } else { GateFailureOrigin::Baseline(SelectionOperation::Start) };
        gate_failed(&ctx, origin, &configured_command, exit_code).or_abort("emit failure");
        let text = buffer.0.borrow();
        let value: Value = serde_json::from_str(&text).or_abort("one JSON result");
        prop_assert_eq!(text.lines().count(), 1);
        prop_assert!(text.ends_with('\n'));
        prop_assert_eq!(value.get("operation").and_then(Value::as_str), Some(if candidate { "continue" } else { "start" }));
        prop_assert_eq!(value.get("result").and_then(Value::as_str), Some("gate_failed"));
        prop_assert_eq!(value.pointer("/gate/command").and_then(Value::as_str), Some(command.as_str()));
        prop_assert_eq!(value.pointer("/gate/exit_code").and_then(Value::as_i64), Some(NonZeroI64::from(exit_code).get()));
        prop_assert_eq!(value.pointer("/actions/submit").is_some(), candidate);
    }
}

pub(in crate::git_factor::output) fn verify_started_details(
    path: &str,
    message: &str,
    added: u64,
    deleted: u64,
    resumed: bool,
    commit_count: NonZeroUsize,
) {
    let dir = TempDir::new().or_abort("tempdir");
    let runner = ChangeRunner {
        diff: format!("{added}\t{deleted}\t{path}\0").into_bytes(),
        failed_query: None,
        paths: format!("{path}\0").into_bytes(),
        root: b"/empty-test-repository\n".to_vec(),
    };
    let buffer = Buffer::default();
    let ctx = Ctx {
        cwd: dir.path().to_path_buf(),
        env: &AgentEnvironment(false),
        fs: &REAL_FS,
        io: &buffer,
        runner: &runner,
    };
    let expected_commit = "f".repeat(40);
    let commit = CommitSha::new(expected_commit.clone()).or_abort("selected tip");
    let operation = if resumed {
        SelectionOperation::Continue
    } else {
        SelectionOperation::Start
    };
    started_checkpoint(&ctx, &commit, commit_count, operation, message, "aaaaaaa")
        .or_abort("emit start");
    let text = buffer.0.borrow();
    let value: Value = serde_json::from_str(&text).or_abort("one JSON result");
    assert!(text.ends_with('\n'));
    assert_eq!(text.lines().count(), 1);
    assert_eq!(
        value.pointer("/operation").and_then(Value::as_str),
        Some(if resumed { "continue" } else { "start" })
    );
    assert_eq!(
        value.pointer("/target/commit").and_then(Value::as_str),
        Some(expected_commit.as_str())
    );
    assert_eq!(
        value
            .pointer("/target/short_commit")
            .and_then(Value::as_str),
        Some("aaaaaaa")
    );
    assert_eq!(
        value.pointer("/target/message").and_then(Value::as_str),
        Some(message)
    );
    assert_eq!(
        value
            .pointer("/changes/unstaged/0/path")
            .and_then(Value::as_str),
        Some(path)
    );
    assert_eq!(
        value
            .pointer("/changes/unstaged/0/added")
            .and_then(Value::as_u64),
        Some(added)
    );
    assert_eq!(
        value
            .pointer("/changes/unstaged/0/deleted")
            .and_then(Value::as_u64),
        Some(deleted)
    );
    assert_eq!(
        value
            .pointer("/changes/untracked/0")
            .and_then(Value::as_str),
        Some(path)
    );
    assert_eq!(
        value
            .pointer("/target/commit_count")
            .and_then(Value::as_u64),
        Some(u64::try_from(commit_count.get()).or_abort("count fits JSON number"))
    );
}

pub(in crate::git_factor::output) fn verify_started_bytes() {
    let runner = ChangeRunner {
        diff: b"-\t-\t\xff\t\n\0".to_vec(),
        failed_query: None,
        paths: b"\xff\0".to_vec(),
        root: b"/empty-test-repository\n".to_vec(),
    };
    let buffer = Buffer::default();
    let ctx = Ctx {
        cwd: PathBuf::from("/empty-test-repository"),
        env: &AgentEnvironment(false),
        fs: &REAL_FS,
        io: &buffer,
        runner: &runner,
    };
    let span = CommitSpan::new(
        NonEmpty::new(CommitSha::new("a".repeat(40)).or_abort("commit")),
        BaseParent::Commit,
    );
    started_checkpoint(
        &ctx,
        span.tip_commit(),
        NonZeroUsize::new(1).or_abort("singleton range"),
        SelectionOperation::Start,
        "message",
        "aaaaaaa",
    )
    .or_abort("emit binary paths");
    let value: Value = serde_json::from_str(&buffer.0.borrow()).or_abort("JSON result");
    let changes: Value = serde_json::from_str(r#"{"unstaged":[{"path":{"bytes":[255,9,10]},"kind":"binary"}],"untracked":[{"bytes":[255]}]}"#).or_abort("expected paths");
    assert_eq!(value.get("changes"), Some(&changes));
}

pub(in crate::git_factor::output) fn verify_started_output_failures() {
    for fail_at in [1, 2] {
        let failure = ResultOutputFailure {
            fail_at,
            out: RefCell::new(String::new()),
            writes: Cell::new(0),
        };
        let runner = ChangeRunner {
            diff: Vec::new(),
            failed_query: None,
            paths: Vec::new(),
            root: b"/empty-test-repository\n".to_vec(),
        };
        let ctx = Ctx {
            cwd: PathBuf::from("/empty-test-repository"),
            env: &AgentEnvironment(false),
            fs: &REAL_FS,
            io: &failure,
            runner: &runner,
        };
        let span = CommitSpan::new(
            NonEmpty::new(CommitSha::new("a".repeat(40)).or_abort("commit")),
            BaseParent::Commit,
        );
        let error = started_checkpoint(
            &ctx,
            span.tip_commit(),
            NonZeroUsize::new(1).or_abort("singleton range"),
            SelectionOperation::Start,
            "message",
            "aaaaaaa",
        )
        .err_or_abort("failed selection write");
        assert!(
            matches!(error, FactorError::Io(inner) if inner.to_string() == "result write failed")
        );
        assert_eq!(failure.writes.get(), fail_at);
        let text = failure.out.borrow();
        if fail_at == 1 {
            assert!(text.is_empty());
        } else {
            let value: Value =
                serde_json::from_str(&text).or_abort("complete JSON before newline failure");
            assert_eq!(
                value.get("operation").and_then(Value::as_str),
                Some("start")
            );
            assert!(!text.ends_with('\n'));
        }
    }
}

pub(in crate::git_factor::output) fn verify_aborted(in_progress: bool) {
    let expected: Value = serde_json::from_str(if in_progress {
        r#"{"operation":"abort","rebase":{"in_progress":true},"actions":{"abort_rebase":["git","rebase","--abort"]}}"#
    } else {
        r#"{"operation":"abort","rebase":{"in_progress":false},"actions":{}}"#
    }).or_abort("expected abort");
    verify_result(|ctx| aborted(ctx, in_progress), &expected);
}

fn verify_result<F>(render: F, expected: &Value)
where
    F: Fn(&Ctx<'_>) -> Result<(), FactorError>,
{
    let runner = ChangeRunner {
        diff: Vec::new(),
        failed_query: Some("spawn"),
        paths: Vec::new(),
        root: Vec::new(),
    };
    verify_result_with(render, expected, &runner, Path::new("/result-contract"));
}

fn verify_result_with<F>(render: F, expected: &Value, runner: &dyn Runner, cwd: &Path)
where
    F: Fn(&Ctx<'_>) -> Result<(), FactorError>,
{
    let buffer = Buffer::default();
    let ctx = Ctx {
        cwd: cwd.to_path_buf(),
        env: &AgentEnvironment(false),
        fs: &REAL_FS,
        io: &buffer,
        runner,
    };
    render(&ctx).or_abort("emit result");
    let text = buffer.0.borrow();
    assert_eq!(text.lines().count(), 1);
    assert!(text.ends_with('\n'));
    let actual: Value = serde_json::from_str(&text).or_abort("result JSON");
    assert_eq!(&actual, expected);
    for fail_at in [1, 2] {
        let failure = ResultOutputFailure {
            fail_at,
            out: RefCell::new(String::new()),
            writes: Cell::new(0),
        };
        let failed_ctx = Ctx {
            io: &failure,
            ..ctx.clone()
        };
        let error = render(&failed_ctx).err_or_abort("failed result write");
        assert!(
            matches!(error, FactorError::Io(inner) if inner.to_string() == "result write failed")
        );
        assert_eq!(failure.writes.get(), fail_at);
        let partial = failure.out.borrow();
        if fail_at == 1 {
            assert!(partial.is_empty());
        } else {
            let before_newline: Value =
                serde_json::from_str(&partial).or_abort("result before newline failure");
            assert_eq!(&before_newline, expected);
            assert!(!partial.ends_with('\n'));
        }
    }
}

pub(in crate::git_factor::output) fn verify_gate_failed(
    command: &str,
    exit_code: NonZeroI32,
    origin: GateFailureOrigin,
) {
    let (candidate, operation_name) = match origin {
        GateFailureOrigin::Baseline(SelectionOperation::Start) => (false, "start"),
        GateFailureOrigin::Baseline(SelectionOperation::Continue) => (false, "continue"),
        GateFailureOrigin::Candidate(CompletionOperation::Continue) => (true, "continue"),
        GateFailureOrigin::Candidate(CompletionOperation::Finish) => (true, "finish"),
    };
    let encoded_command = serde_json::to_string(command).or_abort("encode expected command");
    let (actions, guidance) = if candidate {
        (
            r#"{"submit":["git","factor","--continue","--message","<message>"]}"#,
            "Adjust staged changes so the gate passes, then submit the atom again.",
        )
    } else {
        (
            "{}",
            "Fix the gate environment or amend the current commit without changing its tree, then run git factor --continue. To change the tree, abort the session, repair the checkpoint, and start again.",
        )
    };
    let expected: Value = serde_json::from_str(&format!(
        r#"{{"actions":{actions},"gate":{{"command":{encoded_command},"exit_code":{exit_code}}},"guidance":["{guidance}"],"operation":"{operation_name}","result":"gate_failed"}}"#,
    )).or_abort("expected gate failure");
    let configured = NonEmptyString::try_from(command.to_owned()).or_abort("nonempty command");
    verify_result(
        |ctx| gate_failed(ctx, origin, &configured, exit_code),
        &expected,
    );
}

pub(in crate::git_factor::output) fn verify_recovery(
    fail_at: Option<NonZeroUsize>,
    operation: CommandOperation,
) {
    let mut expected: Value = serde_json::from_str(r#"{"actions":{"amend":["git","commit","--amend","--no-edit"],"continue_factor":["git","factor","--continue"],"stage":["git","add","<paths>"]},"operation":"start","result":"recovery_required"}"#).or_abort("expected recovery");
    *expected
        .get_mut("operation")
        .or_abort("expected operation field") = Value::String(
        match operation {
            CommandOperation::Start => "start",
            CommandOperation::Continue => "continue",
            CommandOperation::Finish => "finish",
        }
        .to_owned(),
    );
    let runner = ChangeRunner {
        diff: Vec::new(),
        failed_query: Some("spawn"),
        paths: Vec::new(),
        root: Vec::new(),
    };
    let buffer = Buffer::default();
    let failure = fail_at.map(|write| ResultOutputFailure {
        fail_at: write.get(),
        out: RefCell::new(String::new()),
        writes: Cell::new(0),
    });
    let output: &dyn Io = if let Some(failing) = failure.as_ref() {
        failing
    } else {
        &buffer
    };
    let ctx = Ctx {
        cwd: PathBuf::from("/recovery-contract"),
        env: &AgentEnvironment(false),
        fs: &REAL_FS,
        io: output,
        runner: &runner,
    };
    let result = start_recovery(&ctx, operation);
    if let Some(failed) = failure {
        let error = result.err_or_abort("failed recovery write");
        assert!(
            matches!(error, FactorError::Io(inner) if inner.to_string() == "result write failed")
        );
        assert_eq!(Some(failed.writes.get()), fail_at.map(NonZeroUsize::get));
        let text = failed.out.borrow();
        if failed.fail_at == 1 {
            assert!(text.is_empty());
        } else {
            let actual: Value =
                serde_json::from_str(&text).or_abort("recovery before newline failure");
            assert_eq!(actual, expected);
            assert!(!text.ends_with('\n'));
        }
    } else {
        result.or_abort("emit recovery");
        let text = buffer.0.borrow();
        let actual: Value = serde_json::from_str(&text).or_abort("recovery JSON");
        assert_eq!(actual, expected);
        assert!(text.ends_with('\n'));
        assert_eq!(text.lines().count(), 1);
    }
}

pub(in crate::git_factor::output) fn verify_empty_selection() {
    let expected: Value = serde_json::from_str(
        "{\"operation\":\"start\",\"reason\":\"empty_change\",\"result\":\"refused\"}",
    )
    .or_abort("empty selection schema");
    verify_result(empty_selection, &expected);
}
/// Supplies every phase as an independent serialization oracle.
pub(in crate::git_factor::output) fn checkpoint_phases() -> [CheckpointPhase; 6] {
    [
        CheckpointPhase::Closing,
        CheckpointPhase::Opening,
        CheckpointPhase::Preparing,
        CheckpointPhase::Replaying,
        CheckpointPhase::Selecting,
        CheckpointPhase::Verified,
    ]
}

/// Observes full-width counts without reconstructing an internal commit index.
pub(in crate::git_factor::output) fn verify_checkpoint_counts(
    commit_count: NonZeroU64,
    split_count: u64,
) {
    let counts = CheckpointCounts::new(commit_count, split_count);
    assert_eq!(counts.commit_count, commit_count);
    assert_eq!(counts.split_count, split_count);
}

/// Observes the complete checkpoint status and both stdout write boundaries.
pub(in crate::git_factor::output) fn verify_checkpoint_status(facts: &CheckpointFacts<'_>) {
    let (phase, required) = match facts.phase {
        CheckpointPhase::Closing => ("closing", false),
        CheckpointPhase::Opening => ("opening", true),
        CheckpointPhase::Preparing => ("preparing", false),
        CheckpointPhase::Replaying => ("replaying", true),
        CheckpointPhase::Selecting => ("selecting", true),
        CheckpointPhase::Verified => ("verified", true),
    };
    let target = CommitSha::new(facts.target.to_owned()).or_abort("target identity");
    let checkpoint = CommitSha::new(facts.checkpoint.to_owned()).or_abort("checkpoint identity");
    let counts = CheckpointCounts::new(facts.commit_count, facts.split_count);
    let expected: Value = serde_json::from_str(&format!(
        r#"{{"operation":"status","session":{{"checkpoint":"{}","phase":"{phase}","rebase":{{"required":{required},"in_progress":{}}},"split_count":{},"target":{{"commit":"{}","commit_count":{},"span_starts_at_root":{}}}}}}}"#,
        facts.checkpoint,
        facts.in_progress,
        facts.split_count,
        facts.target,
        facts.commit_count.get(),
        facts.root,
    ))
    .or_abort("expected checkpoint status");
    verify_result(
        |ctx| {
            super::checkpoint_status(
                ctx,
                &target,
                &checkpoint,
                counts,
                facts.phase,
                facts.root,
                facts.in_progress,
            )
        },
        &expected,
    );
}

/// Observes completion counts beyond the legacy eight-bit ceiling.
pub(in crate::git_factor::output) fn verify_checkpoint_completed(
    operation: CheckpointCompletionOperation,
    count: NonZeroU64,
) {
    let name = match operation {
        CheckpointCompletionOperation::Abort => "abort",
        CheckpointCompletionOperation::Continue => "continue",
        CheckpointCompletionOperation::Finish => "finish",
    };
    let expected: Value = serde_json::from_str(&format!(
        r#"{{"operation":"{name}","result":"complete","split_count":{}}}"#,
        count.get(),
    ))
    .or_abort("expected checkpoint completion");
    verify_result(
        |ctx| super::checkpoint_completed(ctx, operation, count),
        &expected,
    );
}

/// Observes the absence of a session and exact single-result output.
pub(in crate::git_factor::output) fn verify_inactive_status() {
    let expected: Value = serde_json::from_str(r#"{"operation":"status","session":null}"#)
        .or_abort("expected inactive status");
    verify_result(super::inactive_status, &expected);
}

/// Observes checkpoint transitions with lossless paths and query refusal boundaries.
pub(in crate::git_factor::output) fn verify_checkpoint_remaining(transition: CheckpointTransition) {
    let dir = TempDir::new().or_abort("checkpoint pool fixture");
    let mut runner = ChangeRunner {
        diff: b"2\t3\tfile.txt\0-\t-\t\xff\0".to_vec(),
        failed_query: None,
        paths: b"new\tfile\n\0\xff\0".to_vec(),
        root: format!("{}\n", dir.path().display()).into_bytes(),
    };
    let (operation, result, count) = match transition {
        CheckpointTransition::Committed(count) => ("continue", "committed", count.get()),
        CheckpointTransition::Retried(count) => ("retry", "attempt_discarded", count),
    };
    let expected: Value = serde_json::from_str(&format!(
        r#"{{"operation":"{operation}","result":"{result}","split_count":{count},"actions":{{"abort":["git","factor","--abort"],"submit":["git","factor","--continue","--message","<message>"]}},"changes":{{"unstaged":[{{"path":"file.txt","kind":"text","added":2,"deleted":3}},{{"path":{{"bytes":[255]}},"kind":"binary"}}],"untracked":["new\tfile\n",{{"bytes":[255]}}]}},"guidance":["Stage one independently valid atomic change.","Use one concrete action in the commit message.","Submit each atom through git factor so its gates run."],"references":[]}}"#,
    ))
    .or_abort("expected remaining checkpoint");
    verify_result_with(
        |ctx| super::checkpoint_remaining(ctx, transition),
        &expected,
        &runner,
        dir.path(),
    );
    for query in ["spawn", "rev-parse", "diff", "ls-files"] {
        runner.failed_query = Some(query);
        let buffer = Buffer::default();
        let ctx = Ctx {
            cwd: dir.path().to_path_buf(),
            env: &AgentEnvironment(false),
            fs: &REAL_FS,
            io: &buffer,
            runner: &runner,
        };
        let error =
            super::checkpoint_remaining(&ctx, transition).err_or_abort("refused change query");
        assert!(matches!(error, FactorError::GitCommand(_)));
        assert!(buffer.0.borrow().is_empty());
    }
}

pub(in crate::git_factor::output) fn verify_closing_status(
    outcome: ClosingOutcome,
    root: bool,
    in_progress: bool,
) {
    let target = CommitSha::new("a".repeat(40)).or_abort("original identity");
    let checkpoint = CommitSha::new("b".repeat(40)).or_abort("checkpoint identity");
    let (result, count) = match outcome {
        ClosingOutcome::Complete(value) => ("complete", value.get()),
        ClosingOutcome::Aborted(value) => ("aborted", value),
    };
    let expected: Value = serde_json::from_str(&format!(
        r#"{{"operation":"status","session":{{"checkpoint":"{}","outcome":"{result}","phase":"closing","split_count":{count},"target":{{"commit":"{}","span_starts_at_root":{root}}},"rebase":{{"required":false,"in_progress":{in_progress}}}}}}}"#,
        checkpoint.as_str(),
        target.as_str(),
    ))
    .or_abort("expected closing status");
    verify_result(
        |ctx| closing_status(ctx, &target, &checkpoint, outcome, root, in_progress),
        &expected,
    );
}

pub(in crate::git_factor::output) fn gate_failure_origins() -> [GateFailureOrigin; 4] {
    [
        GateFailureOrigin::Baseline(SelectionOperation::Start),
        GateFailureOrigin::Baseline(SelectionOperation::Continue),
        GateFailureOrigin::Candidate(CompletionOperation::Continue),
        GateFailureOrigin::Candidate(CompletionOperation::Finish),
    ]
}
