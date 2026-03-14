//! `git-factor` is a `git` subcommand for splitting a large commit into smaller,
//! atomic commits using interactive rebase.
//!
//! After installing (for example with `cargo install --path .`), run it as:
//! `git factor ...` (git will resolve this to the `git-factor` binary).

#![forbid(unsafe_code)]
#![expect(
    clippy::implicit_return,
    reason = "conflicts with `clippy::needless_return` from `clippy::all`"
)]
#![expect(
    clippy::question_mark_used,
    reason = "state helpers use `?` for simple error propagation"
)]
#![expect(
    dead_code,
    reason = "support modules land before the full factor engine is wired into them"
)]

/// CLI argument model for `git-factor`.
#[path = "git_factor/cli.rs"]
mod cli;
/// Execution context and filesystem access.
#[path = "git_factor/ctx.rs"]
mod ctx;
/// Error model for `git-factor`.
#[path = "git_factor/error.rs"]
mod error;
/// State file read/write operations for `git-factor`.
#[path = "git_factor/state.rs"]
mod state;
/// Core domain types for `git-factor`.
#[path = "git_factor/types.rs"]
mod types;

use std::io;
use std::path::Path;

use crate::exit_codes::EXIT_OK;
use crate::non_empty_string::NonEmptyString;

#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "re-exports ctx items for sibling module access via `use super::*`"
    )
)]
use self::ctx::*;
use self::error::{FactorError, non_empty_msg};

/// Canonical state-file keys used in `.git/factor`.
#[derive(Clone, Copy)]
enum StateFileKey {
    /// Multi-line list of commit SHAs being split in session order.
    Commits,
    /// Zero-based index into [`Self::Commits`] for the active split target.
    CurrentIndex,
    /// Joined `--exec` command pipeline for split-gate checks.
    Exec,
    /// Expected converged tree hash for the active split step.
    ExpectedTree,
    /// Whether the split target is the repository root commit.
    IsRoot,
    /// Current factor-session phase.
    Phase,
    /// Whether session progression is currently managed by rebase.
    RequiresRebase,
    /// Number of split commits created for the active target commit.
    SplitCount,
    /// `HEAD` commit SHA captured when the session started.
    StartHead,
    /// Whether the rebase flow has started for this session.
    StartedRebase,
}

impl StateFileKey {
    /// Returns the on-disk filename for this state key.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Commits => "commits",
            Self::CurrentIndex => "current_index",
            Self::Exec => "exec",
            Self::ExpectedTree => "expected_tree",
            Self::IsRoot => "is_root",
            Self::Phase => "phase",
            Self::RequiresRebase => "requires_rebase",
            Self::SplitCount => "split_count",
            Self::StartHead => "start_head",
            Self::StartedRebase => "started_rebase",
        }
    }
}

/// Canonical boolean encoding for persisted factor-state values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StateBool {
    /// `false`.
    False,
    /// `true`.
    True,
}

impl StateBool {
    /// Returns the boolean value.
    const fn as_bool(self) -> bool {
        matches!(self, Self::True)
    }

    /// Returns the on-disk representation.
    const fn as_str(self) -> &'static str {
        match self {
            Self::False => "false",
            Self::True => "true",
        }
    }

    /// Converts a bool into a typed state boolean.
    const fn from_bool(value: bool) -> Self {
        if value { Self::True } else { Self::False }
    }
}

/// Persisted session phase for factor-state transitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionPhase {
    /// Rebase is paused at a clean baseline commit that has not been opened.
    PendingStart,
    /// The commit has been opened into an active split session.
    Splitting,
}

impl SessionPhase {
    /// Returns the on-disk representation.
    const fn as_str(self) -> &'static str {
        match self {
            Self::PendingStart => "pending_start",
            Self::Splitting => "splitting",
        }
    }

    /// Parses the persisted phase value.
    fn parse(raw: &str) -> Result<Self, FactorError> {
        match raw {
            "pending_start" => Ok(Self::PendingStart),
            "splitting" => Ok(Self::Splitting),
            _ => Err(FactorError::GitCommand(non_empty_msg(format!(
                "corrupted state file 'phase': invalid value '{raw}'"
            )))),
        }
    }
}

/// Runs the `git-factor` CLI entrypoint.
///
/// The full factor workflow is added in later commits. This placeholder keeps
/// the binary wiring intact while the shared support layers land first.
#[must_use]
#[inline]
pub const fn main_entry() -> i32 {
    EXIT_OK
}

#[cfg(test)]
mod tests {
    use super::{SessionPhase, StateBool, StateFileKey};
    use crate::test_support::{OrAbort as _, ResultOrAbort as _};

    #[test]
    fn state_file_key_as_str_matches_expected_names() {
        assert_eq!(StateFileKey::Commits.as_str(), "commits");
        assert_eq!(StateFileKey::CurrentIndex.as_str(), "current_index");
        assert_eq!(StateFileKey::Exec.as_str(), "exec");
        assert_eq!(StateFileKey::ExpectedTree.as_str(), "expected_tree");
        assert_eq!(StateFileKey::IsRoot.as_str(), "is_root");
        assert_eq!(StateFileKey::Phase.as_str(), "phase");
        assert_eq!(StateFileKey::RequiresRebase.as_str(), "requires_rebase");
        assert_eq!(StateFileKey::SplitCount.as_str(), "split_count");
        assert_eq!(StateFileKey::StartHead.as_str(), "start_head");
        assert_eq!(StateFileKey::StartedRebase.as_str(), "started_rebase");
    }

    #[test]
    fn state_bool_round_trips() {
        assert_eq!(StateBool::False.as_str(), "false");
        assert_eq!(StateBool::True.as_str(), "true");
        assert!(!StateBool::False.as_bool());
        assert!(StateBool::True.as_bool());
        assert_eq!(StateBool::from_bool(false), StateBool::False);
        assert_eq!(StateBool::from_bool(true), StateBool::True);
    }

    #[test]
    fn session_phase_parse_round_trips_and_rejects_unknown_values() {
        assert_eq!(SessionPhase::PendingStart.as_str(), "pending_start");
        assert_eq!(SessionPhase::Splitting.as_str(), "splitting");
        assert_eq!(
            SessionPhase::parse("pending_start").or_abort("pending_start phase"),
            SessionPhase::PendingStart
        );
        assert_eq!(
            SessionPhase::parse("splitting").or_abort("splitting phase"),
            SessionPhase::Splitting
        );

        let err = SessionPhase::parse("bogus").err_or_abort("bogus phase should fail");
        assert_eq!(
            err.to_string(),
            "git command failed: corrupted state file 'phase': invalid value 'bogus'"
        );
    }
}
