//! Normalized command results, serialized completely before stdout emission.

#[cfg(test)]
#[path = "output_proptests.rs"]
mod proptests;
#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;

use core::num::NonZeroU8;

use serde::Serialize;

use super::{CommitSha, Ctx, CurrentIndex, FactorError, SessionPhase, SplitCount, StateBool};

/// Existing commands that complete a selected commit's split.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::git_factor) enum CompletionOperation {
    /// Submission consumed the remaining change.
    Continue,
    /// The explicit finish command retained the remaining change.
    Finish,
}

/// A completed split has a proved positive commit count.
#[derive(Serialize)]
struct Completed {
    /// Command producing the result.
    operation: CompletionOperation,
    /// Positive count admitted by the existing completion boundary.
    split_count: NonZeroU8,
}

/// Rebase requirements and the native observation.
#[derive(Clone, Copy, Debug, Serialize)]
struct Rebase {
    /// Whether Git currently has an active rebase.
    in_progress: bool,
    /// Whether the session requires a rebase.
    required: bool,
}

/// The existing session's current selected commit and original range boundary.
#[derive(Clone, Copy, Debug, Serialize)]
struct Target<'session> {
    /// Current selected commit identity.
    commit: &'session str,
    /// `CurrentIndex`: the zero-based position in the saved selected commit sequence.
    index: usize,
    /// Whether the selected span starts at the repository root.
    span_starts_at_root: bool,
}

/// Admitted session observations exposed by the status command.
#[derive(Clone, Copy, Debug, Serialize)]
pub(in crate::git_factor) struct SessionStatus<'session> {
    /// Existing saved phase, including `pending_start` and `splitting`.
    phase: &'static str,
    /// Native rebase requirements and observation.
    rebase: Rebase,
    /// Existing per-commit split count.
    split_count: u8,
    /// Current selected identity and position.
    target: Target<'session>,
}

impl<'session> SessionStatus<'session> {
    /// Projects the existing checked session facts into their JSON representation.
    #[cfg_attr(
        not(test),
        expect(
            clippy::single_call_fn,
            reason = "checked status observations are projected at their first serializer consumer"
        )
    )]
    #[inline]
    #[must_use]
    pub(in crate::git_factor) const fn new(
        commit: &'session CommitSha,
        index: CurrentIndex,
        split_count: SplitCount,
        phase: SessionPhase,
        requires_rebase: StateBool,
        is_root: StateBool,
        in_progress: bool,
    ) -> Self {
        Self {
            phase: phase.as_str(),
            rebase: Rebase {
                in_progress,
                required: requires_rebase.as_bool(),
            },
            split_count: split_count.as_u8(),
            target: Target {
                commit: commit.as_str(),
                index: index.as_usize(),
                span_starts_at_root: is_root.as_bool(),
            },
        }
    }
}

/// One status result; null denotes the absence of a session.
#[derive(Serialize)]
struct Status<'session> {
    /// Identifies the public command.
    operation: &'static str,
    /// Active session facts, or absence.
    session: Option<SessionStatus<'session>>,
}

/// Native rebase state observed after factor cleanup.
#[derive(Serialize)]
struct ObservedRebase {
    /// Whether another rebase is still active.
    in_progress: bool,
}

/// The action applicable when a rebase remains active.
#[derive(Serialize)]
struct AbortActions {
    /// Native arguments for aborting the remaining rebase.
    #[serde(skip_serializing_if = "Option::is_none")]
    abort_rebase: Option<[&'static str; 3]>,
}

/// Result of removing the current factor session.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "field declaration order preserves the exact public abort JSON stream contract"
)]
#[derive(Serialize)]
struct Aborted {
    /// Command producing the result.
    operation: &'static str,
    /// Native observation after cleanup.
    rebase: ObservedRebase,
    /// Applicable recovery action.
    actions: AbortActions,
}

/// Writes the observed abort result and applicable native action.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "abort JSON is emitted through the owning command boundary"
    )
)]
#[expect(
    clippy::expect_used,
    clippy::unwrap_in_result,
    reason = "the closed abort representation contains only JSON-compatible strings and booleans"
)]
pub(in crate::git_factor) fn aborted(ctx: &Ctx<'_>, in_progress: bool) -> Result<(), FactorError> {
    let result = Aborted {
        operation: "abort",
        rebase: ObservedRebase { in_progress },
        actions: AbortActions {
            abort_rebase: in_progress.then_some(["git", "rebase", "--abort"]),
        },
    };
    let json = serde_json::to_string(&result).expect("abort facts are JSON-compatible");
    ctx.outln(&json)
}

/// Writes a complete result using the existing positive split count.
#[expect(
    clippy::expect_used,
    clippy::unwrap_in_result,
    reason = "the closed completion representation contains only JSON-compatible strings and a positive integer"
)]
pub(in crate::git_factor) fn completed(
    ctx: &Ctx<'_>,
    operation: CompletionOperation,
    split_count: NonZeroU8,
) -> Result<(), FactorError> {
    let result = Completed {
        operation,
        split_count,
    };
    let json = serde_json::to_string(&result).expect("completion facts are JSON-compatible");
    ctx.outln(&json)
}

/// Writes one complete normalized status result, followed by a newline.
#[expect(
    clippy::expect_used,
    clippy::unwrap_in_result,
    reason = "the closed status representation contains only JSON-compatible strings, integers, booleans and null"
)]
pub(in crate::git_factor) fn status(
    ctx: &Ctx<'_>,
    session: Option<SessionStatus<'_>>,
) -> Result<(), FactorError> {
    let result = Status {
        operation: "status",
        session,
    };
    let json = serde_json::to_string(&result).expect("status facts are JSON-compatible");
    ctx.outln(&json)
}
