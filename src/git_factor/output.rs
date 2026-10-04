/// Generated contracts for public JSON output.
#[cfg(test)]
#[path = "output_proptests.rs"]
mod proptests;

/// Contract tests for JSON output and machine-data ingress.
#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;

use core::num::{NonZeroI32, NonZeroU64, NonZeroUsize};
use core::str::from_utf8;
#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt as _;
use std::path::PathBuf;

use serde::Serialize;

use super::{CommitSha, Ctx, FactorError, NonEmptyString, git_raw_output, non_empty_msg};

/// Git's rebase requirements and observed progress.
#[derive(Serialize)]
struct Rebase {
    /// Whether a rebase currently exists in Git.
    in_progress: bool,
    /// Whether the session requires replay to finish.
    required: bool,
}

/// Durable checkpoint phase reported to CLI consumers.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::git_factor) enum CheckpointPhase {
    /// Terminal session cleanup is pending.
    Closing,
    /// The next native rebase is being opened.
    Opening,
    /// The next round's durable objects are being prepared.
    Preparing,
    /// The accepted atom and remainder are being replayed.
    Replaying,
    /// The remaining change is available for staging.
    Selecting,
    /// The replayed history passed terminal verification.
    Verified,
}

/// The public command that initiated a validation or replay result.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::git_factor) enum CommandOperation {
    /// Resume replay or submit the staged candidate.
    Continue,
    /// Retain the complete remaining change.
    Finish,
    /// Admit and expose the selected range.
    Start,
}

/// Terminal progress after all active-round resources have been released.
#[derive(Clone, Copy)]
pub(in crate::git_factor) enum ClosingOutcome {
    /// The latest attempt was abandoned without losing prior atoms.
    Aborted(u64),
    /// All selected changes became one or more completed atoms.
    Complete(NonZeroU64),
}

/// Original identity retained after historical range objects may be collected.
#[derive(Serialize)]
struct ClosingTarget<'session> {
    /// Original selected tip identity.
    commit: &'session CommitSha,
    /// Whether the original range began at the root.
    span_starts_at_root: bool,
}

/// Terminal status using only surviving checkpoint ancestry.
#[derive(Serialize)]
struct ClosingStatus<'session> {
    /// Last completed branch tip.
    checkpoint: &'session CommitSha,
    /// Terminal outcome.
    outcome: &'static str,
    /// Terminal cleanup phase.
    phase: &'static str,
    /// Required and observed native rebase facts.
    rebase: Rebase,
    /// Number of completed atoms derived from surviving ancestry.
    split_count: u64,
    /// Original selected identity without unavailable historical counts.
    target: ClosingTarget<'session>,
}

/// Derived progress counters; the original range is nonempty.
#[derive(Clone, Copy)]
pub(in crate::git_factor) struct CheckpointCounts {
    /// Size of the originally selected nonempty commit range.
    commit_count: NonZeroU64,
    /// Number of completed checkpoint atoms.
    split_count: u64,
}
impl CheckpointCounts {
    /// Combines nonempty original range size with derived completed-atom count.
    #[cfg_attr(
        not(test),
        expect(
            clippy::single_call_fn,
            reason = "derived checkpoint counts retain their nonempty range proof"
        )
    )]
    pub(in crate::git_factor) const fn new(commit_count: NonZeroU64, split_count: u64) -> Self {
        Self {
            commit_count,
            split_count,
        }
    }
}
/// Identity and extent of the original selected change.
#[derive(Serialize)]
struct CheckpointTarget<'session> {
    /// Original selected tip.
    commit: &'session CommitSha,
    /// Original nonempty range size.
    commit_count: NonZeroU64,
    /// Whether the range begins at the repository root.
    span_starts_at_root: bool,
}

/// Current durable checkpoint and round observations.
#[derive(Serialize)]
struct CheckpointStatus<'session> {
    /// Last completed branch tip.
    checkpoint: &'session CommitSha,
    /// Durable round phase.
    phase: CheckpointPhase,
    /// Required and observed native rebase facts.
    rebase: Rebase,
    /// Number of completed checkpoint atoms.
    split_count: u64,
    /// Original selected change identity.
    target: CheckpointTarget<'session>,
}

/// A status result; null denotes the absence of a session.
#[derive(Serialize)]
struct Status<Session> {
    /// Identifies the command that produced this result.
    operation: &'static str,
    /// Absence represents an inactive session.
    session: Option<Session>,
}

/// A path encoded without losing non-UTF-8 bytes.
#[derive(Serialize)]
#[serde(untagged)]
enum FilePath {
    /// Raw bytes when the path is not UTF-8.
    Bytes {
        /// Original path bytes.
        bytes: Vec<u8>,
    },
    /// A usual UTF-8 path.
    Text(String),
}

impl From<Vec<u8>> for FilePath {
    fn from(bytes: Vec<u8>) -> Self {
        match String::from_utf8(bytes) {
            Ok(text) => Self::Text(text),
            Err(error) => Self::Bytes {
                bytes: error.into_bytes(),
            },
        }
    }
}

/// Text counts or the binary classification supplied by Git.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ChangeSize {
    /// Git reports no line counts for this binary file.
    Binary,
    /// A file whose line counts are meaningful.
    Text {
        /// Number of inserted lines.
        added: u64,
        /// Number of removed lines.
        deleted: u64,
    },
}

/// One file in the unstaged change.
#[derive(Serialize)]
struct FileChange {
    /// File path relative to the repository root.
    path: FilePath,
    /// Size or binary classification.
    #[serde(flatten)]
    size: ChangeSize,
}

/// Files currently available for selection.
#[derive(Serialize)]
struct Changes {
    /// Tracked changes relative to the index.
    unstaged: Vec<FileChange>,
    /// Paths absent from the index.
    untracked: Vec<FilePath>,
}

/// Original range identity and metadata.
#[derive(Serialize)]
struct RangeTarget<'target> {
    /// Full identity of the selected tip.
    commit: &'target str,
    /// Number of original commits whose combined change is exposed.
    commit_count: usize,
    /// Original tip message.
    message: &'target str,
    /// Git's unambiguous display abbreviation.
    short_commit: &'target str,
}

/// The next action and available recovery command.
#[derive(Serialize)]
struct Actions {
    /// Arguments to abandon the session.
    abort: [&'static str; 3],
    /// Arguments to submit the staged selection.
    submit: [&'static str; 5],
}

/// Commands that open a selection pool.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::git_factor) enum SelectionOperation {
    /// Resume a session after repairing its baseline.
    Continue,
    /// Start a new session.
    Start,
}

/// Information shared by every opened selection pool.
#[derive(Serialize)]
struct Selection {
    /// Commands the caller can run next.
    actions: Actions,
    /// Current pool of selectable files.
    changes: Changes,
    /// Human guidance independent of the result's machine fields.
    guidance: Vec<&'static str>,
    /// Available repository guidance files.
    references: Vec<FilePath>,
}

/// A new selection session.
#[derive(Serialize)]
struct Started<'target> {
    /// Commands the caller can run next.
    actions: Actions,
    /// Current pool of selectable files.
    changes: Changes,
    /// Human guidance independent of the result's machine fields.
    guidance: Vec<&'static str>,
    /// Command producing this result.
    operation: SelectionOperation,
    /// Available repository guidance files.
    references: Vec<FilePath>,
    /// Original selected range.
    target: RangeTarget<'target>,
}

/// A completed checkpoint or a discarded staging attempt.
#[derive(Clone, Copy)]
pub(in crate::git_factor) enum CheckpointTransition {
    /// At least one completed atom is now durable.
    Committed(NonZeroU64),
    /// Staging was reset while completed progress was retained.
    Retried(u64),
}

/// A restored pool after submission or retry.
#[derive(Serialize)]
struct Remaining {
    /// Command producing this result.
    operation: &'static str,
    /// The completed transition.
    result: &'static str,
    /// Facts shared with the session-start selection pool.
    #[serde(flatten)]
    selection: Selection,
    /// Number of submitted split commits.
    split_count: u64,
}

/// Git's observed rebase state after aborting the factor step.
#[derive(Serialize)]
struct ObservedRebase {
    /// Whether Git still has an active rebase.
    in_progress: bool,
}

/// Recovery actions for a rebase outside the aborted factor step.
#[derive(Serialize)]
struct AbortActions {
    /// Arguments to abort a rebase that is still active.
    #[serde(skip_serializing_if = "Option::is_none")]
    abort_rebase: Option<[&'static str; 3]>,
}

/// Commands that can complete a factor session.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::git_factor) enum CompletionOperation {
    /// Submission consumed the final remainder.
    Continue,
    /// Explicitly retained the final remainder.
    Finish,
}

/// The actual command cleaning up a durably completed checkpoint.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::git_factor) enum CheckpointCompletionOperation {
    /// Abort discovered that the final round had already completed.
    Abort,
    /// Resume or submission completed the selected change.
    Continue,
    /// Finish retained the final remainder.
    Finish,
}

/// The validation boundary whose gate failed.
#[derive(Clone, Copy)]
pub(in crate::git_factor) enum GateFailureOrigin {
    /// Validation of the selected baseline before opening the pool.
    Baseline(SelectionOperation),
    /// Validation of the staged selection.
    Candidate(CompletionOperation),
}

/// A failed gate's command and nonzero exit status.
#[derive(Serialize)]
struct FailedGate<'gate> {
    /// The configured shell command.
    command: &'gate str,
    /// Nonzero process exit code, or the signal fallback code.
    exit_code: NonZeroI32,
}

/// The staged submission command supplied after a candidate failure.
#[derive(Serialize)]
struct GateFailureActions {
    /// Submit after adjusting staged changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    submit: Option<[&'static str; 5]>,
}

/// Recovery commands after start pauses inside Git's rebase.
#[derive(Serialize)]
struct StartRecoveryActions {
    /// Amend the repaired baseline commit.
    amend: [&'static str; 4],
    /// Open the pool when rebase reaches the factor break.
    continue_factor: [&'static str; 3],
    /// Stage repaired paths.
    stage: [&'static str; 3],
}

/// The closed set of JSON-compatible command results.
#[derive(Serialize)]
#[serde(untagged)]
enum CommandResult<'result> {
    /// Factor state was removed; another rebase may still exist.
    Aborted {
        /// Command producing this result.
        operation: &'static str,
        /// Observed Git rebase state.
        rebase: ObservedRebase,
        /// Applicable recovery commands.
        actions: AbortActions,
    },
    /// Terminal status remains available after historical lease cleanup.
    ClosingStatus(&'result Status<ClosingStatus<'result>>),
    /// The current factor session has finished.
    Completed {
        /// Command producing this result.
        operation: CheckpointCompletionOperation,
        /// The completed transition.
        result: &'static str,
        /// Number of commits emitted from the selected change.
        split_count: NonZeroU64,
    },
    /// The selected range has no combined change to split.
    EmptySelection {
        /// Command producing this result.
        operation: &'static str,
        /// The admission refusal.
        reason: &'static str,
        /// The rejected transition.
        result: &'static str,
    },
    /// A gate rejected the baseline or staged selection.
    GateFailed {
        /// Applicable next command.
        actions: GateFailureActions,
        /// Gate identity and exit status.
        gate: FailedGate<'result>,
        /// Advice for fixing this validation boundary.
        guidance: [&'static str; 1],
        /// Command producing this result.
        operation: CommandOperation,
        /// The rejected transition.
        result: &'static str,
    },
    /// A restored selection pool.
    Remaining(&'result Remaining),
    /// Replay stopped inside a rebase and requires a repair.
    StartRecovery {
        /// Available repair and continuation commands.
        actions: StartRecoveryActions,
        /// Command producing this result.
        operation: CommandOperation,
        /// The paused transition.
        result: &'static str,
    },
    /// A new selection pool.
    Started(&'result Started<'result>),
    /// An active or inactive session query.
    Status(&'result Status<CheckpointStatus<'result>>),
}

/// Captures machine-readable Git output, rejecting failed queries.
fn git_bytes(ctx: &Ctx<'_>, args: &[&str]) -> Result<Vec<u8>, FactorError> {
    let result = git_raw_output(ctx, args)?;
    if !result.status.success() {
        return Err(FactorError::GitCommand(non_empty_msg(format!(
            "Git change query failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        ))));
    }
    Ok(result.stdout)
}

/// Rejects malformed machine data instead of guessing file boundaries.
fn malformed_changes() -> FactorError {
    FactorError::GitCommand(non_empty_msg("malformed Git change summary".to_owned()))
}

/// Decodes numstat with rename detection disabled and NUL record terminators.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "isolates Git numstat grammar from session output"
    )
)]
fn parse_numstat(bytes: &[u8]) -> Result<Vec<FileChange>, FactorError> {
    let mut changes = Vec::new();
    if bytes.is_empty() {
        return Ok(changes);
    }
    let records = bytes.strip_suffix(&[0]).ok_or_else(malformed_changes)?;
    for record in records.split(|byte| *byte == 0) {
        let fields = record.splitn(3, |byte| *byte == b'\t').collect::<Vec<_>>();
        let &[added, deleted, path] = fields.as_slice() else {
            return Err(malformed_changes());
        };
        if path.is_empty() {
            return Err(malformed_changes());
        }
        let size = if added == b"-" && deleted == b"-" {
            ChangeSize::Binary
        } else {
            ChangeSize::Text {
                added: from_utf8(added)
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .ok_or_else(malformed_changes)?,
                deleted: from_utf8(deleted)
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .ok_or_else(malformed_changes)?,
            }
        };
        changes.push(FileChange {
            path: FilePath::from(path.to_vec()),
            size,
        });
    }
    Ok(changes)
}

/// Collects normalized selection facts from lossless Git queries.
fn selection(ctx: &Ctx<'_>) -> Result<Selection, FactorError> {
    let root_output = git_bytes(ctx, &["rev-parse", "--show-toplevel"])?;
    let root_bytes = root_output
        .strip_suffix(b"\n")
        .filter(|bytes| !bytes.is_empty())
        .ok_or_else(malformed_changes)?;
    #[cfg(unix)]
    let root = PathBuf::from(OsString::from_vec(root_bytes.to_vec()));
    #[cfg(not(unix))]
    let root = PathBuf::from(from_utf8(root_bytes).map_err(|_error| malformed_changes())?);
    let mut root_ctx = ctx.clone();
    root_ctx.cwd.clone_from(&root);
    let diff = git_bytes(
        &root_ctx,
        &["diff", "--numstat", "-z", "--no-renames", "--no-relative"],
    )?;
    let paths = git_bytes(
        &root_ctx,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    let unstaged = parse_numstat(&diff)?;
    let untracked = if paths.is_empty() {
        Vec::new()
    } else {
        let records = paths.strip_suffix(&[0]).ok_or_else(malformed_changes)?;
        records
            .split(|byte| *byte == 0)
            .map(|path| {
                if path.is_empty() {
                    Err(malformed_changes())
                } else {
                    Ok(FilePath::from(path.to_vec()))
                }
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let rust_reference = root.join("references/rust.md");
    let references = if ctx.fs.exists(&rust_reference) {
        vec![FilePath::from(
            rust_reference.as_os_str().as_encoded_bytes().to_vec(),
        )]
    } else {
        Vec::new()
    };
    let mut guidance = vec![
        "Stage one independently valid atomic change.",
        "Use one concrete action in the commit message.",
        "Submit each atom through git factor so its gates run.",
    ];
    if ctx.env.var_os("CLAUDECODE").is_some() {
        guidance.push("Above 50% context, pause and ask the user to /compact.");
        guidance.push("Continue splitting until the session is complete.");
    }
    Ok(Selection {
        actions: Actions {
            abort: ["git", "factor", "--abort"],
            submit: ["git", "factor", "--continue", "--message", "<message>"],
        },
        changes: Changes {
            unstaged,
            untracked,
        },
        guidance,
        references,
    })
}

/// Refuses an empty selected change before Git or session state is modified.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps admission result construction inside the serializer module"
    )
)]
pub(in crate::git_factor) fn empty_selection(ctx: &Ctx<'_>) -> Result<(), FactorError> {
    emit(
        ctx,
        &CommandResult::EmptySelection {
            operation: "start",
            reason: "empty_change",
            result: "refused",
        },
    )
}

/// Reports a combined checkpoint selection without reconstructing discarded internal commits.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "checkpoint selection output is constructed at its serializer boundary"
    )
)]
pub(in crate::git_factor) fn started_checkpoint(
    ctx: &Ctx<'_>,
    commit: &CommitSha,
    commit_count: NonZeroUsize,
    operation: SelectionOperation,
    message: &str,
    short_commit: &str,
) -> Result<(), FactorError> {
    let Selection {
        actions,
        changes,
        guidance,
        references,
    } = selection(ctx)?;
    let result = Started {
        actions,
        changes,
        guidance,
        operation,
        references,
        target: RangeTarget {
            commit: commit.as_str(),
            commit_count: commit_count.get(),
            message,
            short_commit,
        },
    };
    emit(ctx, &CommandResult::Started(&result))
}

/// Reports the observed rebase state and applicable abort recovery command.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps abort result construction inside the serializer module"
    )
)]
pub(in crate::git_factor) fn aborted(ctx: &Ctx<'_>, in_progress: bool) -> Result<(), FactorError> {
    emit(
        ctx,
        &CommandResult::Aborted {
            operation: "abort",
            rebase: ObservedRebase { in_progress },
            actions: AbortActions {
                abort_rebase: in_progress.then_some(["git", "rebase", "--abort"]),
            },
        },
    )
}

/// Reports gate failure and keeps subprocess diagnostics on stderr.
pub(in crate::git_factor) fn gate_failed(
    ctx: &Ctx<'_>,
    origin: GateFailureOrigin,
    command: &NonEmptyString,
    exit_code: NonZeroI32,
) -> Result<(), FactorError> {
    let (operation, guidance, submit) = match origin {
        GateFailureOrigin::Baseline(operation) => (
            match operation {
                SelectionOperation::Start => CommandOperation::Start,
                SelectionOperation::Continue => CommandOperation::Continue,
            },
            "Fix the gate environment or amend the current commit without changing its tree, then run git factor --continue. To change the tree, abort the session, repair the checkpoint, and start again.",
            None,
        ),
        GateFailureOrigin::Candidate(operation) => (
            match operation {
                CompletionOperation::Continue => CommandOperation::Continue,
                CompletionOperation::Finish => CommandOperation::Finish,
            },
            "Adjust staged changes so the gate passes, then submit the atom again.",
            Some(["git", "factor", "--continue", "--message", "<message>"]),
        ),
    };
    emit(
        ctx,
        &CommandResult::GateFailed {
            actions: GateFailureActions { submit },
            gate: FailedGate {
                command: command.as_str(),
                exit_code,
            },
            guidance: [guidance],
            operation,
            result: "gate_failed",
        },
    )
}

/// Reports paused replay for its initiating public command.
pub(in crate::git_factor) fn start_recovery(
    ctx: &Ctx<'_>,
    operation: CommandOperation,
) -> Result<(), FactorError> {
    emit(
        ctx,
        &CommandResult::StartRecovery {
            actions: StartRecoveryActions {
                amend: ["git", "commit", "--amend", "--no-edit"],
                continue_factor: ["git", "factor", "--continue"],
                stage: ["git", "add", "<paths>"],
            },
            operation,
            result: "recovery_required",
        },
    )
}

/// Serializes completely before writing the single result to stdout.
#[expect(
    clippy::expect_used,
    clippy::unwrap_in_result,
    reason = "closed result variants contain only JSON-compatible strings, integers, booleans and sequences"
)]
fn emit(ctx: &Ctx<'_>, result: &CommandResult<'_>) -> Result<(), FactorError> {
    let json = serde_json::to_string(result).expect("command results are JSON-compatible");
    ctx.outln(&json)
}

/// Reports the natural checkpoint facts through the existing status serializer.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "checkpoint status facts are constructed at their serializer boundary"
    )
)]
pub(in crate::git_factor) fn checkpoint_status(
    ctx: &Ctx<'_>,
    target: &CommitSha,
    checkpoint: &CommitSha,
    counts: CheckpointCounts,
    phase: CheckpointPhase,
    is_root: bool,
    in_progress: bool,
) -> Result<(), FactorError> {
    emit(
        ctx,
        &CommandResult::Status(&Status {
            operation: "status",
            session: Some(CheckpointStatus {
                phase,
                checkpoint,
                split_count: counts.split_count,
                target: CheckpointTarget {
                    commit: target,
                    commit_count: counts.commit_count,
                    span_starts_at_root: is_root,
                },
                rebase: Rebase {
                    in_progress,
                    required: matches!(
                        phase,
                        CheckpointPhase::Opening
                            | CheckpointPhase::Selecting
                            | CheckpointPhase::Replaying
                            | CheckpointPhase::Verified
                    ),
                },
            }),
        }),
    )
}
/// Reports checkpoint progress without truncating the number of completed rounds.
pub(in crate::git_factor) fn checkpoint_remaining(
    ctx: &Ctx<'_>,
    transition: CheckpointTransition,
) -> Result<(), FactorError> {
    let (operation, outcome, split_count) = match transition {
        CheckpointTransition::Committed(count) => ("continue", "committed", count.get()),
        CheckpointTransition::Retried(count) => ("retry", "attempt_discarded", count),
    };
    let result = Remaining {
        operation,
        result: outcome,
        selection: selection(ctx)?,
        split_count,
    };
    emit(ctx, &CommandResult::Remaining(&result))
}
/// Reports all completed checkpoint atoms using the shared completion envelope.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "checkpoint completion output is constructed at its serializer boundary"
    )
)]
pub(in crate::git_factor) fn checkpoint_completed(
    ctx: &Ctx<'_>,
    operation: CheckpointCompletionOperation,
    split_count: NonZeroU64,
) -> Result<(), FactorError> {
    emit(
        ctx,
        &CommandResult::Completed {
            operation,
            result: "complete",
            split_count,
        },
    )
}

/// Reports that no checkpoint session is active.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "inactive status output is constructed at its serializer boundary"
    )
)]
pub(in crate::git_factor) fn inactive_status(ctx: &Ctx<'_>) -> Result<(), FactorError> {
    emit(
        ctx,
        &CommandResult::Status(&Status {
            operation: "status",
            session: None,
        }),
    )
}

/// Reports terminal status without querying prunable historical objects.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "terminal facts are encoded at their serializer boundary"
    )
)]
pub(in crate::git_factor) fn closing_status(
    ctx: &Ctx<'_>,
    target: &CommitSha,
    checkpoint: &CommitSha,
    outcome: ClosingOutcome,
    is_root: bool,
    in_progress: bool,
) -> Result<(), FactorError> {
    let (result, split_count) = match outcome {
        ClosingOutcome::Complete(count) => ("complete", count.get()),
        ClosingOutcome::Aborted(count) => ("aborted", count),
    };
    emit(
        ctx,
        &CommandResult::ClosingStatus(&Status {
            operation: "status",
            session: Some(ClosingStatus {
                checkpoint,
                outcome: result,
                phase: "closing",
                split_count,
                target: ClosingTarget {
                    commit: target,
                    span_starts_at_root: is_root,
                },
                rebase: Rebase {
                    in_progress,
                    required: false,
                },
            }),
        }),
    )
}
