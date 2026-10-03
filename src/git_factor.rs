//! `git-factor` is a `git` subcommand for splitting a large commit into smaller,
//! atomic commits using interactive rebase.
//!
//! After installing (for example with `cargo install --path .`), run it as:
//! `git factor ...` (git will resolve this to the `git-factor` binary).

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    expect(
        clippy::self_named_module_files,
        reason = "test-target clippy enables this module-file style lint for the crate entry module"
    )
)]
#![expect(
    clippy::question_mark_used,
    reason = "fallible orchestration code is intentionally expressed with `?` for readability"
)]
#![expect(
    clippy::implicit_return,
    reason = "orchestration and helpers use expression tails for readability"
)]
/// CLI argument model for `git-factor`.
#[path = "git_factor/cli.rs"]
mod cli;
/// Execution context, IO traits, and real implementations.
#[path = "git_factor/ctx.rs"]
mod ctx;
/// Error model for `git-factor`.
#[path = "git_factor/error.rs"]
mod error;
/// Git command execution and process management.
#[path = "git_factor/git.rs"]
mod git;
/// Shared utility helpers for `git-factor`.
#[path = "git_factor/helpers.rs"]
mod helpers;
/// Normalized JSON result serialization.
#[path = "git_factor/output.rs"]
mod output;
/// State file read/write operations for `git-factor`.
#[path = "git_factor/state.rs"]
mod state;
/// Trace logging infrastructure for process and note events.
#[path = "git_factor/trace.rs"]
mod trace;
/// Core domain types for `git-factor`.
#[path = "git_factor/types.rs"]
mod types;
/// User-facing output and status queries.
#[path = "git_factor/ui.rs"]
mod ui;
/// Validation, resolution, and sorting of commits.
#[path = "git_factor/validation.rs"]
mod validation;

use core::num::NonZeroU8;
use std::ffi::OsString;
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
#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "re-exports trace items for sibling module access via `use super::*`"
    )
)]
use self::trace::*;
use clap::{CommandFactory as _, Parser as _};
use nonempty::NonEmpty;

use self::cli::Cli;
use self::error::{FactorError, non_empty_msg};
use self::git::{
    REBASE_APPLY_DIR, REBASE_MERGE_DIR, command_output_with, command_status_with, git_dir_in,
    git_output, git_raw_output, git_status, run_git, run_git_non_interactive,
};
use self::helpers::{editor_path, error_to_exit, factor_dir_in, shell_quote, status_code};
use self::state::{read_state, read_state_bool_or_default, read_state_parsed, write_state};
#[cfg(test)]
use self::types::COMMIT_SHA_HEX_LEN;
use self::types::{BaseParent, CommitSha, CommitSpan, StateDir, TreeHash};
use self::ui::{
    is_factor_active_in, is_mid_rebase_in, print_hints_with_remaining_in, print_session_started,
};
use self::validation::{
    remove_empty_root_in, resolve_commit, resolve_commit_span, validate_exec_syntax,
};
#[cfg(test)]
use self::validation::{resolve_commit_refs, resolve_head_commit};

#[cfg(test)]
use crate::test_support::{OrAbort as _, ResultOrAbort as _};

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
    #[expect(
        clippy::single_call_fn,
        reason = "phase parsing is intentionally centralized for state-file validation"
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

    /// Reads `current_index` from persisted state.
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
    #[expect(
        clippy::single_call_fn,
        reason = "called via Session::split_count; kept on SplitCount for type cohesion"
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

/// Result of completing a fully-split span.
#[derive(Debug)]
enum AdvanceOutcome {
    /// Session is complete — no more commits to split.
    Completed {
        /// Number of split commits produced for the final commit.
        final_split_count: NonZeroU8,
    },
}

/// Result of starting the rebase wrapper for multi-commit factor sessions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StartRebaseOutcome {
    /// Rebase paused at the inserted `break` and the split session can open now.
    PausedAtBreak,
    /// Rebase is paused for user recovery before the split session can begin.
    WaitingForRecovery,
}

/// Factor session state accessor.
///
/// Eagerly loads the immutable commit list on construction; all other reads
/// are lazy. Write operations (increment, advance, cleanup) go through this
/// struct so callers never touch `state_dir` directly.
struct Session<'ctx> {
    /// Parsed commit SHAs for the session (immutable after start).
    commits: Vec<CommitSha>,
    /// Execution context for IO and git operations.
    ctx: &'ctx Ctx<'ctx>,
    /// Path to the `.git/factor` state directory.
    state_dir: StateDir,
}

/// Bootstrap inputs needed to create factor state during the first rebase begin.
struct RebaseExecBeginBootstrap {
    /// Ordered commits selected for this factor session.
    commits: NonEmpty<CommitSha>,
    /// Joined start/split gate command.
    exec_command: NonEmptyString,
    /// Whether the session span begins at the repository root.
    is_root: StateBool,
    /// Original `HEAD` captured before the session started.
    start_head: CommitSha,
}

impl<'ctx> Session<'ctx> {
    /// Completes the active factor span and advances rebase to the next non-factor step.
    fn advance_to_next_commit(&self) -> Result<AdvanceOutcome, FactorError> {
        let split_count = self.split_count()?;
        let requires_rebase = self.requires_rebase(StateBool::True)?;

        if requires_rebase.as_bool() {
            if split_count.as_non_zero().is_none() {
                return Err(FactorError::GitCommand(non_empty_msg(
                    "split_count is zero at advance".to_owned(),
                )));
            }
            if !is_mid_rebase_in(self.ctx) {
                return Err(FactorError::GitCommand(non_empty_msg(
                    "no rebase in progress".to_owned(),
                )));
            }

            if let Err(err) = run_git_non_interactive(self.ctx, &["rebase", "--continue"]) {
                let recovery = "Resolve the rebase issue, then rerun 'git rebase --continue'.";
                return Err(FactorError::GitCommand(non_empty_msg(format!(
                    "{err}\n\n{recovery}\nTo abandon the factor session, run 'git factor --abort'"
                ))));
            }

            if is_mid_rebase_in(self.ctx) {
                return Err(FactorError::GitCommand(non_empty_msg(
                    "rebase remained active after span completion".to_owned(),
                )));
            }
        }

        // Session finished (either rebase is done, or single-commit no-rebase mode).
        let is_root = read_state_key(self.ctx, &self.state_dir, StateFileKey::IsRoot)
            .is_ok_and(|value| value.as_str() == "true");
        self.remove_state_strict()?;

        if is_root {
            remove_empty_root_in(self.ctx)?;
        }

        let final_split_count = match split_count.as_non_zero() {
            Some(final_split_count) => final_split_count,
            None => {
                return Err(FactorError::GitCommand(non_empty_msg(
                    "split_count is zero at completion".to_owned(),
                )));
            }
        };

        Ok(AdvanceOutcome::Completed { final_split_count })
    }

    /// Returns the current commit SHA by indexing the cached commit list.
    fn current_commit(&self) -> Result<CommitSha, FactorError> {
        let current_index = CurrentIndex::read(self.ctx, &self.state_dir)?;
        let out_of_range = FactorError::GitCommand(non_empty_msg(format!(
            "commit index {} out of range (have {} commits)",
            current_index.as_usize(),
            self.commits.len()
        )));
        self.commits
            .get(current_index.as_usize())
            .cloned()
            .ok_or(out_of_range)
    }

    /// Reads the current commit index from persisted state.
    fn current_index(&self) -> Result<CurrentIndex, FactorError> {
        CurrentIndex::read(self.ctx, &self.state_dir)
    }

    /// Returns whether the selected span begins at the root commit.
    fn current_target_is_root(&self) -> Result<bool, FactorError> {
        Ok(self.is_root(StateBool::False)?.as_bool())
    }

    /// Reads the exec command from persisted state.
    fn exec(&self) -> Result<NonEmptyString, FactorError> {
        read_state_key(self.ctx, &self.state_dir, StateFileKey::Exec)
    }

    /// Returns the expected converged tree for the current split step.
    fn expected_tree(&self) -> Result<TreeHash, FactorError> {
        let original_commit = self.current_commit()?;
        expected_tree_for_current_step(self.ctx, &self.state_dir, &original_commit)
    }

    /// Returns the first commit in the selected factor span.
    fn first_commit(&self) -> Result<CommitSha, FactorError> {
        self.commits.first().cloned().ok_or_else(|| {
            FactorError::GitCommand(non_empty_msg("commit list is empty".to_owned()))
        })
    }

    /// Creates a session from the active factor state directory.
    fn from_active(ctx: &'ctx Ctx<'ctx>) -> Result<Self, FactorError> {
        let state_dir = factor_dir_in(ctx)?;
        let commits_raw = read_state_key(ctx, &state_dir, StateFileKey::Commits)?;
        let commits = commits_raw
            .as_str()
            .lines()
            .map(|line| CommitSha::new(line.to_owned()))
            .collect::<Result<Vec<CommitSha>, _>>()?;
        Ok(Self {
            commits,
            ctx,
            state_dir,
        })
    }

    /// Increments `split_count` in persisted state and returns the new value.
    fn increment_split_count(&self) -> Result<u8, FactorError> {
        let split_count = self.split_count()?.increment()?;
        let split_count_text = split_count.as_u8().to_string();
        write_state(
            self.ctx,
            self.state_dir.as_path(),
            StateFileKey::SplitCount.as_str(),
            split_count_text.as_str(),
        )?;
        Ok(split_count.as_u8())
    }

    /// Reads the `is_root` flag with the given default.
    fn is_root(&self, default: StateBool) -> Result<StateBool, FactorError> {
        read_state_bool_key_or_default(self.ctx, &self.state_dir, StateFileKey::IsRoot, default)
    }

    /// Reads the persisted session phase, defaulting for legacy sessions.
    fn phase(&self, default: SessionPhase) -> Result<SessionPhase, FactorError> {
        match read_state_key(self.ctx, &self.state_dir, StateFileKey::Phase) {
            Ok(value) => SessionPhase::parse(value.as_str()),
            Err(FactorError::StateRead(err)) if err.kind() == io::ErrorKind::NotFound => {
                Ok(default)
            }
            Err(err) => Err(err),
        }
    }

    /// Removes the factor state path and fails if cleanup does not succeed.
    fn remove_state_strict(&self) -> Result<(), FactorError> {
        remove_state_path_required(self.ctx, &self.state_dir)
    }

    /// Reads the `requires_rebase` flag with the given default.
    fn requires_rebase(&self, default: StateBool) -> Result<StateBool, FactorError> {
        read_state_bool_key_or_default(
            self.ctx,
            &self.state_dir,
            StateFileKey::RequiresRebase,
            default,
        )
    }

    /// Reads the split count from persisted state.
    fn split_count(&self) -> Result<SplitCount, FactorError> {
        SplitCount::read(self.ctx, &self.state_dir)
    }

    /// Reads the `start_head` state key.
    fn start_head(&self) -> Result<NonEmptyString, FactorError> {
        read_state_key(self.ctx, &self.state_dir, StateFileKey::StartHead)
    }

    /// Reads the `started_rebase` flag with the given default.
    fn started_rebase(&self, default: StateBool) -> Result<StateBool, FactorError> {
        read_state_bool_key_or_default(
            self.ctx,
            &self.state_dir,
            StateFileKey::StartedRebase,
            default,
        )
    }

    /// Creates a session with an explicit state directory and commit list.
    #[cfg(test)]
    const fn with_state(
        ctx: &'ctx Ctx<'ctx>,
        state_dir: StateDir,
        commits: Vec<CommitSha>,
    ) -> Self {
        Self {
            commits,
            ctx,
            state_dir,
        }
    }
}

/// Allowed repository states at gate boundaries.
#[derive(Clone, Copy)]
enum RepoStatePolicy {
    /// No staged, unstaged, or untracked changes are allowed.
    FullyClean,
    /// Staged changes are allowed, but unstaged or untracked changes are not.
    StagedOnly,
}

/// Reads a typed factor-state boolean with a typed default.
fn read_state_bool_key_or_default(
    ctx: &Ctx<'_>,
    state_dir: &StateDir,
    key: StateFileKey,
    default: StateBool,
) -> Result<StateBool, FactorError> {
    read_state_bool_or_default(ctx, state_dir.as_path(), key.as_str(), default.as_bool())
        .map(StateBool::from_bool)
}

/// Reads a typed factor-state value.
fn read_state_key(
    ctx: &Ctx<'_>,
    state_dir: &StateDir,
    key: StateFileKey,
) -> Result<NonEmptyString, FactorError> {
    read_state(ctx, state_dir.as_path(), key.as_str())
}

/// Returns the joined exec pipeline used by factor gates.
#[expect(
    clippy::single_call_fn,
    reason = "exec commands are joined in one place so start and rebase preflight stay identical"
)]
fn joined_exec_command(exec: &NonEmpty<NonEmptyString>) -> NonEmptyString {
    let mut exec_command = exec.first().clone();
    for command in exec.iter().skip(1) {
        exec_command.push_str(" && ");
        exec_command.push_str(command.as_str());
    }
    exec_command
}

/// Formats captured stdout/stderr sections for diagnostics.
#[expect(
    clippy::single_call_fn,
    reason = "captured output formatting is centralized for consistent diagnostics"
)]
fn format_captured_output(stdout: &str, stderr: &str) -> String {
    let mut out = String::new();
    if !stdout.is_empty() {
        out.push_str("STDOUT:\n");
        out.push_str(stdout);
        if !stdout.ends_with('\n') {
            out.push('\n');
        }
    }
    if !stderr.is_empty() {
        out.push_str("STDERR:\n");
        out.push_str(stderr);
        if !stderr.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// Returns exact stdout/stderr text for a captured command result.
fn output_text(output: &Output) -> (String, String) {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (stdout, stderr)
}

/// Returns the exact porcelain status output, asserting success and empty stderr.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "status capture is isolated to keep repo-state enforcement readable"
    )
)]
fn repo_status_stdout(ctx: &Ctx<'_>) -> Result<String, FactorError> {
    let output = git_raw_output(ctx, &["status", "--porcelain=v1"])?;
    let (stdout, stderr) = output_text(&output);
    if !output.status.success() || !stderr.is_empty() {
        let mut message = format!(
            "git status --porcelain=v1 produced unexpected output (exit {})\n",
            status_code(output.status)
        );
        message.push_str(format_captured_output(stdout.as_str(), stderr.as_str()).as_str());
        return Err(FactorError::GitCommand(non_empty_msg(
            message.trim_end().to_owned(),
        )));
    }
    Ok(stdout)
}

/// Returns whether the porcelain output matches the required gate-boundary policy.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "repo-state policy checks stay centralized for gate-boundary invariants"
    )
)]
fn repo_status_matches_policy(stdout: &str, policy: RepoStatePolicy) -> bool {
    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        if matches!(policy, RepoStatePolicy::FullyClean) {
            return false;
        }
        if line.starts_with("??") || line.starts_with("!!") {
            return false;
        }
        let bytes = line.as_bytes();
        if bytes.get(1) != Some(&b' ') {
            return false;
        }
    }
    true
}

/// Enforces the expected repository cleanliness invariant.
fn ensure_repo_state(
    ctx: &Ctx<'_>,
    policy: RepoStatePolicy,
    message: &str,
) -> Result<(), FactorError> {
    let stdout = repo_status_stdout(ctx)?;
    if repo_status_matches_policy(stdout.as_str(), policy) {
        return Ok(());
    }
    let trimmed = stdout.trim_end();
    let mut full = message.to_owned();
    if !trimmed.is_empty() {
        full.push_str("\nSTATUS:\n");
        full.push_str(trimmed);
    }
    Err(FactorError::GitCommand(non_empty_msg(full)))
}

/// Rewrites the stored commit list entry at `index` with the current `HEAD` SHA.
#[expect(
    clippy::single_call_fn,
    reason = "state commit rewriting is intentionally centralized"
)]
fn update_current_commit_in_state(
    ctx: &Ctx<'_>,
    state_dir: &StateDir,
    index: CurrentIndex,
) -> Result<(), FactorError> {
    let commits_raw = read_state_key(ctx, state_dir, StateFileKey::Commits)?;
    let mut commits = commits_raw
        .as_str()
        .lines()
        .map(ToOwned::to_owned)
        .collect::<Vec<String>>();
    let Some(entry) = commits.get_mut(index.as_usize()) else {
        return Err(FactorError::GitCommand(non_empty_msg(format!(
            "commit index {} out of range (have {} commits)",
            index.as_usize(),
            commits.len()
        ))));
    };
    *entry = git_output(ctx, &["rev-parse", "HEAD^{commit}"])?;
    write_state(
        ctx,
        state_dir.as_path(),
        StateFileKey::Commits.as_str(),
        commits.join("\n").as_str(),
    )
}

/// Returns an absolute executable command prefix for hidden rebase helpers.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "current executable resolution is isolated for helper command construction"
    )
)]
fn current_exe_command_prefix(ctx: &Ctx<'_>) -> Result<String, FactorError> {
    let current_exe = ctx.env.current_exe().map_err(|err| {
        FactorError::GitCommand(non_empty_msg(format!(
            "cannot resolve current executable: {err}"
        )))
    })?;
    let current_exe_str = current_exe.to_str().ok_or_else(|| {
        FactorError::GitCommand(non_empty_msg(
            "current executable path is not valid UTF-8".to_owned(),
        ))
    })?;
    Ok(shell_quote(current_exe_str))
}

/// Joins commit SHAs into one hidden internal CLI argument.
#[expect(
    clippy::single_call_fn,
    reason = "commit-list encoding is centralized for hidden rebase helper commands"
)]
fn encode_internal_commits_arg(commits: &NonEmpty<CommitSha>) -> String {
    commits
        .iter()
        .map(CommitSha::as_str)
        .collect::<Vec<_>>()
        .join(",")
}

/// Builds a shell-safe hidden helper command for rebase `exec` lines.
fn rebase_exec_command(
    ctx: &Ctx<'_>,
    subcommand: &str,
    args: &[&str],
) -> Result<String, FactorError> {
    let prefix = current_exe_command_prefix(ctx)?;
    let mut command_parts = vec![prefix, shell_quote(subcommand)];
    command_parts.extend(args.iter().copied().map(shell_quote));

    Ok(command_parts.join(" "))
}

/// Builds the hidden preflight command for one rebase-backed factor step.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "preflight command assembly stays isolated for hidden rebase helper wiring"
    )
)]
fn rebase_exec_preflight_command(
    ctx: &Ctx<'_>,
    current_index: CurrentIndex,
    exec_command: &NonEmptyString,
) -> Result<String, FactorError> {
    let current_index_text = current_index.as_usize().to_string();
    rebase_exec_command(
        ctx,
        "rebase-exec-preflight",
        &[current_index_text.as_str(), exec_command.as_str()],
    )
}

/// Builds the hidden begin command for one rebase-backed factor step.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "begin command assembly stays isolated for hidden rebase helper wiring"
    )
)]
fn rebase_exec_begin_command(
    ctx: &Ctx<'_>,
    current_index: CurrentIndex,
    start_head: &CommitSha,
    is_root: bool,
    exec_command: &NonEmptyString,
    commits: &NonEmpty<CommitSha>,
) -> Result<String, FactorError> {
    let current_index_text = current_index.as_usize().to_string();
    let is_root_text = StateBool::from_bool(is_root).as_str();
    let commits_text = encode_internal_commits_arg(commits);
    rebase_exec_command(
        ctx,
        "rebase-exec-begin",
        &[
            current_index_text.as_str(),
            start_head.as_str(),
            is_root_text,
            exec_command.as_str(),
            commits_text.as_str(),
        ],
    )
}

/// Aborts the current factor session.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps abort workflow isolated for deterministic state cleanup"
    )
)]
fn cmd_abort_in(ctx: &Ctx<'_>) -> Result<i32, FactorError> {
    trace_note(ctx, "factor_cmd_abort", &[]);

    if !is_factor_active_in(ctx) {
        return Err(FactorError::NoActiveSession);
    }

    let session = Session::from_active(ctx)?;
    let fallback_requires_rebase = session.requires_rebase(StateBool::False)?;
    let started_rebase = session.started_rebase(fallback_requires_rebase)?;
    if started_rebase.as_bool() && is_mid_rebase_in(ctx) {
        run_git_non_interactive(ctx, &["rebase", "--abort"])?;
    }
    let reset_target = match session.start_head() {
        Ok(start_head) => start_head.to_string(),
        Err(FactorError::StateRead(err)) if err.kind() == io::ErrorKind::NotFound => {
            session.current_commit()?.to_string()
        }
        Err(err) => return Err(err),
    };
    run_git(ctx, &["reset", "--hard", "--quiet", reset_target.as_str()])?;
    run_git(ctx, &["clean", "--force", "--quiet", "-d"])?;
    session.remove_state_strict()?;

    output::aborted(ctx, is_mid_rebase_in(ctx))?;

    Ok(EXIT_OK)
}

/// Shows status for the current factor session.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "dedicated status formatter keeps command dispatch readable"
    )
)]
fn cmd_status_in(ctx: &Ctx<'_>) -> Result<i32, FactorError> {
    trace_note(ctx, "factor_cmd_status", &[]);

    if !is_factor_active_in(ctx) {
        output::status(ctx, None)?;
        return Ok(EXIT_OK);
    }

    let session = Session::from_active(ctx)?;
    let current_commit = session.current_commit()?;
    let current_index = session.current_index()?;
    let split_count = session.split_count()?;
    let requires_rebase = session.requires_rebase(StateBool::True)?;
    let is_root = session.is_root(StateBool::False)?;
    let phase = session.phase(SessionPhase::Splitting)?;
    let rebase_in_progress = is_mid_rebase_in(ctx);

    output::status(
        ctx,
        Some(output::SessionStatus::new(
            &current_commit,
            current_index,
            split_count,
            phase,
            requires_rebase,
            is_root,
            rebase_in_progress,
        )),
    )?;

    Ok(EXIT_OK)
}

/// Writes multiple state key/value pairs to the factor state directory.
fn write_state_pairs(
    ctx: &Ctx<'_>,
    state_dir: &StateDir,
    pairs: &[(StateFileKey, &str)],
) -> Result<(), FactorError> {
    for &(key, value) in pairs {
        write_state(ctx, state_dir.as_path(), key.as_str(), value)?;
    }
    Ok(())
}

/// Creates the persisted factor state for a newly active session.
#[expect(
    clippy::too_many_arguments,
    reason = "post-gate session bootstrap writes all persisted fields in one place"
)]
fn write_initial_session_state(
    ctx: &Ctx<'_>,
    state_dir: &StateDir,
    commits: &NonEmpty<CommitSha>,
    current_index: CurrentIndex,
    exec_command: &NonEmptyString,
    phase: SessionPhase,
    requires_rebase: StateBool,
    started_rebase: StateBool,
    start_head: &CommitSha,
    is_root: StateBool,
) -> Result<(), FactorError> {
    ctx.fs
        .create_dir_all(state_dir.as_path())
        .map_err(FactorError::StateWrite)?;
    let current_index_text = current_index.as_usize().to_string();
    let split_count_text = SplitCount::zero().as_u8().to_string();
    let commits_content = commits.iter().map(CommitSha::as_str).collect::<Vec<_>>();
    write_state(
        ctx,
        state_dir.as_path(),
        StateFileKey::Commits.as_str(),
        &commits_content.join("\n"),
    )?;
    write_state_pairs(
        ctx,
        state_dir,
        &[
            (StateFileKey::CurrentIndex, current_index_text.as_str()),
            (StateFileKey::Exec, exec_command.as_str()),
            (StateFileKey::Phase, phase.as_str()),
            (StateFileKey::SplitCount, split_count_text.as_str()),
            (StateFileKey::RequiresRebase, requires_rebase.as_str()),
            (StateFileKey::StartedRebase, started_rebase.as_str()),
            (StateFileKey::StartHead, start_head.as_str()),
            (StateFileKey::IsRoot, is_root.as_str()),
        ],
    )
}

/// Removes the factor state path, supporting either a directory or a stray file.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "low-level state-path removal stays isolated for direct cleanup tests"
    )
)]
fn remove_state_path(ctx: &Ctx<'_>, state_dir: &StateDir) -> io::Result<()> {
    let path = state_dir.as_path();
    if !ctx.fs.exists(path) {
        return Ok(());
    }
    if ctx.fs.is_dir(path) {
        return ctx.fs.remove_dir_all(path);
    }
    ctx.fs.remove_file(path)
}

/// Removes a state path and fails if cleanup does not succeed.
fn remove_state_path_required(ctx: &Ctx<'_>, state_dir: &StateDir) -> Result<(), FactorError> {
    let path = state_dir.as_path();
    remove_state_path(ctx, state_dir).map_err(|err| {
        FactorError::GitCommand(non_empty_msg(format!(
            "failed to remove factor state path '{}': {err}",
            path.display()
        )))
    })?;
    if ctx.fs.exists(path) {
        return Err(FactorError::GitCommand(non_empty_msg(format!(
            "factor state path '{}' still exists after cleanup",
            path.display()
        ))));
    }
    Ok(())
}

/// Captures the current `HEAD` tree hash into factor state as `expected_tree`.
fn capture_expected_tree_in_state(ctx: &Ctx<'_>, state_dir: &StateDir) -> Result<(), FactorError> {
    let expected_tree = TreeHash::new(&git_output(ctx, &["rev-parse", "HEAD^{tree}"])?)?;
    write_state(
        ctx,
        state_dir.as_path(),
        StateFileKey::ExpectedTree.as_str(),
        expected_tree.as_str(),
    )?;
    Ok(())
}

/// Writes the current session phase to factor state.
fn write_session_phase(
    ctx: &Ctx<'_>,
    state_dir: &StateDir,
    phase: SessionPhase,
) -> Result<(), FactorError> {
    write_state(
        ctx,
        state_dir.as_path(),
        StateFileKey::Phase.as_str(),
        phase.as_str(),
    )
}

/// Opens a pending-start baseline into an active split session.
fn enter_pending_split_session(
    ctx: &Ctx<'_>,
    session: &Session<'_>,
) -> Result<(String, String), FactorError> {
    let phase = session.phase(SessionPhase::Splitting)?;
    if phase != SessionPhase::PendingStart {
        return Err(FactorError::GitCommand(non_empty_msg(
            "factor session is not waiting to begin splitting".to_owned(),
        )));
    }

    ensure_repo_state(
        ctx,
        RepoStatePolicy::FullyClean,
        "baseline commit must be fully clean before opening the split session",
    )?;
    let reset_target = if session.current_target_is_root()? {
        git_output(
            ctx,
            &[
                "commit-tree",
                "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
                "-m",
                "empty",
            ],
        )?
    } else {
        format!("{}^", session.first_commit()?)
    };
    run_git(ctx, &["reset", "--quiet", reset_target.as_str()])?;
    write_session_phase(ctx, &session.state_dir, SessionPhase::Splitting)?;

    let current_commit = session.current_commit()?;
    let message = commit_message(ctx, &current_commit)?;
    let short_sha = git_output(ctx, &["rev-parse", "--short", current_commit.as_str()])?;

    Ok((short_sha, message))
}

/// Runs the start gate against the current baseline commit.
#[expect(
    clippy::single_call_fn,
    reason = "hidden rebase exec preflight stays isolated from CLI dispatch"
)]
fn cmd_rebase_exec_preflight_in(
    ctx: &Ctx<'_>,
    current_index: CurrentIndex,
    bootstrap_exec: &NonEmptyString,
) -> Result<i32, FactorError> {
    let current_index_text = current_index.as_usize().to_string();
    trace_note(
        ctx,
        "factor_rebase_exec_preflight",
        &[("current_index", current_index_text.as_str())],
    );
    let exec_command = match Session::from_active(ctx) {
        Ok(session) => {
            let expected_index = session.current_index()?;
            if expected_index != current_index {
                return Err(FactorError::GitCommand(non_empty_msg(format!(
                    "unexpected current_index: expected {}, got {}",
                    expected_index.as_usize(),
                    current_index.as_usize()
                ))));
            }
            session.exec()?
        }
        Err(FactorError::StateRead(err)) if err.kind() == io::ErrorKind::NotFound => {
            bootstrap_exec.clone()
        }
        Err(err) => return Err(err),
    };
    ensure_repo_state(
        ctx,
        RepoStatePolicy::FullyClean,
        "cannot run the start gate because the repository is not clean",
    )?;

    let output = command_output_with(ctx, "bash", &["-c", exec_command.as_str()])?;
    let (stdout, stderr) = output_text(&output);
    if !stdout.is_empty() {
        ctx.out(stdout.as_str())?;
    }
    if !stderr.is_empty() {
        ctx.err(stderr.as_str())?;
    }
    if !output.status.success() {
        let code = status_code(output.status);
        ctx.outln("FACTOR: Start gate failed.")?;
        ctx.outln(&format!("EXEC: {exec_command}"))?;
        ctx.outln(&format!("CODE: {code}"))?;
        ctx.outln("")?;
        ctx.outln("NEXT: Fix the problems in the current commit, then run:")?;
        ctx.outln("  git add <paths>")?;
        ctx.outln("  git commit --amend --no-edit")?;
        ctx.outln("  git rebase --continue")?;
        ctx.outln("")?;
        ctx.outln("When rebase pauses again, run:")?;
        ctx.outln("  git factor --continue")?;
        return Err(FactorError::ExecFailed {
            code,
            command: exec_command,
        });
    }

    ensure_repo_state(
        ctx,
        RepoStatePolicy::FullyClean,
        "start gate must not leave tracked, unstaged, or untracked changes behind",
    )?;
    Ok(EXIT_OK)
}

/// Captures the green baseline for the active target and marks the session pending.
#[expect(
    clippy::single_call_fn,
    reason = "hidden rebase exec begin stays isolated from CLI dispatch"
)]
fn cmd_rebase_exec_begin_in(
    ctx: &Ctx<'_>,
    current_index: CurrentIndex,
    bootstrap: &RebaseExecBeginBootstrap,
) -> Result<i32, FactorError> {
    let current_index_text = current_index.as_usize().to_string();
    trace_note(
        ctx,
        "factor_rebase_exec_begin",
        &[("current_index", current_index_text.as_str())],
    );
    ensure_repo_state(
        ctx,
        RepoStatePolicy::FullyClean,
        "cannot begin the factor session because the repository is not clean",
    )?;
    let session = match Session::from_active(ctx) {
        Ok(session) => {
            let expected_index = session.current_index()?;
            if expected_index != current_index {
                return Err(FactorError::GitCommand(non_empty_msg(format!(
                    "unexpected current_index: expected {}, got {}",
                    expected_index.as_usize(),
                    current_index.as_usize()
                ))));
            }
            session
        }
        Err(FactorError::StateRead(err)) if err.kind() == io::ErrorKind::NotFound => {
            let state_dir = factor_dir_in(ctx)?;
            write_initial_session_state(
                ctx,
                &state_dir,
                &bootstrap.commits,
                current_index,
                &bootstrap.exec_command,
                SessionPhase::PendingStart,
                StateBool::True,
                StateBool::True,
                &bootstrap.start_head,
                bootstrap.is_root,
            )?;
            Session::from_active(ctx)?
        }
        Err(err) => return Err(err),
    };
    let split_count_text = SplitCount::zero().as_u8().to_string();
    write_state_pairs(
        ctx,
        &session.state_dir,
        &[
            (StateFileKey::CurrentIndex, current_index_text.as_str()),
            (StateFileKey::SplitCount, split_count_text.as_str()),
        ],
    )?;
    update_current_commit_in_state(ctx, &session.state_dir, current_index)?;
    capture_expected_tree_in_state(ctx, &session.state_dir)?;
    write_session_phase(ctx, &session.state_dir, SessionPhase::PendingStart)?;
    Ok(EXIT_OK)
}

/// Restores index and worktree from the provided commit.
fn restore_staged_and_worktree_from_commit(
    ctx: &Ctx<'_>,
    original_commit: &CommitSha,
) -> Result<(), FactorError> {
    run_git(
        ctx,
        &[
            "restore",
            "--source",
            original_commit.as_str(),
            "--staged",
            "--worktree",
            "--",
            ".",
        ],
    )
}

/// Restores the remaining pool and returns unstaged/untracked summaries.
fn restore_remaining_pool(
    ctx: &Ctx<'_>,
    original_commit: &CommitSha,
    expected_tree: &TreeHash,
) -> Result<(String, String), FactorError> {
    restore_staged_and_worktree_from_commit(ctx, original_commit)?;
    let restored_tree = TreeHash::new(&git_output(ctx, &["write-tree"])?)?;
    if restored_tree != *expected_tree {
        return Err(FactorError::TreeHashMismatch {
            actual: restored_tree,
            expected: expected_tree.clone(),
        });
    }
    run_git(ctx, &["reset", "--quiet"])?;
    let stat_output = git_output(ctx, &["diff", "--stat"])?;
    let untracked_output = git_output(ctx, &["ls-files", "--others", "--exclude-standard"])?;
    Ok((stat_output, untracked_output))
}

/// Prints the restored remaining-pool summary and next-step guidance.
fn print_remaining_pool_state(
    ctx: &Ctx<'_>,
    heading: &str,
    stat_output: &str,
    untracked_output: &str,
) -> Result<(), FactorError> {
    let remaining = stat_output.lines().last().unwrap_or_default().to_owned();
    ctx.outln(heading)?;
    ctx.outln("STATE: Remaining changes are unstaged.")?;
    ctx.outln("UNSTAGED:")?;
    for line in stat_output.lines() {
        ctx.outln(&format!("  {line}"))?;
    }
    if !untracked_output.is_empty() {
        ctx.outln("UNTRACKED:")?;
        for line in untracked_output.lines() {
            ctx.outln(&format!("  {line}"))?;
        }
    }
    ctx.out("\n")?;
    print_continue_command(ctx, "NEXT: Stage changes for the next commit, then run:")?;
    ctx.out("\n")?;
    print_hints_with_remaining_in(ctx, remaining.as_str())?;
    Ok(())
}

/// Continues an in-progress factor session.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "continue path is intentionally extracted from CLI dispatch"
    )
)]
fn cmd_continue_in(ctx: &Ctx<'_>, messages: &NonEmpty<NonEmptyString>) -> Result<i32, FactorError> {
    trace_note(ctx, "factor_cmd_continue", &[]);
    if !is_factor_active_in(ctx) {
        return Err(FactorError::NoActiveSession);
    }
    let session = Session::from_active(ctx)?;
    if session.phase(SessionPhase::Splitting)? != SessionPhase::Splitting {
        return Err(FactorError::Usage(non_empty_msg(
            "run 'git factor --continue' with no --message to begin splitting this commit"
                .to_owned(),
        )));
    }
    let requires_rebase = session.requires_rebase(StateBool::True)?;
    if requires_rebase.as_bool() && !is_mid_rebase_in(ctx) {
        return Err(FactorError::GitCommand(non_empty_msg(
            "no rebase in progress".to_owned(),
        )));
    }
    let original_commit = session.current_commit()?;
    let exec_command = session.exec()?;
    if git_status(ctx, &["diff", "--quiet", "--staged"])?.success() {
        return Err(FactorError::NoStagedChanges);
    }
    run_git(ctx, &["checkout", "--quiet", "--", "."])?;
    run_git(ctx, &["clean", "--force", "--quiet", "-d"])?;
    run_git(ctx, &["checkout-index", "--all", "--force", "--quiet"])?;
    ensure_repo_state(
        ctx,
        RepoStatePolicy::StagedOnly,
        "continue gate requires staged changes only; remove unstaged or untracked changes first",
    )?;
    let exec_status =
        match command_status_with(ctx, "bash", &["-c", exec_command.as_str()], &[], false) {
            Ok(exec_status) => exec_status,
            Err(err) => {
                rehydrate_pool_preserving_index(ctx, &original_commit)?;
                return Err(err);
            }
        };
    if let Err(err) = ensure_repo_state(
        ctx,
        RepoStatePolicy::StagedOnly,
        "exec gate must not leave unstaged or untracked changes behind",
    ) {
        rehydrate_pool_preserving_index(ctx, &original_commit)?;
        return Err(err);
    }
    if !exec_status.success() {
        rehydrate_pool_preserving_index(ctx, &original_commit)?;
        let code = status_code(exec_status);
        ctx.outln("FACTOR: Exec gate failed. No commit created.")?;
        ctx.outln(&format!("EXEC: {exec_command}"))?;
        ctx.outln(&format!("CODE: {code}"))?;
        ctx.out("\n")?;
        print_continue_command(
            ctx,
            "NEXT: Adjust staged changes so the exec gate passes, then retry:",
        )?;
        return Err(FactorError::ExecFailed {
            code,
            command: exec_command,
        });
    }
    git_commit_preserving_metadata(ctx, &original_commit, messages, false)?;
    let split_count = session.increment_split_count()?;
    let head_tree = TreeHash::new(&git_output(ctx, &["rev-parse", "HEAD^{tree}"])?)?;
    let expected_tree = session.expected_tree()?;
    if trace_tree_convergence(ctx, "tree_compare_continue", &head_tree, &expected_tree) {
        let AdvanceOutcome::Completed { final_split_count } = session.advance_to_next_commit()?;
        return ctx
            .outln(&format!(
                "FACTOR: Complete. Final commit split into {} commits.",
                final_split_count.get()
            ))
            .map(|()| EXIT_OK);
    }

    let (stat_output, untracked_output) =
        restore_remaining_pool(ctx, &original_commit, &expected_tree)?;
    print_remaining_pool_state(
        ctx,
        format!("FACTOR: Split {split_count} committed.").as_str(),
        stat_output.as_str(),
        untracked_output.as_str(),
    )?;
    Ok(EXIT_OK)
}

/// Prints the standard `git factor --continue` command banner.
fn print_continue_command(ctx: &Ctx<'_>, heading: &str) -> Result<(), FactorError> {
    ctx.outln(heading)?;
    ctx.outln("  git factor --continue --message \"type: description\"")
}

/// Opens a pending-start factor session after rebase preflight/begin succeeded.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "pending-start transition stays isolated from normal continue flow"
    )
)]
fn cmd_continue_pending_start_in(ctx: &Ctx<'_>) -> Result<i32, FactorError> {
    if !is_factor_active_in(ctx) {
        return Err(FactorError::NoActiveSession);
    }
    let session = Session::from_active(ctx)?;
    let (short_sha, message) = enter_pending_split_session(ctx, &session)?;
    let started = split_started_line(short_sha.as_str(), None);
    print_session_started(ctx, started.as_str(), message.as_str())?;
    Ok(EXIT_OK)
}

/// Discards the current split attempt and restores the remaining pool.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "retry path stays isolated from continue/finish for session clarity"
    )
)]
fn cmd_retry_in(ctx: &Ctx<'_>) -> Result<i32, FactorError> {
    trace_note(ctx, "factor_cmd_retry", &[]);

    if !is_factor_active_in(ctx) {
        return Err(FactorError::NoActiveSession);
    }
    let session = Session::from_active(ctx)?;
    if session.phase(SessionPhase::Splitting)? != SessionPhase::Splitting {
        return Err(FactorError::Usage(non_empty_msg(
            "run 'git factor --continue' with no --message to begin splitting this commit"
                .to_owned(),
        )));
    }
    let requires_rebase = session.requires_rebase(StateBool::True)?;
    if requires_rebase.as_bool() && !is_mid_rebase_in(ctx) {
        return Err(FactorError::GitCommand(non_empty_msg(
            "no rebase in progress".to_owned(),
        )));
    }

    let original_commit = session.current_commit()?;
    let expected_tree = session.expected_tree()?;

    run_git(ctx, &["clean", "--force", "--quiet", "-d"])?;

    let (stat_output, untracked_output) =
        restore_remaining_pool(ctx, &original_commit, &expected_tree)?;
    print_remaining_pool_state(
        ctx,
        "FACTOR: Split attempt discarded.",
        stat_output.as_str(),
        untracked_output.as_str(),
    )?;
    Ok(EXIT_OK)
}

/// Traces tree hash comparison and returns whether the trees converged.
fn trace_tree_convergence(
    ctx: &Ctx<'_>,
    trace_event: &str,
    actual_tree: &TreeHash,
    expected_tree: &TreeHash,
) -> bool {
    let converged = actual_tree == expected_tree;
    let converged_flag = if converged { "true" } else { "false" };
    trace_note(
        ctx,
        trace_event,
        &[
            ("expected_tree", expected_tree.as_str()),
            ("actual_tree", actual_tree.as_str()),
            ("converged", converged_flag),
        ],
    );
    converged
}

/// Rehydrates the full original commit into the working tree while restoring
/// the index back to its prior state. This is used on exec-gate failure so the
/// user can adjust the staged slice without losing the remaining pool.
fn rehydrate_pool_preserving_index(
    ctx: &Ctx<'_>,
    original_commit: &CommitSha,
) -> Result<(), FactorError> {
    let idx_tree = TreeHash::new(&git_output(ctx, &["write-tree"])?)?;

    let status = git_status(
        ctx,
        &[
            "cherry-pick",
            "--no-commit",
            "--strategy-option",
            "theirs",
            original_commit.as_str(),
        ],
    )?;

    if !status.success() {
        let unmerged = git_output(ctx, &["diff", "--name-only", "--diff-filter=U"])?;
        drop(git_status(ctx, &["cherry-pick", "--abort"]));
        if unmerged.is_empty() {
            drop(git_status(ctx, &["cherry-pick", "--quit"]));
            return Err(FactorError::GitCommand(non_empty_msg(format!(
                "cherry-pick failed (exit {}) with no merge conflicts for {original_commit}",
                status_code(status)
            ))));
        }
        drop(git_status(ctx, &["cherry-pick", "--quit"]));
        return Err(FactorError::GitCommand(non_empty_msg(format!(
            "rehydrate cherry-pick left conflicts:\n{unmerged}"
        ))));
    }

    let quit_status = git_status(ctx, &["cherry-pick", "--quit"])?;
    if !quit_status.success() {
        drop(git_status(ctx, &["cherry-pick", "--abort"]));
        return Err(FactorError::GitCommand(non_empty_msg(format!(
            "git cherry-pick --quit failed (exit {})",
            status_code(quit_status)
        ))));
    }

    let read_tree_status = git_status(ctx, &["read-tree", idx_tree.as_str()])?;
    if !read_tree_status.success() {
        return Err(FactorError::GitCommand(non_empty_msg(format!(
            "git read-tree failed (exit {})",
            status_code(read_tree_status)
        ))));
    }

    Ok(())
}

/// Finishes the factor session by committing all remaining staged changes.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "finish path remains a dedicated command implementation"
    )
)]
fn cmd_finish_in(ctx: &Ctx<'_>, messages: &[NonEmptyString]) -> Result<i32, FactorError> {
    trace_note(ctx, "factor_cmd_finish", &[]);

    if !is_factor_active_in(ctx) {
        return Err(FactorError::NoActiveSession);
    }
    let session = Session::from_active(ctx)?;
    if session.phase(SessionPhase::Splitting)? != SessionPhase::Splitting {
        return Err(FactorError::Usage(non_empty_msg(
            "run 'git factor --continue' with no --message to begin splitting this commit"
                .to_owned(),
        )));
    }
    let requires_rebase = session.requires_rebase(StateBool::True)?;
    if requires_rebase.as_bool() && !is_mid_rebase_in(ctx) {
        return Err(FactorError::GitCommand(non_empty_msg(
            "no rebase in progress".to_owned(),
        )));
    }
    let original_commit = session.current_commit()?;
    let expected_tree = session.expected_tree()?;

    // Clean unstaged/untracked changes.
    run_git(ctx, &["checkout", "--quiet", "--", "."])?;
    run_git(ctx, &["clean", "--force", "--quiet", "-d"])?;

    // Restore the original commit tree directly into index/worktree.
    // This avoids merge/cherry-pick conflict mechanics during finish.
    restore_staged_and_worktree_from_commit(ctx, &original_commit)?;

    // Resolve effective messages: use original commit message when none provided.
    let effective_messages: NonEmpty<NonEmptyString> =
        if let Some((first, rest)) = messages.split_first() {
            let mut out = NonEmpty::new(first.clone());
            for msg in rest {
                out.push(msg.clone());
            }
            out
        } else {
            let original_msg = commit_message(ctx, &original_commit)?;
            NonEmpty::new(
                NonEmptyString::try_from(original_msg.trim_end().to_owned()).map_err(|_err| {
                    FactorError::GitCommand(non_empty_msg(
                        "original commit has empty message".to_owned(),
                    ))
                })?,
            )
        };
    // Verify tree hash matches the original commit before committing.
    let actual_tree = TreeHash::new(&git_output(ctx, &["write-tree"])?)?;
    if !trace_tree_convergence(ctx, "tree_compare_finish", &actual_tree, &expected_tree) {
        return Err(FactorError::TreeHashMismatch {
            actual: actual_tree,
            expected: expected_tree,
        });
    }

    // Handle the empty-commit case (or "finish called when nothing remains"):
    // if there are no staged changes, create an empty commit so the rebase edit
    // stop can be satisfied without silently dropping the original commit.
    let has_staged = git_status(ctx, &["diff", "--quiet", "--staged"])?;

    // Create the commit after proving the reconstructed tree matches baseline.
    git_commit_preserving_metadata(
        ctx,
        &original_commit,
        &effective_messages,
        has_staged.success(),
    )?;

    // Update split count and complete the factor span.
    session.increment_split_count()?;
    let AdvanceOutcome::Completed { final_split_count } = session.advance_to_next_commit()?;
    ctx.outln(&format!(
        "FACTOR: Complete. Final commit split into {} commits.",
        final_split_count.get()
    ))?;

    Ok(EXIT_OK)
}

/// Formats the banner shown when a split session starts or advances.
fn split_started_line(short_sha: &str, total_commits: Option<usize>) -> String {
    match total_commits {
        Some(total) if total > 1 => {
            format!("FACTOR: Split session started for {total} commits (tip: {short_sha}).")
        }
        Some(_) => format!("FACTOR: Split session started for {short_sha}."),
        None => format!("FACTOR: Now splitting {short_sha}."),
    }
}

/// Starts a new factor session.
fn cmd_start_prep_in(ctx: &Ctx<'_>) -> Result<StateDir, FactorError> {
    let state_dir = factor_dir_in(ctx)?;
    if ctx.fs.is_dir(state_dir.as_path()) {
        return Err(FactorError::ActiveSession);
    }
    if is_mid_rebase_in(ctx) {
        return Err(FactorError::ActiveRebase);
    }
    ensure_repo_state(
        ctx,
        RepoStatePolicy::FullyClean,
        "working tree must be clean before starting; stash, commit, or remove local changes",
    )?;
    Ok(state_dir)
}

/// Starts a new factor session from resolved commit SHAs.
fn cmd_start_with_resolved_in(
    ctx: &Ctx<'_>,
    exec: &NonEmpty<NonEmptyString>,
    state_dir: &StateDir,
    resolved_commits: &NonEmpty<CommitSha>,
) -> Result<i32, FactorError> {
    let head_commit = resolve_commit(ctx, "HEAD")?;
    for sha in resolved_commits {
        validate_split_target_in(ctx, sha)?;
    }
    let span = CommitSpan::new(
        resolved_commits.clone(),
        validation::base_parent_in(ctx, resolved_commits.first())?,
    );
    let span_tip = span.tip_commit();
    let short_sha = NonEmptyString::try_from(git_output(
        ctx,
        &["rev-parse", "--short", span_tip.as_str()],
    )?)
    .map_err(|_err| FactorError::GitCommand(non_empty_msg("empty short SHA".to_owned())))?;
    let message = commit_message(ctx, span_tip)?;
    let exec_command = joined_exec_command(exec);
    validate_exec_syntax(ctx, exec_command.as_str())?;
    let single_head_session = span.tip_commit() == &head_commit;
    let requires_rebase = StateBool::from_bool(!single_head_session);
    let is_root_state = StateBool::from_bool(span.is_root());
    let current_index = CurrentIndex(span.commits().tail.len());
    if single_head_session {
        let output = command_output_with(ctx, "bash", &["-c", exec_command.as_str()])?;
        let (stdout, stderr) = output_text(&output);
        if !stdout.is_empty() {
            ctx.out(stdout.as_str())?;
        }
        if !stderr.is_empty() {
            ctx.err(stderr.as_str())?;
        }
        if !output.status.success() {
            ctx.outln("FACTOR: Start gate failed.")?;
            ctx.outln(&format!("EXEC: {exec_command}"))?;
            ctx.outln(&format!("CODE: {}", status_code(output.status)))?;
            ctx.outln("")?;
            ctx.outln("NEXT: Fix the current commit, amend it, then rerun git factor.")?;
            return Err(FactorError::ExecFailed {
                code: status_code(output.status),
                command: exec_command,
            });
        }
        ensure_repo_state(
            ctx,
            RepoStatePolicy::FullyClean,
            "start gate must not leave tracked, unstaged, or untracked changes behind",
        )?;
        write_initial_session_state(
            ctx,
            state_dir,
            span.commits(),
            current_index,
            &exec_command,
            SessionPhase::Splitting,
            requires_rebase,
            StateBool::False,
            &head_commit,
            is_root_state,
        )?;
        capture_expected_tree_in_state(ctx, state_dir)?;
        let reset_target = if span.is_root() {
            git_output(
                ctx,
                &[
                    "commit-tree",
                    "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
                    "-m",
                    "empty",
                ],
            )?
        } else {
            format!("{}^", span.first_commit())
        };
        run_git(ctx, &["reset", "--quiet", reset_target.as_str()])?;

        let started = split_started_line(short_sha.as_str(), Some(span.len()));
        print_session_started(ctx, started.as_str(), &message)?;
        return Ok(EXIT_OK);
    }

    match run_start_rebase_in(ctx, &span, state_dir, &head_commit, &exec_command)? {
        StartRebaseOutcome::PausedAtBreak => {
            let session = Session::from_active(ctx)?;
            let (next_short_sha, next_message) = enter_pending_split_session(ctx, &session)?;
            let started = split_started_line(next_short_sha.as_str(), Some(span.len()));
            print_session_started(ctx, started.as_str(), next_message.as_str())?;
            Ok(EXIT_OK)
        }
        StartRebaseOutcome::WaitingForRecovery => Ok(EXIT_TEMPFAIL),
    }
}

#[cfg(test)]
fn cmd_start_in(
    ctx: &Ctx<'_>,
    exec: &NonEmpty<NonEmptyString>,
    commit_refs: &NonEmpty<NonEmptyString>,
) -> Result<i32, FactorError> {
    trace_note(ctx, "factor_cmd_start", &[]);
    let state_dir = cmd_start_prep_in(ctx)?;
    let span = resolve_commit_span(ctx, commit_refs)?;
    cmd_start_with_resolved_in(ctx, exec, &state_dir, &span)
}

/// Validates that a split target commit is reachable from `HEAD`.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "validation remains extracted for readability and targeted tests"
    )
)]
fn validate_split_target_in(ctx: &Ctx<'_>, sha: &CommitSha) -> Result<(), FactorError> {
    let ancestor_status = match command_status_with(
        ctx,
        "git",
        &["merge-base", "--is-ancestor", sha.as_str(), "HEAD"],
        &[],
        true,
    ) {
        Ok(status) => status,
        Err(err) => return Err(FactorError::GitCommand(non_empty_msg(err.to_string()))),
    };
    if !ancestor_status.success() {
        return Err(FactorError::NotAncestor(sha.clone()));
    }

    Ok(())
}

/// Starts interactive rebase and stops at each selected commit for splitting.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "rebase execution extracted to keep start orchestration focused"
    )
)]
fn run_start_rebase_in(
    ctx: &Ctx<'_>,
    span: &CommitSpan,
    state_dir: &StateDir,
    start_head: &CommitSha,
    exec_command: &NonEmptyString,
) -> Result<StartRebaseOutcome, FactorError> {
    let editor = editor_path(ctx)?;
    let editor_str = match editor.to_str() {
        Some(editor_str) => editor_str,
        None => {
            return Err(FactorError::GitCommand(non_empty_msg(
                "editor path is not valid UTF-8".to_owned(),
            )));
        }
    };
    let mut seq_parts: Vec<String> = vec![editor_str.to_owned()];
    let short = git_output(ctx, &["rev-parse", "--short", span.tip_commit().as_str()])?;
    let current_index = CurrentIndex(span.commits().tail.len());
    let preflight = rebase_exec_preflight_command(ctx, current_index, exec_command)?;
    let begin = rebase_exec_begin_command(
        ctx,
        current_index,
        start_head,
        span.is_root(),
        exec_command,
        span.commits(),
    )?;
    seq_parts.push("--factor-target".to_owned());
    seq_parts.push(short);
    seq_parts.push("--factor-preflight".to_owned());
    seq_parts.push(preflight);
    seq_parts.push("--factor-begin".to_owned());
    seq_parts.push(begin);
    let seq_editor = seq_parts
        .iter()
        .map(String::as_str)
        .map(shell_quote)
        .collect::<Vec<String>>()
        .join(" ");

    let parent = format!("{}^", span.first_commit());
    let mut rebase_args = vec![
        "rebase",
        "--empty",
        "drop",
        "--interactive",
        "--no-autosquash",
        "--no-autostash",
        "--no-rebase-merges",
        "--no-update-refs",
        "--no-stat",
        "--quiet",
        "--reschedule-failed-exec",
    ];
    if span.is_root() {
        rebase_args.push("--root");
    } else {
        rebase_args.push(parent.as_str());
    }
    let status = command_status_with(
        ctx,
        "git",
        &rebase_args,
        &[
            ("GIT_EDITOR", "false"),
            ("GIT_SEQUENCE_EDITOR", &seq_editor),
        ],
        false,
    )?;

    if status.success() {
        if is_mid_rebase_in(ctx) {
            return Ok(StartRebaseOutcome::PausedAtBreak);
        }
        remove_state_path_required(ctx, state_dir)?;
        return Err(FactorError::GitCommand(non_empty_msg(
            "git rebase finished without pausing at the factor session break".to_owned(),
        )));
    }
    if is_mid_rebase_in(ctx) {
        return Ok(StartRebaseOutcome::WaitingForRecovery);
    }
    remove_state_path_required(ctx, state_dir)?;
    Err(FactorError::GitCommand(non_empty_msg(format!(
        "git rebase failed (exit {})",
        status_code(status)
    ))))
}

/// Returns the full commit message for a given commit SHA.
fn commit_message(ctx: &Ctx<'_>, sha: &CommitSha) -> Result<String, FactorError> {
    git_output(ctx, &["show", "--format=%B", "--no-patch", sha.as_str()])
}

/// Returns the expected converged tree for the current split step.
///
/// New sessions persist this as `expected_tree`, derived from the live edit-stop
/// commit (`HEAD^{tree}`), which stays correct after rebase rewrites. For older
/// sessions created before this state key existed, fall back to deriving the
/// tree from the original commit SHA recorded in `commits`.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps legacy expected-tree fallback isolated from session reads"
    )
)]
fn expected_tree_for_current_step(
    ctx: &Ctx<'_>,
    state_dir: &StateDir,
    original_commit: &CommitSha,
) -> Result<TreeHash, FactorError> {
    if let Ok(tree_str) = read_state_key(ctx, state_dir, StateFileKey::ExpectedTree) {
        trace_note(
            ctx,
            "expected_tree_source",
            &[("source", "state"), ("expected_tree", tree_str.as_str())],
        );
        return TreeHash::new(&tree_str);
    }

    let raw = git_output(ctx, &["rev-parse", &format!("{original_commit}^{{tree}}")])?;
    trace_note(
        ctx,
        "expected_tree_source",
        &[
            ("source", "original_commit"),
            ("original_commit", original_commit.as_str()),
            ("expected_tree", raw.as_str()),
        ],
    );
    TreeHash::new(&raw)
}

/// Creates a git commit preserving the original author and committer metadata.
fn git_commit_preserving_metadata(
    ctx: &Ctx<'_>,
    original_commit: &CommitSha,
    messages: &NonEmpty<NonEmptyString>,
    allow_empty: bool,
) -> Result<(), FactorError> {
    // Derive author/committer metadata from the original commit.
    let raw = git_output(
        ctx,
        &[
            "show",
            "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
            "--no-patch",
            original_commit.as_str(),
        ],
    )?;
    let parts: Vec<&str> = raw.splitn(6, '\0').collect();
    let &[
        author_name,
        author_email,
        author_date,
        committer_name,
        committer_email,
        committer_date,
    ] = parts.as_slice()
    else {
        return Err(FactorError::GitCommand(non_empty_msg(format!(
            "truncated commit metadata: expected 6 fields, got {} for {original_commit}",
            parts.len()
        ))));
    };

    let mut commit_args: Vec<&str> = vec!["commit", "--quiet"];
    if allow_empty {
        commit_args.push("--allow-empty");
    }
    for msg in messages {
        commit_args.push("--message");
        commit_args.push(msg.as_str());
    }

    let envs = [
        ("GIT_AUTHOR_NAME", author_name),
        ("GIT_AUTHOR_EMAIL", author_email),
        ("GIT_AUTHOR_DATE", author_date),
        ("GIT_COMMITTER_NAME", committer_name),
        ("GIT_COMMITTER_EMAIL", committer_email),
        ("GIT_COMMITTER_DATE", committer_date),
    ];
    let status = command_status_with(ctx, "git", &commit_args, &envs, false)?;

    if status.success() {
        Ok(())
    } else {
        Err(FactorError::GitCommand(non_empty_msg(format!(
            "git commit failed (exit {})",
            status_code(status)
        ))))
    }
}

/// Entrypoint for the `git-factor` binary.
///
/// Returns an exit code suitable for `std::process::exit`.
#[inline]
#[must_use]
pub fn main_entry() -> i32 {
    use std::env;
    let args = env::args_os().collect::<Vec<OsString>>();
    main_entry_with_vec(&REAL_IO, build_ctx_from_cwd(REAL_ENV.current_dir()), &args)
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
    args: &[OsString],
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
///
/// This is the shared implementation for both the real CLI entrypoint and
/// unit tests that assert on exact output.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "single error-reporting shim avoids duplicating exit mapping logic"
    )
)]
fn run_and_report_with_args_vec(ctx: &Ctx<'_>, args: &[OsString]) -> i32 {
    match run_with_args_vec(ctx, args.to_owned()) {
        Ok(code) => code,
        Err(err) => {
            persist_unexpected_session_error(ctx, &err, args);
            let (code, message) = error_to_exit(&err);
            drop(ctx.errln(&message));
            code
        }
    }
}

/// Persists unexpected active-session failures for later debugging.
#[expect(
    clippy::single_call_fn,
    reason = "best-effort error-log persistence stays separate from exit reporting"
)]
fn persist_unexpected_session_error(ctx: &Ctx<'_>, err: &FactorError, args: &[OsString]) {
    if !err.should_persist_error_log() {
        return;
    }
    let Ok(state_dir) = factor_dir_in(ctx) else {
        return;
    };
    if !ctx.fs.is_dir(state_dir.as_path()) {
        return;
    }
    drop(write_error_log(ctx, args, err));
}

/// Parses one hidden internal UTF-8 argument from CLI argv.
fn parse_internal_utf8_arg<'args>(
    args: &'args [OsString],
    subcommand: &str,
    index: usize,
    label: &str,
) -> Result<&'args str, FactorError> {
    args.get(index).and_then(|arg| arg.to_str()).ok_or_else(|| {
        FactorError::Usage(non_empty_msg(format!(
            "{subcommand} {label} argument must be valid UTF-8"
        )))
    })
}

/// Parses the hidden internal current-index argument from CLI argv.
fn parse_internal_current_index_arg(
    args: &[OsString],
    subcommand: &str,
) -> Result<CurrentIndex, FactorError> {
    let raw = parse_internal_utf8_arg(args, subcommand, 2, "current-index")?;
    let parsed = raw.parse::<usize>().map_err(|_err| {
        FactorError::Usage(non_empty_msg(format!(
            "{subcommand} current-index argument must be a non-negative integer"
        )))
    })?;
    Ok(CurrentIndex(parsed))
}

/// Parses the hidden internal args for `rebase-exec-preflight`.
#[expect(
    clippy::single_call_fn,
    reason = "preflight argv parsing is isolated from the main CLI dispatch"
)]
fn parse_internal_preflight_args(
    args: &[OsString],
    subcommand: &str,
) -> Result<(CurrentIndex, NonEmptyString), FactorError> {
    if args.len() != 4 {
        return Err(FactorError::Usage(non_empty_msg(format!(
            "{subcommand} requires exactly two arguments: current-index and exec-command"
        ))));
    }
    let current_index = parse_internal_current_index_arg(args, subcommand)?;
    let exec_command = NonEmptyString::try_from(
        parse_internal_utf8_arg(args, subcommand, 3, "exec-command")?.to_owned(),
    )
    .map_err(|_err| {
        FactorError::Usage(non_empty_msg(format!(
            "{subcommand} exec-command argument must not be empty"
        )))
    })?;
    Ok((current_index, exec_command))
}

/// Parses the hidden internal args for `rebase-exec-begin`.
#[expect(
    clippy::single_call_fn,
    reason = "begin argv parsing is isolated from the main CLI dispatch"
)]
fn parse_internal_begin_args(
    args: &[OsString],
    subcommand: &str,
) -> Result<(CurrentIndex, RebaseExecBeginBootstrap), FactorError> {
    if args.len() != 7 {
        return Err(FactorError::Usage(non_empty_msg(format!(
            "{subcommand} requires exactly five arguments: current-index, start-head, is-root, exec-command, and commits"
        ))));
    }
    let current_index = parse_internal_current_index_arg(args, subcommand)?;
    let start_head =
        CommitSha::new(parse_internal_utf8_arg(args, subcommand, 3, "start-head")?.to_owned())?;
    let is_root = match parse_internal_utf8_arg(args, subcommand, 4, "is-root")? {
        "true" => StateBool::True,
        "false" => StateBool::False,
        raw => {
            return Err(FactorError::Usage(non_empty_msg(format!(
                "{subcommand} is-root argument must be 'true' or 'false', got '{raw}'"
            ))));
        }
    };
    let exec_command = NonEmptyString::try_from(
        parse_internal_utf8_arg(args, subcommand, 5, "exec-command")?.to_owned(),
    )
    .map_err(|_err| {
        FactorError::Usage(non_empty_msg(format!(
            "{subcommand} exec-command argument must not be empty"
        )))
    })?;
    let commits_raw = parse_internal_utf8_arg(args, subcommand, 6, "commits")?;
    let parsed_commits = commits_raw
        .split(',')
        .map(|sha| CommitSha::new(sha.to_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    let commits = NonEmpty::from_vec(parsed_commits).ok_or_else(|| {
        FactorError::Usage(non_empty_msg(format!(
            "{subcommand} commits argument must not be empty"
        )))
    })?;
    Ok((
        current_index,
        RebaseExecBeginBootstrap {
            commits,
            exec_command,
            is_root,
            start_head,
        },
    ))
}

/// Like [`run_and_report_with_args_vec`], but returns structured errors.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "central parse/dispatch function keeps CLI behavior consistent"
    )
)]
#[expect(
    clippy::too_many_lines,
    reason = "CLI dispatch keeps hidden helpers and public modes in one parser path"
)]
fn run_with_args_vec(ctx: &Ctx<'_>, args: Vec<OsString>) -> Result<i32, FactorError> {
    if let Some(subcommand) = args.get(1).and_then(|arg| arg.to_str()) {
        if subcommand == "rebase-exec-preflight" {
            let (current_index, exec_command) = parse_internal_preflight_args(&args, subcommand)?;
            return cmd_rebase_exec_preflight_in(ctx, current_index, &exec_command);
        }
        if subcommand == "rebase-exec-begin" {
            let (current_index, bootstrap) = parse_internal_begin_args(&args, subcommand)?;
            return cmd_rebase_exec_begin_in(ctx, current_index, &bootstrap);
        }
    }

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
        && !cli.retry()
        && !cli.finish()
        && !has_start_args()
        && cli.message().is_empty()
    {
        let help = Cli::command().render_long_help().to_string();
        ctx.out(&help)?;
        return Ok(EXIT_OK);
    }

    if cli.abort() {
        if cli.status()
            || cli.continue_flag()
            || cli.retry()
            || cli.finish()
            || has_start_args()
            || !cli.message().is_empty()
        {
            return Err(FactorError::Usage(non_empty_msg(
                "--abort cannot be combined with other options".to_owned(),
            )));
        }
        return cmd_abort_in(ctx);
    }

    if cli.status() {
        if cli.continue_flag()
            || cli.retry()
            || cli.finish()
            || has_start_args()
            || !cli.message().is_empty()
        {
            return Err(FactorError::Usage(non_empty_msg(
                "--status cannot be combined with other options".to_owned(),
            )));
        }
        return cmd_status_in(ctx);
    }

    if cli.retry() {
        if cli.continue_flag() || cli.finish() || has_start_args() || !cli.message().is_empty() {
            return Err(FactorError::Usage(non_empty_msg(
                "--retry cannot be combined with other options".to_owned(),
            )));
        }
        return cmd_retry_in(ctx);
    }

    if cli.finish() {
        if cli.continue_flag() || has_start_args() {
            return Err(FactorError::Usage(non_empty_msg(
                "--finish cannot be combined with --continue, --exec, or COMMIT".to_owned(),
            )));
        }
        return cmd_finish_in(ctx, cli.message());
    }

    if cli.continue_flag() {
        if has_start_args() {
            return Err(FactorError::Usage(non_empty_msg(
                "--continue cannot be combined with --exec or COMMIT".to_owned(),
            )));
        }
        if let Some(messages) = NonEmpty::from_vec(cli.message().to_vec()) {
            return cmd_continue_in(ctx, &messages);
        }
        if !is_factor_active_in(ctx) {
            return Err(FactorError::Usage(non_empty_msg(
                "--continue requires --message <MSG>".to_owned(),
            )));
        }
        let session = Session::from_active(ctx)?;
        if session.phase(SessionPhase::Splitting)? == SessionPhase::PendingStart {
            return cmd_continue_pending_start_in(ctx);
        }
        return Err(FactorError::Usage(non_empty_msg(
            "--continue requires --message <MSG>".to_owned(),
        )));
    }

    let exec = NonEmpty::from_vec(cli.exec().to_vec()).ok_or_else(|| {
        FactorError::Usage(non_empty_msg(
            "--exec <COMMAND> is required when starting a factor session".to_owned(),
        ))
    })?;
    if !cli.message().is_empty() {
        return Err(FactorError::Usage(non_empty_msg(
            "--message can only be used with --continue or --finish".to_owned(),
        )));
    }
    trace_note(ctx, "factor_cmd_start", &[]);
    if let Some(commits) = NonEmpty::from_vec(cli.commits().to_vec()) {
        let state_dir = cmd_start_prep_in(ctx)?;
        let span = resolve_commit_span(ctx, &commits)?;
        return cmd_start_with_resolved_in(ctx, &exec, &state_dir, &span);
    }
    let state_dir = cmd_start_prep_in(ctx)?;
    let resolved_commits = NonEmpty::new(resolve_commit(ctx, "HEAD")?);
    cmd_start_with_resolved_in(ctx, &exec, &state_dir, &resolved_commits)
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "preserve the established inline test layout"
)]
mod proptests {
    mod main_entry;

    use core::cell::RefCell;
    use core::mem;
    use std::env;
    use std::ffi::OsString;
    use std::fs;
    use std::io;
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt as _;
    use std::path::{Path, PathBuf};
    use std::process::{self, ExitStatus, Output};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::non_empty_string::NonEmptyString;
    use nonempty::NonEmpty;
    use proptest::prelude::*;
    use proptest::string::string_regex;
    use tempfile::TempDir;

    use super::*;

    #[derive(Default)]
    pub(in crate::git_factor) struct TestIo {
        err: RefCell<String>,
        out: RefCell<String>,
    }

    impl Io for TestIo {
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

    #[derive(Clone, Copy)]
    pub(in crate::git_factor) struct TestEnv;

    impl Env for TestEnv {
        fn current_dir(&self) -> io::Result<PathBuf> {
            Ok(PathBuf::from("."))
        }

        fn current_exe(&self) -> io::Result<PathBuf> {
            Ok(PathBuf::from("git-factor"))
        }

        fn var_os(&self, _key: &str) -> Option<OsString> {
            None
        }
    }

    #[derive(Clone, Copy)]
    pub(in crate::git_factor) struct TestFs;

    impl Fs for TestFs {
        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            Ok(path.to_path_buf())
        }

        fn create_dir_all(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }

        fn exists(&self, _path: &Path) -> bool {
            false
        }

        fn is_dir(&self, _path: &Path) -> bool {
            false
        }

        fn read_to_string(&self, _path: &Path) -> io::Result<String> {
            Err(io::Error::new(io::ErrorKind::NotFound, "not found"))
        }

        fn remove_dir_all(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }

        fn remove_file(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }

        fn write_string(&self, _path: &Path, _content: &str) -> io::Result<()> {
            Ok(())
        }
    }

    #[derive(Clone, Copy)]
    pub(in crate::git_factor) struct OverflowSplitCountFs;

    impl Fs for OverflowSplitCountFs {
        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            Ok(path.to_path_buf())
        }

        fn create_dir_all(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }

        fn exists(&self, _path: &Path) -> bool {
            false
        }

        fn is_dir(&self, path: &Path) -> bool {
            path.ends_with("rebase-merge") || path.ends_with("rebase-apply")
        }

        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            if path.ends_with("split_count") {
                Ok(u8::MAX.to_string())
            } else if path.ends_with("requires_rebase") {
                Ok("true".to_owned())
            } else if path.ends_with("current_index") {
                Ok(usize::MAX.to_string())
            } else {
                Err(io::Error::new(io::ErrorKind::NotFound, "not found"))
            }
        }

        fn remove_dir_all(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }

        fn remove_file(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }

        fn write_string(&self, _path: &Path, _content: &str) -> io::Result<()> {
            Ok(())
        }
    }

    #[cfg(unix)]
    #[derive(Clone, Copy)]
    pub(in crate::git_factor) struct NonUtf8Fs;

    #[cfg(unix)]
    impl Fs for NonUtf8Fs {
        fn canonicalize(&self, _path: &Path) -> io::Result<PathBuf> {
            let mut bytes = b"/tmp/".to_vec();
            bytes.push(0xff);
            bytes.extend_from_slice(b"/bin/git-factor");
            Ok(PathBuf::from(OsString::from_vec(bytes)))
        }

        fn create_dir_all(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }

        fn exists(&self, _path: &Path) -> bool {
            false
        }

        fn is_dir(&self, _path: &Path) -> bool {
            false
        }

        fn read_to_string(&self, _path: &Path) -> io::Result<String> {
            Err(io::Error::new(io::ErrorKind::NotFound, "not found"))
        }

        fn remove_dir_all(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }

        fn remove_file(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }

        fn write_string(&self, _path: &Path, _content: &str) -> io::Result<()> {
            Ok(())
        }
    }

    #[derive(Clone, Copy)]
    pub(in crate::git_factor) struct TestRunner;

    impl Runner for TestRunner {
        fn output(&self, _bin: &str, _args: &[&str], _cwd: &Path) -> io::Result<Output> {
            Err(io::Error::other("unused in parser-only tests"))
        }

        fn status(
            &self,
            _bin: &str,
            _args: &[&str],
            _envs: &[(&str, &str)],
            _quiet: bool,
            _cwd: &Path,
        ) -> io::Result<ExitStatus> {
            Err(io::Error::other("unused in parser-only tests"))
        }
    }

    #[expect(
        clippy::field_scoped_visibility_modifiers,
        reason = "the moved launcher witnesses read calls from a sibling test module"
    )]
    pub(in crate::git_factor) struct ScriptedRunner {
        outputs: RefCell<Vec<io::Result<Output>>>,
        pub(in crate::git_factor) status_calls: RefCell<Vec<Vec<String>>>,
        statuses: RefCell<Vec<io::Result<ExitStatus>>>,
    }

    impl ScriptedRunner {
        pub(in crate::git_factor) fn new(
            outputs: Vec<io::Result<Output>>,
            statuses: Vec<io::Result<ExitStatus>>,
        ) -> Self {
            let mut reversed_outputs = outputs;
            reversed_outputs.reverse();
            let mut reversed_statuses = statuses;
            reversed_statuses.reverse();
            Self {
                outputs: RefCell::new(reversed_outputs),
                status_calls: RefCell::new(Vec::new()),
                statuses: RefCell::new(reversed_statuses),
            }
        }
    }

    impl Runner for ScriptedRunner {
        fn output(&self, _bin: &str, _args: &[&str], _cwd: &Path) -> io::Result<Output> {
            self.outputs
                .borrow_mut()
                .pop()
                .or_abort("missing scripted output")
        }

        fn status(
            &self,
            _bin: &str,
            args: &[&str],
            _envs: &[(&str, &str)],
            _quiet: bool,
            _cwd: &Path,
        ) -> io::Result<ExitStatus> {
            self.status_calls.borrow_mut().push(
                args.iter()
                    .map(|arg| (*arg).to_owned())
                    .collect::<Vec<String>>(),
            );
            self.statuses
                .borrow_mut()
                .pop()
                .or_abort("missing scripted status")
        }
    }

    fn test_ctx() -> (Ctx<'static>, &'static TestIo) {
        let io = Box::leak(Box::new(TestIo::default()));
        let env = Box::leak(Box::new(TestEnv));
        let fs = Box::leak(Box::new(TestFs));
        let runner = Box::leak(Box::new(TestRunner));
        (
            Ctx {
                cwd: PathBuf::from("."),
                runner,
                io,
                env,
                fs,
            },
            io,
        )
    }

    #[test]
    fn io_line_methods_cover_default_real_and_test_impls() {
        REAL_IO
            .errln("real-err")
            .or_abort("real errln should succeed");
        REAL_IO
            .outln("real-out")
            .or_abort("real outln should succeed");

        let test_io = TestIo::default();
        test_io
            .errln("test-err")
            .or_abort("test errln should succeed");
        test_io
            .outln("test-out")
            .or_abort("test outln should succeed");
        assert_eq!(test_io.err.borrow().as_str(), "test-err\n");
        assert_eq!(test_io.out.borrow().as_str(), "test-out\n");

        let dyn_test_io = TestIo::default();
        let dyn_io: &dyn Io = &dyn_test_io;
        dyn_io
            .outln("dyn-test-out")
            .or_abort("dyn test outln should succeed");
        assert_eq!(dyn_test_io.out.borrow().as_str(), "dyn-test-out\n");

        let ufcs_test_io = TestIo::default();
        <TestIo as Io>::outln(&ufcs_test_io, "ufcs-test-out")
            .or_abort("ufcs test outln should succeed");
        <TestIo as Io>::outln(&ufcs_test_io, "ufcs-test-out-2")
            .or_abort("second ufcs test outln should succeed");
        assert_eq!(
            ufcs_test_io.out.borrow().as_str(),
            "ufcs-test-out\nufcs-test-out-2\n"
        );

        let (ctx, ctx_io) = test_ctx();
        ctx.outln("ctx-test-out")
            .or_abort("ctx outln should succeed");
        assert_eq!(ctx_io.out.borrow().as_str(), "ctx-test-out\n");
    }

    proptest! {
        #[test]
        fn proptest_run_with_args_vec_reports_parse_errors(
            invalid_flag in string_regex("zz[a-z0-9]{1,12}").or_abort("valid regex"),
        ) {
            let (ctx, io) = test_ctx();
            let args = vec![
                OsString::from("git-factor"),
                OsString::from(format!("--{invalid_flag}")),
            ];

            let code = run_with_args_vec(&ctx, args).or_abort("parse errors map to exit code");
            prop_assert_eq!(code, EXIT_USAGE);
            prop_assert!(!io.err.borrow().is_empty());
        }

        #[test]
        fn proptest_run_with_args_vec_rejects_abort_combinations(
            include_status in any::<bool>(),
            include_continue in any::<bool>(),
            include_finish in any::<bool>(),
        ) {
            prop_assume!(include_status || include_continue || include_finish);

            let (ctx, _io) = test_ctx();
            let mut args = vec![OsString::from("git-factor"), OsString::from("--abort")];
            if include_status {
                args.push(OsString::from("--status"));
            }
            if include_continue {
                args.push(OsString::from("--continue"));
            }
            if include_finish {
                args.push(OsString::from("--finish"));
            }

            let err = run_with_args_vec(&ctx, args).err_or_abort("invalid combination should fail");
            prop_assert!(matches!(err, FactorError::Usage(_)));
        }
    }

    #[test]
    fn proptest_run_with_args_vec_no_options_prints_help() {
        let (ctx, io) = test_ctx();
        let args = vec![OsString::from("git-factor")];

        let code = run_with_args_vec(&ctx, args).or_abort("no-options path should succeed");
        assert_eq!(code, EXIT_OK);
        assert!(io.out.borrow().contains("Usage:"));
    }

    #[test]
    fn proptest_run_with_args_vec_continue_requires_message() {
        let (ctx, _io) = test_ctx();
        let args = vec![OsString::from("git-factor"), OsString::from("--continue")];

        let err =
            run_with_args_vec(&ctx, args).err_or_abort("continue without message should fail");
        assert_eq!(
            mem::discriminant(&err),
            mem::discriminant(&FactorError::Usage(non_empty_msg("x".to_owned())))
        );
    }

    #[test]
    fn proptest_run_and_report_with_args_vec_converts_usage_errors() {
        let (ctx, io) = test_ctx();
        let args = vec![
            OsString::from("git-factor"),
            OsString::from("--abort"),
            OsString::from("--status"),
        ];

        let code = run_and_report_with_args_vec(&ctx, &args);
        assert_eq!(code, EXIT_USAGE);
        assert!(!io.err.borrow().is_empty());
    }

    #[test]
    fn proptest_main_entry_with_vec_reports_ctx_errors() {
        let io = Box::leak(Box::new(TestIo::default()));
        let code = main_entry_with_vec(
            io,
            Err(FactorError::Usage(non_empty_msg(
                "ctx setup failed".to_owned(),
            ))),
            &[OsString::from("git-factor")],
        );
        assert_eq!(code, EXIT_USAGE);
        assert!(!io.err.borrow().is_empty());
    }

    #[test]
    fn proptest_main_entry_with_vec_passes_through_success_path() {
        let (ctx, io) = test_ctx();
        let code = main_entry_with_vec(io, Ok(ctx), &[OsString::from("git-factor")]);

        assert_eq!(code, EXIT_OK);
        assert!(io.out.borrow().contains("Usage:"));
    }

    #[test]
    fn proptest_main_entry_with_vec_uses_default_program_name_when_args_are_empty() {
        let (ctx, io) = test_ctx();
        let code = main_entry_with_vec(io, Ok(ctx), &[]);

        assert_eq!(code, EXIT_OK);
        assert!(io.out.borrow().contains("Usage:"));
    }

    #[test]
    fn proptest_build_ctx_from_cwd_reports_error() {
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
    fn proptest_build_ctx_from_cwd_returns_real_context_on_success() {
        let cwd = PathBuf::from("repo");
        let ctx = build_ctx_from_cwd(Ok(cwd.clone())).or_abort("cwd success should build context");

        assert_eq!(ctx.cwd, cwd);
        assert_eq!(
            ctx.env.current_dir().or_abort("ctx env current_dir"),
            env::current_dir().or_abort("host current_dir")
        );
    }

    #[test]
    fn proptest_test_env_and_fs_trait_methods_are_exercised() {
        let env = TestEnv;
        assert_eq!(
            env.current_dir().or_abort("current_dir"),
            PathBuf::from(".")
        );
        assert_eq!(
            env.current_exe().or_abort("current_exe"),
            PathBuf::from("git-factor")
        );
        assert_eq!(env.var_os("GIT_FACTOR_TEST"), None);

        let fs = TestFs;
        let path = PathBuf::from("tmp");
        fs.create_dir_all(path.as_path()).or_abort("create_dir_all");
        fs.remove_dir_all(path.as_path()).or_abort("remove_dir_all");
        fs.remove_file(path.as_path()).or_abort("remove_file");
        fs.write_string(path.as_path(), "value")
            .or_abort("write_string");
        assert_eq!(
            fs.canonicalize(path.as_path()).or_abort("canonicalize"),
            PathBuf::from("tmp")
        );
        assert!(!fs.is_dir(path.as_path()));
        assert!(!fs.exists(path.as_path()));
        let read_err = fs
            .read_to_string(path.as_path())
            .err_or_abort("read_to_string should fail");
        assert_eq!(read_err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn overflow_split_count_fs_trait_methods_are_exercised() {
        let fs = OverflowSplitCountFs;
        let path = PathBuf::from("tmp");
        fs.create_dir_all(path.as_path()).or_abort("create_dir_all");
        fs.remove_dir_all(path.as_path()).or_abort("remove_dir_all");
        fs.remove_file(path.as_path()).or_abort("remove_file");
        fs.write_string(path.as_path(), "value")
            .or_abort("write_string");
        assert_eq!(
            fs.canonicalize(path.as_path()).or_abort("canonicalize"),
            PathBuf::from("tmp")
        );
        assert!(!fs.is_dir(path.as_path()));
        assert!(!fs.exists(path.as_path()));

        let non_split_err = fs
            .read_to_string(path.as_path())
            .err_or_abort("non split_count read_to_string should fail");
        assert_eq!(non_split_err.kind(), io::ErrorKind::NotFound);

        assert_eq!(
            fs.read_to_string(Path::new(".git/factor/split_count"))
                .or_abort("split_count should read"),
            u8::MAX.to_string()
        );
    }

    pub(in crate::git_factor) fn success_status() -> ExitStatus {
        match () {
            #[cfg(unix)]
            () => {
                use std::os::unix::process::ExitStatusExt as _;
                ExitStatus::from_raw(0)
            }
            #[cfg(windows)]
            () => {
                use std::os::windows::process::ExitStatusExt as _;
                ExitStatus::from_raw(0)
            }
            #[cfg(not(any(unix, windows)))]
            () => panic!("unsupported platform"),
        }
    }

    fn failure_status() -> ExitStatus {
        match () {
            #[cfg(unix)]
            () => {
                use std::os::unix::process::ExitStatusExt as _;
                ExitStatus::from_raw(1 << 8)
            }
            #[cfg(windows)]
            () => {
                use std::os::windows::process::ExitStatusExt as _;
                ExitStatus::from_raw(1)
            }
            #[cfg(not(any(unix, windows)))]
            () => panic!("unsupported platform"),
        }
    }

    #[test]
    fn proptest_test_runner_trait_methods_are_exercised() {
        let runner = TestRunner;
        let cwd = Path::new(".");

        let output_err = runner
            .output("git", &["status"], cwd)
            .err_or_abort("output should fail in parser-only test runner");
        assert_eq!(output_err.kind(), io::ErrorKind::Other);
        assert_eq!(output_err.to_string(), "unused in parser-only tests");

        let status_err = runner
            .status("git", &["status"], &[], false, cwd)
            .err_or_abort("status should fail in parser-only test runner");
        assert_eq!(status_err.kind(), io::ErrorKind::Other);
        assert_eq!(status_err.to_string(), "unused in parser-only tests");

        let _status = success_status();
    }

    #[test]
    fn success_status_reports_success() {
        assert!(success_status().success());
    }

    #[test]
    fn failure_status_reports_failure() {
        assert!(!failure_status().success());
    }

    fn completed_split_count(outcome: &AdvanceOutcome) -> NonZeroU8 {
        match *outcome {
            AdvanceOutcome::Completed { final_split_count } => final_split_count,
        }
    }

    #[test]
    fn increment_split_count_reports_overflow() {
        let io = Box::leak(Box::new(TestIo::default()));
        let env = Box::leak(Box::new(TestEnv));
        let fs = Box::leak(Box::new(OverflowSplitCountFs));
        let runner = Box::leak(Box::new(TestRunner));
        let ctx = Ctx {
            cwd: PathBuf::from("."),
            env,
            fs,
            io,
            runner,
        };

        let err = Session::with_state(&ctx, StateDir::new(PathBuf::from(".git/factor")), vec![])
            .increment_split_count()
            .err_or_abort("u32::MAX split_count should overflow");
        assert_eq!(err.to_string(), "git command failed: split_count overflow");
    }

    #[test]
    fn session_from_active_loads_state_and_accessors() {
        let dir = TempDir::new().or_abort("tempdir");
        let repo = dir.path();
        let git_dir = repo.join(".git");
        let state_dir = git_dir.join("factor");
        fs::create_dir_all(&state_dir).or_abort("create factor dir");

        let current_commit = "0123456789abcdef0123456789abcdef01234567";
        let start_head = "89abcdef0123456789abcdef0123456789abcdef";
        let expected_tree = "dddddddddddddddddddddddddddddddddddddddd";
        fs::write(state_dir.join("commits"), format!("{current_commit}\n"))
            .or_abort("write commits");
        fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
        fs::write(state_dir.join("split_count"), "1\n").or_abort("write split_count");
        fs::write(state_dir.join("exec"), "true\n").or_abort("write exec");
        fs::write(state_dir.join("start_head"), format!("{start_head}\n"))
            .or_abort("write start_head");
        fs::write(state_dir.join("started_rebase"), "true\n").or_abort("write started_rebase");
        fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
        fs::write(state_dir.join("is_root"), "true\n").or_abort("write is_root");
        fs::write(
            state_dir.join("expected_tree"),
            format!("{expected_tree}\n"),
        )
        .or_abort("write expected_tree");

        let io = Box::leak(Box::new(TestIo::default()));
        let env = Box::leak(Box::new(TestEnv));
        let fs = &REAL_FS;
        let runner = Box::leak(Box::new(ScriptedRunner::new(
            vec![Ok(Output {
                status: success_status(),
                stdout: b".git\n".to_vec(),
                stderr: Vec::new(),
            })],
            Vec::new(),
        )));
        let ctx = Ctx {
            cwd: repo.to_path_buf(),
            env,
            fs,
            io,
            runner,
        };

        let session = Session::from_active(&ctx).or_abort("load active session");
        assert_eq!(
            session.current_commit().or_abort("current commit").as_str(),
            current_commit
        );
        assert_eq!(
            session.current_index().or_abort("current index").as_usize(),
            0
        );
        assert_eq!(session.exec().or_abort("exec").as_str(), "true");
        assert_eq!(
            session.start_head().or_abort("start head").as_str(),
            start_head
        );
        assert!(
            session
                .started_rebase(StateBool::False)
                .or_abort("started_rebase")
                .as_bool()
        );
        assert!(
            !session
                .requires_rebase(StateBool::True)
                .or_abort("requires_rebase")
                .as_bool()
        );
        assert!(
            session
                .is_root(StateBool::False)
                .or_abort("is_root")
                .as_bool()
        );
        assert_eq!(session.split_count().or_abort("split_count").as_u8(), 1);
        assert_eq!(
            session.expected_tree().or_abort("expected tree").as_str(),
            expected_tree
        );
    }

    #[test]
    fn session_from_active_rejects_invalid_commit_entries() {
        let dir = TempDir::new().or_abort("tempdir");
        let repo = dir.path();
        let state_dir = repo.join(".git").join("factor");
        fs::create_dir_all(&state_dir).or_abort("create factor dir");
        fs::write(state_dir.join("commits"), "bad\n").or_abort("write commits");

        let io = Box::leak(Box::new(TestIo::default()));
        let env = Box::leak(Box::new(TestEnv));
        let fs = &REAL_FS;
        let runner = Box::leak(Box::new(ScriptedRunner::new(
            vec![Ok(Output {
                status: success_status(),
                stdout: b".git\n".to_vec(),
                stderr: Vec::new(),
            })],
            Vec::new(),
        )));
        let ctx = Ctx {
            cwd: repo.to_path_buf(),
            env,
            fs,
            io,
            runner,
        };

        let err = Session::from_active(&ctx)
            .map(|_session| ())
            .err_or_abort("invalid commits state should fail");
        assert_eq!(err.to_string(), "invalid commit: bad");
    }

    #[test]
    fn session_expected_tree_propagates_current_commit_lookup_error() {
        let dir = TempDir::new().or_abort("tempdir");
        let state_dir = dir.path().join(".git").join("factor");
        fs::create_dir_all(&state_dir).or_abort("create factor dir");
        fs::write(state_dir.join("current_index"), "1\n").or_abort("write current_index");

        let io = Box::leak(Box::new(TestIo::default()));
        let env = Box::leak(Box::new(TestEnv));
        let fs = &REAL_FS;
        let runner = Box::leak(Box::new(TestRunner));
        let ctx = Ctx {
            cwd: dir.path().to_path_buf(),
            env,
            fs,
            io,
            runner,
        };
        let commit = CommitSha::new("a".repeat(COMMIT_SHA_HEX_LEN)).or_abort("valid sha");

        let err = Session::with_state(&ctx, StateDir::new(state_dir), vec![commit])
            .expected_tree()
            .err_or_abort("missing current commit should fail");
        assert_eq!(
            err.to_string(),
            "git command failed: commit index 1 out of range (have 1 commits)"
        );
    }

    #[test]
    fn advance_to_next_commit_reports_zero_split_count_on_advance() {
        let dir = TempDir::new().or_abort("tempdir");
        let repo = dir.path();
        let rebase_dir = repo.join(".git").join("rebase-merge");
        let state_dir = repo.join(".git").join("factor");
        fs::create_dir_all(&rebase_dir).or_abort("create rebase dir");
        fs::create_dir_all(&state_dir).or_abort("create factor dir");

        let first_commit = CommitSha::new("a".repeat(COMMIT_SHA_HEX_LEN)).or_abort("first sha");
        let second_commit = CommitSha::new("b".repeat(COMMIT_SHA_HEX_LEN)).or_abort("second sha");
        fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
        fs::write(state_dir.join("requires_rebase"), "true\n").or_abort("write requires_rebase");
        fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");

        let io = Box::leak(Box::new(TestIo::default()));
        let env = Box::leak(Box::new(TestEnv));
        let fs = &REAL_FS;
        let runner = Box::leak(Box::new(ScriptedRunner::new(
            vec![
                Ok(Output {
                    status: success_status(),
                    stdout: b".git\n".to_vec(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: b".git\n".to_vec(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: b"next message\n".to_vec(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: b"bbbbbbb\n".to_vec(),
                    stderr: Vec::new(),
                }),
            ],
            vec![Ok(success_status()), Ok(success_status())],
        )));
        let ctx = Ctx {
            cwd: repo.to_path_buf(),
            env,
            fs,
            io,
            runner,
        };

        let err = Session::with_state(
            &ctx,
            StateDir::new(state_dir),
            vec![first_commit, second_commit],
        )
        .advance_to_next_commit()
        .err_or_abort("zero split count should fail");
        assert_eq!(
            err.to_string(),
            "git command failed: split_count is zero at advance"
        );
    }

    #[test]
    fn advance_to_next_commit_errors_when_rebase_remains_active_after_span_completion() {
        let dir = TempDir::new().or_abort("tempdir");
        let repo = dir.path();
        let rebase_dir = repo.join(".git").join("rebase-merge");
        let state_dir = repo.join(".git").join("factor");
        fs::create_dir_all(&rebase_dir).or_abort("create rebase dir");
        fs::create_dir_all(&state_dir).or_abort("create factor dir");

        let first_commit = CommitSha::new("a".repeat(COMMIT_SHA_HEX_LEN)).or_abort("first sha");
        let second_commit = CommitSha::new("b".repeat(COMMIT_SHA_HEX_LEN)).or_abort("second sha");
        fs::write(state_dir.join("split_count"), "1\n").or_abort("write split_count");
        fs::write(state_dir.join("requires_rebase"), "true\n").or_abort("write requires_rebase");
        fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");

        let io = Box::leak(Box::new(TestIo::default()));
        let env = Box::leak(Box::new(TestEnv));
        let fs = &REAL_FS;
        let runner = Box::leak(Box::new(ScriptedRunner::new(
            vec![
                Ok(Output {
                    status: success_status(),
                    stdout: b".git\n".to_vec(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: b".git\n".to_vec(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: b"next message\n".to_vec(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: b"bbbbbbb\n".to_vec(),
                    stderr: Vec::new(),
                }),
            ],
            vec![Ok(success_status()), Ok(success_status())],
        )));
        let ctx = Ctx {
            cwd: repo.to_path_buf(),
            env,
            fs,
            io,
            runner,
        };

        let err = Session::with_state(
            &ctx,
            StateDir::new(state_dir.clone()),
            vec![first_commit, second_commit],
        )
        .advance_to_next_commit()
        .err_or_abort("mid-rebase span completion should fail closed");
        assert_eq!(
            err.to_string(),
            "git command failed: rebase remained active after span completion"
        );
        assert!(state_dir.exists(), "state dir should be preserved on error");
    }

    #[test]
    fn advance_to_next_commit_reports_zero_split_count_on_completion() {
        let dir = TempDir::new().or_abort("tempdir");
        let state_dir = dir.path().join(".git").join("factor");
        fs::create_dir_all(&state_dir).or_abort("create factor dir");
        fs::write(state_dir.join("split_count"), "0\n").or_abort("write split_count");
        fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");

        let io = Box::leak(Box::new(TestIo::default()));
        let env = Box::leak(Box::new(TestEnv));
        let fs = &REAL_FS;
        let runner = Box::leak(Box::new(TestRunner));
        let ctx = Ctx {
            cwd: dir.path().to_path_buf(),
            env,
            fs,
            io,
            runner,
        };

        let err = Session::with_state(&ctx, StateDir::new(state_dir.clone()), vec![])
            .advance_to_next_commit()
            .err_or_abort("zero final split count should fail");
        assert_eq!(
            err.to_string(),
            "git command failed: split_count is zero at completion"
        );
        assert!(
            !state_dir.exists(),
            "state dir should be removed before failure"
        );
    }

    #[test]
    fn cmd_start_in_propagates_invalid_range_error_after_prep() {
        let dir = TempDir::new().or_abort("tempdir");
        let repo = dir.path();

        let io = Box::leak(Box::new(TestIo::default()));
        let env = Box::leak(Box::new(TestEnv));
        let fs = &REAL_FS;
        let runner = Box::leak(Box::new(ScriptedRunner::new(
            vec![
                Ok(Output {
                    status: success_status(),
                    stdout: b".git\n".to_vec(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: b".git\n".to_vec(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: b"bad\n".to_vec(),
                    stderr: Vec::new(),
                }),
            ],
            Vec::new(),
        )));
        let ctx = Ctx {
            cwd: repo.to_path_buf(),
            env,
            fs,
            io,
            runner,
        };
        let exec =
            NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("non-empty exec"));
        let commit_refs = NonEmpty::new(
            NonEmptyString::try_from("HEAD~1..HEAD".to_owned()).or_abort("non-empty commit ref"),
        );

        let err = cmd_start_in(&ctx, &exec, &commit_refs)
            .err_or_abort("commit resolution should fail after prep");
        assert_eq!(err.to_string(), "invalid commit: HEAD~1..HEAD");
    }

    #[test]
    fn cmd_start_in_propagates_invalid_inclusive_span_after_resolution() {
        let dir = TempDir::new().or_abort("tempdir");
        let repo = dir.path();
        let resolved = "a".repeat(COMMIT_SHA_HEX_LEN);
        let other = "b".repeat(COMMIT_SHA_HEX_LEN);

        let io = Box::leak(Box::new(TestIo::default()));
        let env = Box::leak(Box::new(TestEnv));
        let fs = &REAL_FS;
        let runner = Box::leak(Box::new(ScriptedRunner::new(
            vec![
                Ok(Output {
                    status: success_status(),
                    stdout: b".git\n".to_vec(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: b".git\n".to_vec(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: format!("{resolved}\n").into_bytes(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: format!("{other}\n").into_bytes(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                }),
            ],
            Vec::new(),
        )));
        let ctx = Ctx {
            cwd: repo.to_path_buf(),
            env,
            fs,
            io,
            runner,
        };
        let exec =
            NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("non-empty exec"));
        let commit_refs = NonEmpty {
            head: NonEmptyString::try_from("HEAD~1".to_owned()).or_abort("non-empty ref"),
            tail: vec![NonEmptyString::try_from("HEAD".to_owned()).or_abort("non-empty ref")],
        };

        let err =
            cmd_start_in(&ctx, &exec, &commit_refs).err_or_abort("inclusive span should fail");
        assert_eq!(
            err.to_string(),
            "invalid commit: HEAD~1 HEAD (range must resolve to a contiguous ancestry span)"
        );
    }

    #[test]
    fn validate_split_target_returns_not_ancestor_when_merge_base_fails() {
        let io = Box::leak(Box::new(TestIo::default()));
        let env = Box::leak(Box::new(TestEnv));
        let fs = Box::leak(Box::new(TestFs));
        let runner = Box::leak(Box::new(ScriptedRunner::new(
            Vec::new(),
            vec![Ok(failure_status())],
        )));
        let ctx = Ctx {
            cwd: PathBuf::from("."),
            env,
            fs,
            io,
            runner,
        };
        let sha = CommitSha::new("0123456789abcdef0123456789abcdef01234567".to_owned())
            .or_abort("valid sha");

        let err = validate_split_target_in(&ctx, &sha).err_or_abort("non-ancestor should fail");
        assert_eq!(
            err.to_string(),
            format!("commit {sha} is not an ancestor of HEAD")
        );
        assert_eq!(runner.status_calls.borrow().len(), 1);
    }

    #[test]
    fn validate_split_target_accepts_non_merge_ancestor_commit() {
        let io = Box::leak(Box::new(TestIo::default()));
        let env = Box::leak(Box::new(TestEnv));
        let fs = Box::leak(Box::new(TestFs));
        let runner = Box::leak(Box::new(ScriptedRunner::new(
            Vec::new(),
            vec![Ok(success_status())],
        )));
        let ctx = Ctx {
            cwd: PathBuf::from("."),
            env,
            fs,
            io,
            runner,
        };
        let sha = CommitSha::new("0123456789abcdef0123456789abcdef01234567".to_owned())
            .or_abort("valid sha");

        validate_split_target_in(&ctx, &sha).or_abort("ancestor should be accepted");
        assert_eq!(
            runner.status_calls.borrow().as_slice(),
            &[vec![
                "merge-base".to_owned(),
                "--is-ancestor".to_owned(),
                sha.to_string(),
                "HEAD".to_owned(),
            ]]
        );
    }

    #[test]
    fn advance_to_next_commit_enters_empty_root_cleanup_for_root_sessions() {
        let io = Box::leak(Box::new(TestIo::default()));
        let env = Box::leak(Box::new(TestEnv));
        let fs = &REAL_FS;
        let runner = Box::leak(Box::new(TestRunner));
        let state_dir = env::temp_dir().join(format!(
            "git-factor-root-cleanup-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .or_abort("current time should be >= unix epoch")
                .as_nanos(),
        ));
        fs::create_dir_all(&state_dir).or_abort("create temp state dir");
        fs::write(state_dir.join("split_count"), "1\n").or_abort("write split_count state");
        fs::write(state_dir.join("requires_rebase"), "false\n")
            .or_abort("write requires_rebase state");
        fs::write(state_dir.join("is_root"), "true\n").or_abort("write is_root state");
        let ctx = Ctx {
            cwd: PathBuf::from("."),
            env,
            fs,
            io,
            runner,
        };

        let err = Session::with_state(&ctx, StateDir::new(state_dir.clone()), vec![])
            .advance_to_next_commit()
            .err_or_abort("root cleanup should invoke git operations");
        let err_text = err.to_string();
        assert!(
            err_text.contains("unused in parser-only tests"),
            "unexpected error"
        );
        let _ignored = fs::remove_dir_all(&state_dir);
    }

    #[test]
    fn advance_to_next_commit_completes_root_cleanup_when_root_is_non_empty() {
        let io = Box::leak(Box::new(TestIo::default()));
        let env = Box::leak(Box::new(TestEnv));
        let fs = &REAL_FS;
        let runner = Box::leak(Box::new(ScriptedRunner::new(
            vec![
                Ok(Output {
                    status: success_status(),
                    stdout: b"0123456789abcdef0123456789abcdef01234567\n".to_vec(),
                    stderr: Vec::new(),
                }),
                Ok(Output {
                    status: success_status(),
                    stdout: b"100644 blob deadbeef\tREADME.md\n".to_vec(),
                    stderr: Vec::new(),
                }),
            ],
            Vec::new(),
        )));
        let state_dir = env::temp_dir().join(format!(
            "git-factor-root-cleanup-success-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .or_abort("current time should be >= unix epoch")
                .as_nanos(),
        ));
        fs::create_dir_all(&state_dir).or_abort("create temp state dir");
        fs::write(state_dir.join("split_count"), "1\n").or_abort("write split_count state");
        fs::write(state_dir.join("requires_rebase"), "false\n")
            .or_abort("write requires_rebase state");
        fs::write(state_dir.join("is_root"), "true\n").or_abort("write is_root state");
        let ctx = Ctx {
            cwd: PathBuf::from("."),
            env,
            fs,
            io,
            runner,
        };

        let outcome = Session::with_state(&ctx, StateDir::new(state_dir.clone()), vec![])
            .advance_to_next_commit()
            .or_abort("root cleanup should complete when root has tree entries");

        let final_split_count = completed_split_count(&outcome);
        assert_eq!(final_split_count.get(), 1);
        assert!(!state_dir.exists(), "state dir should be removed");
        let _ignored = fs::remove_dir_all(&state_dir);
    }

    #[test]
    fn advance_to_next_commit_completes_without_root_cleanup_when_not_root() {
        let io = Box::leak(Box::new(TestIo::default()));
        let env = Box::leak(Box::new(TestEnv));
        let fs = &REAL_FS;
        let runner = Box::leak(Box::new(ScriptedRunner::new(Vec::new(), Vec::new())));
        let state_dir = env::temp_dir().join(format!(
            "git-factor-non-root-complete-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .or_abort("current time should be >= unix epoch")
                .as_nanos(),
        ));
        fs::create_dir_all(&state_dir).or_abort("create temp state dir");
        fs::write(state_dir.join("split_count"), "1\n").or_abort("write split_count state");
        fs::write(state_dir.join("requires_rebase"), "false\n")
            .or_abort("write requires_rebase state");
        let ctx = Ctx {
            cwd: PathBuf::from("."),
            env,
            fs,
            io,
            runner,
        };

        let outcome = Session::with_state(&ctx, StateDir::new(state_dir.clone()), vec![])
            .advance_to_next_commit()
            .or_abort("non-root completion should succeed without root cleanup");

        let final_split_count = completed_split_count(&outcome);
        assert_eq!(final_split_count.get(), 1);
        assert!(!state_dir.exists(), "state dir should be removed");
        let _ignored = fs::remove_dir_all(&state_dir);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_fs_delegates_non_canonicalize_operations() {
        let fs = NonUtf8Fs;
        let path = Path::new("ignored");

        fs.create_dir_all(path)
            .or_abort("create_dir_all should succeed");
        assert!(!fs.exists(path));
        assert!(!fs.is_dir(path));
        let read_err = fs
            .read_to_string(path)
            .err_or_abort("read_to_string should return not found");
        assert_eq!(read_err.kind(), io::ErrorKind::NotFound);
        fs.remove_dir_all(path)
            .or_abort("remove_dir_all should succeed");
        fs.remove_file(path).or_abort("remove_file should succeed");
        fs.write_string(path, "hello")
            .or_abort("write_string should succeed");
    }
}

#[cfg(test)]
mod tests;
