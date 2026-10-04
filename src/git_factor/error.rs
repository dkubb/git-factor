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

    /// Native compatibility could not be observed before session authority was admitted.
    #[error("git command failed: {0}")]
    PrerequisiteObservation(NonEmptyString),

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

impl FactorError {
    /// Returns true when this error should be persisted to `.git/factor/error.log`.
    pub(in crate::git_factor) const fn should_persist_error_log(&self) -> bool {
        match self {
            &Self::GitCommand(_)
            | &Self::Io(_)
            | &Self::StateRead(_)
            | &Self::StateWrite(_)
            | &Self::TreeHashMismatch { .. } => true,
            &Self::ActiveRebase
            | &Self::ActiveSession
            | &Self::ExecFailed { .. }
            | &Self::GitDir(_)
            | &Self::InvalidCommit(_)
            | &Self::InvalidExecSyntax(_)
            | &Self::MergeCommit(_)
            | &Self::NoActiveSession
            | &Self::NoStagedChanges
            | &Self::NotAncestor(_)
            | &Self::NotGitRepo
            | &Self::PrerequisiteObservation(_)
            | &Self::Usage(_) => false,
        }
    }
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

#[cfg(test)]
mod tests {
    mod factor_error {
        mod should_persist_error_log {
            #[test]
            fn prerequisite_observation_preserves_primary_diagnostic_without_session_logging() {
                let error = super::super::super::FactorError::PrerequisiteObservation(
                    super::super::super::non_empty_msg("owned query failure".to_owned()),
                );
                assert!(!error.should_persist_error_log());
                assert_eq!(error.to_string(), "git command failed: owned query failure");
            }
        }
    }
    use super::*;

    use crate::git_factor::types::COMMIT_SHA_HEX_LEN;
    use crate::test_support::OrAbort as _;

    #[test]
    fn factor_error_flags_unexpected_errors_for_persisted_logs() {
        let sha = CommitSha::new("a".repeat(COMMIT_SHA_HEX_LEN)).or_abort("valid sha");
        let tree_expected = TreeHash::new(&"b".repeat(COMMIT_SHA_HEX_LEN)).or_abort("valid tree");
        let tree_actual = TreeHash::new(&"c".repeat(COMMIT_SHA_HEX_LEN)).or_abort("valid tree");
        let exec = NonEmptyString::try_from("true".to_owned()).or_abort("non-empty");
        let exit_code: i32 = 1;

        assert!(
            FactorError::GitCommand(non_empty_msg("boom".to_owned())).should_persist_error_log()
        );
        assert!(FactorError::Io(io::Error::other("boom")).should_persist_error_log());
        assert!(FactorError::StateRead(io::Error::other("boom")).should_persist_error_log());
        assert!(FactorError::StateWrite(io::Error::other("boom")).should_persist_error_log());
        assert!(
            FactorError::TreeHashMismatch {
                actual: tree_actual,
                expected: tree_expected,
            }
            .should_persist_error_log()
        );

        assert!(
            !FactorError::ExecFailed {
                code: exit_code,
                command: exec
            }
            .should_persist_error_log()
        );
        assert!(!FactorError::NoStagedChanges.should_persist_error_log());
        assert!(!FactorError::Usage(non_empty_msg("usage".to_owned())).should_persist_error_log());
        assert!(!FactorError::MergeCommit(sha).should_persist_error_log());
    }
}

#[cfg(test)]
mod proptests {
    mod factor_error {
        mod should_persist_error_log {
            proptest::proptest! {
                #[test]
                fn preserves_generated_prerequisite_diagnostic_without_logging(diagnostic in ".{1,40}") {
                    let error = super::super::super::FactorError::PrerequisiteObservation(
                        super::super::super::non_empty_msg(diagnostic.clone()),
                    );
                    proptest::prop_assert!(!error.should_persist_error_log());
                    proptest::prop_assert_eq!(error.to_string(), format!("git command failed: {diagnostic}"));
                }
            }
        }
    }
}
