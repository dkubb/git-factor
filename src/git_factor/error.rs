use std::io;

use thiserror::Error;

use super::types::CommitSha;

/// Errors that can occur during a factor session.
#[derive(Debug, Error)]
pub(super) enum FactorError {
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
        command: non_empty_string::NonEmptyString,
    },

    /// A git command failed.
    #[error("git command failed: {0}")]
    GitCommand(String),

    /// Failed to determine the git directory.
    #[error("failed to determine git directory: {0}")]
    GitDir(String),

    /// The commit reference could not be resolved.
    #[error("invalid commit: {0}")]
    InvalidCommit(String),

    /// The exec command has invalid bash syntax.
    #[error("invalid exec syntax: {0}")]
    InvalidExecSyntax(String),

    /// The commit is a merge commit.
    #[error("commit {0} is a merge commit and cannot be split")]
    MergeCommit(CommitSha),

    /// No factor session is active.
    #[error("no active factor session")]
    NoActiveSession,

    /// No staged changes to commit.
    #[error("no staged changes to commit")]
    NoStagedChanges,

    /// Invalid CLI usage not covered by clap parsing.
    #[error("{0}")]
    Usage(String),

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

    /// Failed to write to stdout or stderr.
    #[error("failed to write output: {0}")]
    Io(#[source] io::Error),

    /// The tree hash after factoring does not match the original.
    #[error("tree hash mismatch: expected {expected}, got {actual}")]
    TreeHashMismatch {
        /// The actual tree hash after factoring.
        actual: String,
        /// The expected tree hash from the original commit.
        expected: String,
    },
}
