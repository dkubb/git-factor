use std::io;

use thiserror::Error;

use super::types::{CommitSha, TreeHash};
use crate::non_empty_string::NonEmptyString;

/// Errors that can occur during a factor session.
#[derive(Debug, Error)]
pub(in crate::git_factor) enum FactorError {
    /// A rebase is already in progress.
    #[error("a rebase is already in progress")]
    ActiveRebase,

    /// A factor session is already active.
    #[error("a factor session is already active (use --abort to cancel)")]
    ActiveSession,

    /// The exec gate command failed.
    #[error("exec gate failed: {command} (exit code {code})")]
    ExecFailed {
        /// The exit code of the command.
        code: i32,
        /// The command that failed.
        command: NonEmptyString,
    },

    /// A git command failed.
    #[error("git command failed: {0}")]
    GitCommand(NonEmptyString),

    /// Failed to determine the git directory.
    #[error("failed to determine git directory: {0}")]
    GitDir(NonEmptyString),

    /// The commit reference could not be resolved.
    #[error("invalid commit: {0}")]
    InvalidCommit(String),

    /// The exec command has invalid bash syntax.
    #[error("invalid exec syntax: {0}")]
    InvalidExecSyntax(String),

    /// Failed to write to stdout or stderr.
    #[error("failed to write output: {0}")]
    Io(#[source] io::Error),

    /// The commit is a merge commit.
    #[error("commit {0} is a merge commit and cannot be split")]
    MergeCommit(CommitSha),

    /// No factor session is active.
    #[error("no active factor session")]
    NoActiveSession,

    /// No staged changes to commit.
    #[error(
        "no staged changes to commit\nNEXT: stage exactly one atomic change, then rerun:\n  git factor --continue --message \"type: description\""
    )]
    NoStagedChanges,

    /// The commit is not an ancestor of HEAD.
    #[error("commit {0} is not an ancestor of HEAD")]
    NotAncestor(CommitSha),

    /// Not inside a git repository.
    #[error("not a git repository")]
    NotGitRepo,

    /// Failed to read state file.
    #[error("failed to read state: {0}")]
    StateRead(#[source] io::Error),

    /// Failed to write state file.
    #[error("failed to write state: {0}")]
    StateWrite(#[source] io::Error),

    /// The tree hash after factoring does not match the original.
    #[error("tree hash mismatch: expected {expected}, got {actual}")]
    TreeHashMismatch {
        /// The actual tree hash after factoring.
        actual: TreeHash,
        /// The expected tree hash from the original commit.
        expected: TreeHash,
    },

    /// Invalid CLI usage not covered by clap parsing.
    #[error("{0}")]
    Usage(NonEmptyString),
}

/// Converts a `String` to `NonEmptyString` for error messages that are
/// trivially non-empty by construction (e.g., `format!` with a literal prefix).
#[expect(
    clippy::expect_used,
    reason = "error messages from format! are always non-empty"
)]
pub(in crate::git_factor) fn non_empty_msg(msg: String) -> NonEmptyString {
    NonEmptyString::try_from(msg).expect("error message was unexpectedly empty")
}
