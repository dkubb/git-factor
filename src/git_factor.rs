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
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use clap::{CommandFactory as _, Parser as _};

use crate::exit_codes::{EXIT_DATAERR, EXIT_OK, EXIT_SOFTWARE, EXIT_TEMPFAIL, EXIT_USAGE};
use crate::non_empty_string::NonEmptyString;

use self::cli::Cli;
#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "re-exports ctx items for sibling module access via `use super::*`"
    )
)]
use self::ctx::*;
use self::error::{FactorError, non_empty_msg};
use self::state::{read_state, read_state_bool_or_default, read_state_parsed};
use self::types::StateDir;

/// Backend directory name for apply-based rebases.
const REBASE_APPLY_DIR: &str = "rebase-apply";

/// Backend directory name for merge-based rebases.
const REBASE_MERGE_DIR: &str = "rebase-merge";

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
    #[cfg_attr(
        not(test),
        expect(
            clippy::single_call_fn,
            reason = "phase parsing is isolated so later session flows can reuse it"
        )
    )]
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

/// Minimal accessor for the active factor session state.
struct Session<'ctx> {
    /// Execution context for filesystem and git access.
    ctx: &'ctx Ctx<'ctx>,
    /// Path to the `.git/factor` state directory.
    state_dir: PathBuf,
}

impl<'ctx> Session<'ctx> {
    /// Returns the current commit SHA from the persisted commit list.
    fn current_commit(&self) -> Result<String, FactorError> {
        let current_index = self.current_index()?;
        let commits_raw = read_state(self.ctx, &self.state_dir, StateFileKey::Commits.as_str())?;
        let Some(current_commit) = commits_raw.as_str().lines().nth(current_index) else {
            return Err(FactorError::GitCommand(non_empty_msg(format!(
                "commit index {current_index} out of range"
            ))));
        };
        Ok(current_commit.to_owned())
    }

    /// Reads the current commit index from state.
    fn current_index(&self) -> Result<usize, FactorError> {
        read_state_parsed::<usize>(
            self.ctx,
            &self.state_dir,
            StateFileKey::CurrentIndex.as_str(),
        )
    }

    /// Creates a session accessor from the active factor state directory.
    #[expect(
        clippy::single_call_fn,
        reason = "active-session loading is kept isolated for later workflow commands"
    )]
    fn from_active(ctx: &'ctx Ctx<'ctx>) -> Result<Self, FactorError> {
        Ok(Self {
            ctx,
            state_dir: factor_dir_in(ctx)?,
        })
    }

    /// Reads the `is_root` flag from state.
    fn is_root(&self) -> Result<bool, FactorError> {
        read_state_bool_or_default(
            self.ctx,
            &self.state_dir,
            StateFileKey::IsRoot.as_str(),
            false,
        )
    }

    /// Reads the persisted session phase.
    fn phase(&self) -> Result<SessionPhase, FactorError> {
        match read_state(self.ctx, &self.state_dir, StateFileKey::Phase.as_str()) {
            Ok(phase) => SessionPhase::parse(phase.as_str()),
            Err(FactorError::StateRead(err)) if err.kind() == io::ErrorKind::NotFound => {
                Ok(SessionPhase::Splitting)
            }
            Err(err) => Err(err),
        }
    }

    /// Reads the `requires_rebase` flag from state.
    fn requires_rebase(&self) -> Result<bool, FactorError> {
        read_state_bool_or_default(
            self.ctx,
            &self.state_dir,
            StateFileKey::RequiresRebase.as_str(),
            false,
        )
    }

    /// Reads the number of split commits created for the current target.
    fn split_count(&self) -> Result<u8, FactorError> {
        read_state_parsed::<u8>(self.ctx, &self.state_dir, StateFileKey::SplitCount.as_str())
    }
}

/// Returns the path to the factor state directory.
fn factor_dir_in(ctx: &Ctx<'_>) -> Result<PathBuf, FactorError> {
    Ok(git_dir_in(ctx)?.join("factor"))
}

/// Returns the absolute path to the git directory.
fn git_dir_in(ctx: &Ctx<'_>) -> Result<PathBuf, FactorError> {
    let output = ctx
        .runner
        .output("git", &["rev-parse", "--git-dir"], &ctx.cwd)
        .map_err(|error| FactorError::GitDir(non_empty_msg(error.to_string())))?;

    if !output.status.success() {
        return Err(FactorError::NotGitRepo);
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let git_dir_path = PathBuf::from(path);
    if git_dir_path.is_relative() {
        return Ok(ctx.cwd.join(git_dir_path));
    }
    Ok(git_dir_path)
}

/// Returns true when the factor state directory exists.
#[expect(
    clippy::single_call_fn,
    reason = "active-session detection stays isolated for status and later workflow commands"
)]
fn is_factor_active_in(ctx: &Ctx<'_>) -> bool {
    factor_dir_in(ctx).is_ok_and(|dir| ctx.fs.is_dir(&dir))
}

/// Returns true when either rebase backend directory exists.
#[expect(
    clippy::single_call_fn,
    reason = "rebase detection stays isolated for status and later workflow commands"
)]
fn is_mid_rebase_in(ctx: &Ctx<'_>) -> bool {
    git_dir_in(ctx).is_ok_and(|dir| {
        ctx.fs.is_dir(&dir.join(REBASE_MERGE_DIR)) || ctx.fs.is_dir(&dir.join(REBASE_APPLY_DIR))
    })
}

/// Shows status for the current factor session.
#[expect(
    clippy::single_call_fn,
    reason = "status output stays isolated until later workflow commands land"
)]
fn cmd_status_in(ctx: &Ctx<'_>) -> Result<i32, FactorError> {
    if !is_factor_active_in(ctx) {
        ctx.outln("FACTOR: No active session.")?;
        return Ok(EXIT_OK);
    }

    let session = Session::from_active(ctx)?;
    let current_commit = session.current_commit()?;
    let current_index = session.current_index()?;
    let split_count = session.split_count()?;
    let phase = session.phase()?;
    let requires_rebase = session.requires_rebase()?;
    let is_root = session.is_root()?;
    let rebase_in_progress = is_mid_rebase_in(ctx);

    ctx.outln("FACTOR: Active session.")?;
    ctx.outln(&format!("CURRENT_COMMIT: {current_commit}"))?;
    ctx.outln(&format!("CURRENT_INDEX: {current_index}"))?;
    ctx.outln(&format!("SPLIT_COUNT: {split_count}"))?;
    ctx.outln(&format!("PHASE: {}", phase.as_str()))?;
    ctx.outln(&format!("REQUIRES_REBASE: {requires_rebase}"))?;
    ctx.outln(&format!("REBASE_IN_PROGRESS: {rebase_in_progress}"))?;
    ctx.outln(&format!("IS_ROOT: {is_root}"))?;

    Ok(EXIT_OK)
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
pub fn main_entry() -> i32 {
    use std::env;

    let args = env::args_os().collect::<Vec<OsString>>();
    main_entry_with_vec(&REAL_IO, build_ctx_from_cwd(REAL_ENV.current_dir()), args)
}

/// Builds the real runtime context from a cwd lookup result.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "separates context construction from top-level CLI entrypoint"
    )
)]
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

/// Runs the CLI entrypoint with a provided `Ctx` build result.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "entrypoint adapter isolates context-resolution error handling"
    )
)]
fn main_entry_with_vec(
    io: &dyn Io,
    ctx_result: Result<Ctx<'_>, FactorError>,
    args: Vec<OsString>,
) -> i32 {
    match ctx_result {
        Ok(ctx) => run_and_report_with_args_vec(&ctx, args),
        Err(err) => {
            let (code, message) = error_to_exit(&err);
            drop(io.errln(&message));
            code
        }
    }
}

/// Runs `git-factor` with the given arguments and prints any errors to stderr.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "single error-reporting shim avoids duplicating exit mapping logic"
    )
)]
fn run_and_report_with_args_vec(ctx: &Ctx<'_>, args: Vec<OsString>) -> i32 {
    match run_with_args_vec(ctx, args) {
        Ok(code) => code,
        Err(err) => {
            let (code, message) = error_to_exit(&err);
            drop(ctx.errln(&message));
            code
        }
    }
}

/// Parses CLI args and handles the initial help and validation paths.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "central parse/dispatch function keeps CLI behavior consistent"
    )
)]
fn run_with_args_vec(ctx: &Ctx<'_>, args: Vec<OsString>) -> Result<i32, FactorError> {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => {
            let code = if err.use_stderr() {
                EXIT_USAGE
            } else {
                EXIT_OK
            };
            let message = err.to_string();
            if err.use_stderr() {
                ctx.err(&message)?;
            } else {
                ctx.out(&message)?;
            }
            return Ok(code);
        }
    };
    let has_start_args = || !cli.exec().is_empty() || !cli.commits().is_empty();

    if !cli.abort()
        && !cli.status()
        && !cli.continue_flag()
        && !cli.finish()
        && !has_start_args()
        && cli.message().is_empty()
    {
        let help = Cli::command().render_long_help().to_string();
        ctx.out(&help)?;
        return Ok(EXIT_OK);
    }

    if cli.abort()
        && (cli.status()
            || cli.continue_flag()
            || cli.finish()
            || has_start_args()
            || !cli.message().is_empty())
    {
        return Err(FactorError::Usage(non_empty_msg(
            "--abort cannot be combined with other options".to_owned(),
        )));
    }

    if cli.status()
        && (cli.continue_flag() || cli.finish() || has_start_args() || !cli.message().is_empty())
    {
        return Err(FactorError::Usage(non_empty_msg(
            "--status cannot be combined with other options".to_owned(),
        )));
    }

    if cli.finish() && (cli.continue_flag() || has_start_args()) {
        return Err(FactorError::Usage(non_empty_msg(
            "--finish cannot be combined with --continue, --exec, or COMMIT".to_owned(),
        )));
    }

    if cli.continue_flag() {
        if has_start_args() {
            return Err(FactorError::Usage(non_empty_msg(
                "--continue cannot be combined with --exec or COMMIT".to_owned(),
            )));
        }
        if cli.message().is_empty() {
            return Err(FactorError::Usage(non_empty_msg(
                "--continue requires --message <MSG>".to_owned(),
            )));
        }
        return Err(FactorError::Usage(non_empty_msg(
            "continue workflow lands in later commits".to_owned(),
        )));
    }

    if !cli.message().is_empty() {
        return Err(FactorError::Usage(non_empty_msg(
            "--message can only be used with --continue or --finish".to_owned(),
        )));
    }

    if cli.abort() {
        return Err(FactorError::Usage(non_empty_msg(
            "abort workflow lands in later commits".to_owned(),
        )));
    }

    if cli.status() {
        return cmd_status_in(ctx);
    }

    if cli.finish() {
        return Err(FactorError::Usage(non_empty_msg(
            "finish workflow lands in later commits".to_owned(),
        )));
    }

    if cli.exec().is_empty() {
        return Err(FactorError::Usage(non_empty_msg(
            "--exec <COMMAND> is required when starting a factor session".to_owned(),
        )));
    }

    Err(FactorError::Usage(non_empty_msg(
        "start workflow lands in later commits".to_owned(),
    )))
}

#[cfg(test)]
mod tests {
    use super::{
        CommitMessage, Ctx, CurrentIndex, FactorError, Io, REAL_ENV, REAL_FS, REAL_RUNNER,
        SessionPhase, ShortSha, SplitCount, StateBool, StateDir, StateFileKey, build_ctx_from_cwd,
        main_entry_with_vec, run_and_report_with_args_vec, run_with_args_vec,
    };
    use crate::exit_codes::{EXIT_OK, EXIT_USAGE};
    use crate::git_factor::non_empty_msg;
    use crate::test_support::{OrAbort as _, ResultOrAbort as _};
    use core::cell::RefCell;
    use core::num::NonZeroU8;
    use std::ffi::OsString;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    #[derive(Default)]
    struct BufferIo {
        err: RefCell<String>,
        out: RefCell<String>,
    }

    impl Io for BufferIo {
        fn err(&self, text: &str) -> io::Result<()> {
            self.err.borrow_mut().push_str(text);
            Ok(())
        }

        fn errln(&self, line: &str) -> io::Result<()> {
            let mut err = self.err.borrow_mut();
            err.push_str(line);
            err.push('\n');
            Ok(())
        }

        fn out(&self, text: &str) -> io::Result<()> {
            self.out.borrow_mut().push_str(text);
            Ok(())
        }

        fn outln(&self, line: &str) -> io::Result<()> {
            let mut out = self.out.borrow_mut();
            out.push_str(line);
            out.push('\n');
            Ok(())
        }
    }

    fn ctx_for(path: &Path, io: &'static dyn Io) -> Ctx<'static> {
        Ctx {
            runner: &REAL_RUNNER,
            cwd: path.to_path_buf(),
            io,
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
        let io = Box::leak(Box::new(BufferIo::default()));
        let ctx = ctx_for(dir.path(), io);

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
        let io = Box::leak(Box::new(BufferIo::default()));
        let ctx = ctx_for(dir.path(), io);

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
    fn run_with_args_vec_reports_parse_errors() {
        let io = Box::leak(Box::new(BufferIo::default()));
        let ctx = ctx_for(Path::new("."), io);
        let args = vec![OsString::from("git-factor"), OsString::from("--bogus-flag")];

        let code = run_with_args_vec(&ctx, args).or_abort("parse errors map to exit code");
        assert_eq!(code, EXIT_USAGE);
        assert!(!io.err.borrow().is_empty());
    }

    #[test]
    fn run_with_args_vec_no_options_prints_help() {
        let io = Box::leak(Box::new(BufferIo::default()));
        let ctx = ctx_for(Path::new("."), io);
        let args = vec![OsString::from("git-factor")];

        let code = run_with_args_vec(&ctx, args).or_abort("no-options path should succeed");
        assert_eq!(code, EXIT_OK);
        assert!(io.out.borrow().contains("Usage:"));
    }

    #[test]
    fn run_with_args_vec_continue_requires_message() {
        let io = Box::leak(Box::new(BufferIo::default()));
        let ctx = ctx_for(Path::new("."), io);
        let args = vec![OsString::from("git-factor"), OsString::from("--continue")];

        let err =
            run_with_args_vec(&ctx, args).err_or_abort("continue without message should fail");
        assert_eq!(err.to_string(), "--continue requires --message <MSG>");
    }

    #[test]
    fn run_and_report_with_args_vec_converts_usage_errors() {
        let io = Box::leak(Box::new(BufferIo::default()));
        let ctx = ctx_for(Path::new("."), io);
        let args = vec![
            OsString::from("git-factor"),
            OsString::from("--abort"),
            OsString::from("--status"),
        ];

        let code = run_and_report_with_args_vec(&ctx, args);
        assert_eq!(code, EXIT_USAGE);
        assert!(!io.err.borrow().is_empty());
    }

    #[test]
    fn main_entry_with_vec_reports_ctx_errors() {
        let io = Box::leak(Box::new(BufferIo::default()));
        let code = main_entry_with_vec(
            io,
            Err(FactorError::Usage(non_empty_msg(
                "ctx setup failed".to_owned(),
            ))),
            vec![OsString::from("git-factor")],
        );
        assert_eq!(code, EXIT_USAGE);
        assert!(!io.err.borrow().is_empty());
    }

    #[test]
    fn main_entry_with_vec_passes_through_success_path() {
        let io = Box::leak(Box::new(BufferIo::default()));
        let ctx = ctx_for(Path::new("."), io);
        let code = main_entry_with_vec(io, Ok(ctx), vec![OsString::from("git-factor")]);

        assert_eq!(code, EXIT_OK);
        assert!(io.out.borrow().contains("Usage:"));
    }

    #[test]
    fn main_entry_with_vec_uses_default_program_name_when_args_are_empty() {
        let io = Box::leak(Box::new(BufferIo::default()));
        let ctx = ctx_for(Path::new("."), io);
        let code = main_entry_with_vec(io, Ok(ctx), Vec::<OsString>::new());

        assert_eq!(code, EXIT_OK);
        assert!(io.out.borrow().contains("Usage:"));
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
