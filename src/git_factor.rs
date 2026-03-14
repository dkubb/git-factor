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

use core::num::NonZeroU8;
use std::io;
use std::path::{Path, PathBuf};

use crate::exit_codes::{EXIT_DATAERR, EXIT_OK, EXIT_SOFTWARE, EXIT_TEMPFAIL, EXIT_USAGE};
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
use self::state::read_state_parsed;
use self::types::StateDir;

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

/// Current commit index in persisted factor state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CurrentIndex(usize);

impl CurrentIndex {
    /// Returns the underlying index value.
    const fn as_usize(self) -> usize {
        self.0
    }

    /// Returns the next index, failing on overflow.
    fn increment(self) -> Result<Self, FactorError> {
        let Some(value) = self.0.checked_add(1) else {
            return Err(FactorError::GitCommand(non_empty_msg(
                "current_index overflow".to_owned(),
            )));
        };
        Ok(Self(value))
    }

    /// Reads `current_index` from persisted state.
    #[cfg_attr(
        test,
        expect(
            clippy::single_call_fn,
            reason = "called in later workflow commits; kept on CurrentIndex for type cohesion"
        )
    )]
    fn read(ctx: &Ctx<'_>, state_dir: &StateDir) -> Result<Self, FactorError> {
        read_state_parsed::<usize>(
            ctx,
            state_dir.as_path(),
            StateFileKey::CurrentIndex.as_str(),
        )
        .map(Self)
    }
}

/// Number of split commits produced for the current commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SplitCount(u8);

impl SplitCount {
    /// Returns the split count as a `NonZeroU8`, or `None` when zero.
    const fn as_non_zero(self) -> Option<NonZeroU8> {
        NonZeroU8::new(self.0)
    }

    /// Returns the underlying split-count value.
    const fn as_u8(self) -> u8 {
        self.0
    }

    /// Returns the incremented split count, failing on overflow.
    fn increment(self) -> Result<Self, FactorError> {
        let Some(value) = self.0.checked_add(1) else {
            return Err(FactorError::GitCommand(non_empty_msg(
                "split_count overflow".to_owned(),
            )));
        };
        Ok(Self(value))
    }

    /// Reads `split_count` from persisted state.
    #[cfg_attr(
        test,
        expect(
            clippy::single_call_fn,
            reason = "called via Session::split_count; kept on SplitCount for type cohesion"
        )
    )]
    fn read(ctx: &Ctx<'_>, state_dir: &StateDir) -> Result<Self, FactorError> {
        read_state_parsed::<u8>(ctx, state_dir.as_path(), StateFileKey::SplitCount.as_str())
            .map(Self)
    }

    /// Returns the initial split count for a new step.
    const fn zero() -> Self {
        Self(0)
    }
}

/// Full commit message (1-65,536 bytes).
#[derive(Debug)]
struct CommitMessage(NonEmptyString);

impl CommitMessage {
    /// Maximum byte length for a commit message.
    const MAX_LEN: usize = 0x0001_0000;

    /// Returns the message as a string slice.
    const fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<String> for CommitMessage {
    type Error = FactorError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() > Self::MAX_LEN {
            return Err(FactorError::GitCommand(non_empty_msg(format!(
                "commit message exceeds {} bytes ({} bytes)",
                Self::MAX_LEN,
                value.len()
            ))));
        }
        if value
            .chars()
            .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
        {
            return Err(FactorError::GitCommand(non_empty_msg(
                "commit message contains control characters".to_owned(),
            )));
        }
        let non_empty = match NonEmptyString::try_from(value) {
            Ok(non_empty) => non_empty,
            Err(_err) => {
                return Err(FactorError::GitCommand(non_empty_msg(
                    "empty commit message".to_owned(),
                )));
            }
        };
        Ok(Self(non_empty))
    }
}

/// Abbreviated commit SHA from `rev-parse --short` (1-40 lowercase hex chars).
#[derive(Debug)]
struct ShortSha(NonEmptyString);

impl ShortSha {
    /// Maximum length of an abbreviated SHA (full SHA-1 hex).
    const MAX_LEN: usize = 40;

    /// Returns the short SHA as a string slice.
    const fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<String> for ShortSha {
    type Error = FactorError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() > Self::MAX_LEN {
            return Err(FactorError::GitCommand(non_empty_msg(format!(
                "short SHA exceeds {} chars ({} chars)",
                Self::MAX_LEN,
                value.len()
            ))));
        }
        if !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(FactorError::GitCommand(non_empty_msg(format!(
                "short SHA contains non-hex characters: {value}"
            ))));
        }
        let non_empty = match NonEmptyString::try_from(value) {
            Ok(non_empty) => non_empty,
            Err(_err) => {
                return Err(FactorError::GitCommand(non_empty_msg(
                    "empty short SHA".to_owned(),
                )));
            }
        };
        Ok(Self(non_empty))
    }
}

/// Maps a `FactorError` to an `(exit_code, message)` tuple.
fn error_to_exit(error: &FactorError) -> (i32, String) {
    let code = match error {
        &FactorError::ActiveRebase
        | &FactorError::ActiveSession
        | &FactorError::NoActiveSession
        | &FactorError::NoStagedChanges
        | &FactorError::Usage(_) => EXIT_USAGE,
        &FactorError::GitDir(_)
        | &FactorError::InvalidCommit(_)
        | &FactorError::InvalidExecSyntax(_)
        | &FactorError::MergeCommit(_)
        | &FactorError::NotAncestor(_)
        | &FactorError::NotGitRepo => EXIT_DATAERR,
        &FactorError::ExecFailed { .. } | &FactorError::TreeHashMismatch { .. } => EXIT_TEMPFAIL,
        &FactorError::GitCommand(_)
        | &FactorError::StateRead(_)
        | &FactorError::StateWrite(_)
        | &FactorError::Io(_) => EXIT_SOFTWARE,
    };
    (code, error.to_string())
}

/// Runs the `git-factor` CLI entrypoint.
#[inline]
#[must_use]
pub const fn main_entry() -> i32 {
    EXIT_OK
}

/// Builds the real runtime context from a cwd lookup result.
fn build_ctx_from_cwd(cwd_result: io::Result<PathBuf>) -> Result<Ctx<'static>, FactorError> {
    match cwd_result {
        Ok(cwd) => Ok(Ctx {
            cwd,
            env: &REAL_ENV,
            fs: &REAL_FS,
            io: &REAL_IO,
            runner: &REAL_RUNNER,
        }),
        Err(err) => Err(FactorError::GitCommand(non_empty_msg(format!(
            "cannot resolve cwd: {err}"
        )))),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CommitMessage, Ctx, CurrentIndex, FactorError, REAL_ENV, REAL_FS, REAL_IO, REAL_RUNNER,
        SessionPhase, ShortSha, SplitCount, StateBool, StateDir, StateFileKey, build_ctx_from_cwd,
        error_to_exit,
    };
    use crate::exit_codes::{EXIT_DATAERR, EXIT_SOFTWARE, EXIT_TEMPFAIL, EXIT_USAGE};
    use crate::git_factor::non_empty_msg;
    use crate::test_support::{OrAbort as _, ResultOrAbort as _};
    use core::num::NonZeroU8;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn ctx_for(path: &Path) -> Ctx<'static> {
        Ctx {
            runner: &REAL_RUNNER,
            cwd: path.to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        }
    }

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

    #[test]
    fn current_index_reads_and_increments() {
        let dir = TempDir::new().or_abort("tempdir");
        let state_dir = dir.path().join("factor");
        fs::create_dir_all(&state_dir).or_abort("create factor dir");
        fs::write(state_dir.join("current_index"), "41\n").or_abort("write current_index");
        let ctx = ctx_for(dir.path());

        let current_index =
            CurrentIndex::read(&ctx, &StateDir::new(state_dir)).or_abort("read current_index");
        assert_eq!(current_index.as_usize(), 41);
        assert_eq!(
            current_index
                .increment()
                .or_abort("increment current_index")
                .as_usize(),
            42
        );
    }

    #[test]
    fn current_index_increment_reports_overflow() {
        let err = CurrentIndex(usize::MAX)
            .increment()
            .err_or_abort("usize::MAX current_index should overflow");
        assert_eq!(
            err.to_string(),
            "git command failed: current_index overflow"
        );
    }

    #[test]
    fn split_count_reads_and_increments() {
        let dir = TempDir::new().or_abort("tempdir");
        let state_dir = dir.path().join("factor");
        fs::create_dir_all(&state_dir).or_abort("create factor dir");
        fs::write(state_dir.join("split_count"), "1\n").or_abort("write split_count");
        let ctx = ctx_for(dir.path());

        let split_count =
            SplitCount::read(&ctx, &StateDir::new(state_dir)).or_abort("read split_count");
        assert_eq!(split_count.as_non_zero(), NonZeroU8::new(1));
        assert_eq!(split_count.as_u8(), 1);
        assert_eq!(
            split_count
                .increment()
                .or_abort("increment split_count")
                .as_u8(),
            2
        );
    }

    #[test]
    fn commit_message_try_from_rejects_invalid_values() {
        let too_long = "a".repeat(CommitMessage::MAX_LEN + 1);
        let too_long_err = CommitMessage::try_from(too_long).err_or_abort("too-long message");
        assert_eq!(
            too_long_err.to_string(),
            format!(
                "git command failed: commit message exceeds {} bytes ({} bytes)",
                CommitMessage::MAX_LEN,
                CommitMessage::MAX_LEN + 1
            )
        );

        let newline_message = CommitMessage::try_from("line 1\nline 2".to_owned())
            .or_abort("newline should be accepted");
        assert_eq!(newline_message.as_str(), "line 1\nline 2");

        let tab_message =
            CommitMessage::try_from("column\tvalue".to_owned()).or_abort("tab should be accepted");
        assert_eq!(tab_message.as_str(), "column\tvalue");

        let control_err =
            CommitMessage::try_from("bad\u{7f}message".to_owned()).err_or_abort("control char");
        assert_eq!(
            control_err.to_string(),
            "git command failed: commit message contains control characters"
        );

        let empty_err = CommitMessage::try_from(String::new()).err_or_abort("empty message");
        assert_eq!(
            empty_err.to_string(),
            "git command failed: empty commit message"
        );
    }

    #[test]
    fn short_sha_try_from_validates_format() {
        let valid_short_sha =
            ShortSha::try_from("abcdef1".to_owned()).or_abort("valid short sha should parse");
        assert_eq!(valid_short_sha.as_str(), "abcdef1");

        let too_long = "a".repeat(ShortSha::MAX_LEN + 1);
        let too_long_err = ShortSha::try_from(too_long).err_or_abort("too-long short sha");
        assert_eq!(
            too_long_err.to_string(),
            format!(
                "git command failed: short SHA exceeds {} chars ({} chars)",
                ShortSha::MAX_LEN,
                ShortSha::MAX_LEN + 1
            )
        );

        let non_hex_err = ShortSha::try_from("xyz".to_owned()).err_or_abort("non-hex short sha");
        assert_eq!(
            non_hex_err.to_string(),
            "git command failed: short SHA contains non-hex characters: xyz"
        );

        let empty_err = ShortSha::try_from(String::new()).err_or_abort("empty short sha");
        assert_eq!(empty_err.to_string(), "git command failed: empty short SHA");
    }

    #[test]
    fn split_count_increment_reports_overflow() {
        let err = SplitCount(u8::MAX)
            .increment()
            .err_or_abort("u8::MAX split_count should overflow");
        assert_eq!(err.to_string(), "git command failed: split_count overflow");
    }

    #[test]
    fn error_to_exit_maps_usage_errors() {
        let (code, message) =
            error_to_exit(&FactorError::Usage(non_empty_msg("bad usage".to_owned())));
        assert_eq!(code, EXIT_USAGE);
        assert_eq!(message, "bad usage");
    }

    #[test]
    fn error_to_exit_maps_data_and_software_errors() {
        let (data_code, data_message) =
            error_to_exit(&FactorError::InvalidCommit("deadbeef".to_owned()));
        assert_eq!(data_code, EXIT_DATAERR);
        assert_eq!(data_message, "invalid commit: deadbeef");

        let (software_code, software_message) = error_to_exit(&FactorError::GitCommand(
            non_empty_msg("internal state blew up".to_owned()),
        ));
        assert_eq!(software_code, EXIT_SOFTWARE);
        assert_eq!(
            software_message,
            "git command failed: internal state blew up"
        );
    }

    #[test]
    fn error_to_exit_maps_tempfail_errors() {
        let exec_code: i32 = 42;
        let (code, message) = error_to_exit(&FactorError::ExecFailed {
            code: exec_code,
            command: non_empty_msg("just ci".to_owned()),
        });
        assert_eq!(code, EXIT_TEMPFAIL);
        assert_eq!(message, "exec gate failed: just ci (exit code 42)");
    }

    #[test]
    fn build_ctx_from_cwd_reports_error() {
        let result = build_ctx_from_cwd(Err(io::Error::other("cwd failed")));
        let err = result
            .err()
            .or_abort("cwd failure should map to git command error");
        assert_eq!(
            err.to_string(),
            "git command failed: cannot resolve cwd: cwd failed"
        );
    }

    #[test]
    fn build_ctx_from_cwd_returns_real_context_on_success() {
        let cwd = PathBuf::from("repo");
        let ctx = build_ctx_from_cwd(Ok(cwd.clone())).or_abort("cwd success should build context");

        assert_eq!(ctx.cwd, cwd);
    }
}
