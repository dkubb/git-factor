//! `git-factor` is a `git` subcommand for splitting a large commit into smaller,
//! atomic commits using interactive rebase.
//!
//! After installing (for example with `cargo install --path .`), run it as:
//! `git factor ...` (git will resolve this to the `git-factor` binary).

#![forbid(unsafe_code)]
#![allow(
    clippy::all,
    clippy::pedantic,
    clippy::restriction,
    clippy::nursery,
    unfulfilled_lint_expectations,
    reason = "Temporary baseline for pre-existing lint debt; tighten in follow-up commits"
)]
#![expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Clippy suggests ordering statics before their type, which is not possible in Rust"
)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};

use crate::exit_codes::{EXIT_DATAERR, EXIT_OK, EXIT_SOFTWARE, EXIT_TEMPFAIL, EXIT_USAGE};
use clap::{CommandFactory as _, Parser as _};
use non_empty_string::NonEmptyString;
use nonempty::NonEmpty;

/// CLI argument model for `git-factor`.
#[path = "git_factor/cli.rs"]
mod cli;
/// Error model for `git-factor`.
#[path = "git_factor/error.rs"]
mod error;
/// Shared git/state/validation helpers for `git-factor`.
#[path = "git_factor/helpers.rs"]
mod helpers;
/// Core domain types for `git-factor`.
#[path = "git_factor/types.rs"]
mod types;

use self::cli::Cli;
use self::error::FactorError;
use self::helpers::{
    command_status_with, editor_path, error_to_exit, factor_dir_in, git_output, git_status,
    head_ref_literal, is_factor_active_in, is_mid_rebase_in, is_root_commit_in,
    mixed_reset_to_empty, print_hints_in, print_session_started, read_state,
    read_state_bool_or_default, read_state_parsed, remove_empty_root_in, resolve_commit,
    resolve_commit_refs, run_git, run_git_non_interactive, shell_quote, sort_topologically,
    status_code, trace_note, validate_ancestor, validate_exec_syntax, validate_not_merge,
    write_state,
};
use self::types::{CommitSha, Commits};

/// Handles all user-facing IO (stdout/stderr) for the CLI.
///
/// Tests can inject a capturing implementation to assert on exact messages.
trait Io {
    /// Writes raw text to stdout.
    fn out(&self, text: &str) -> io::Result<()>;

    /// Writes raw text to stderr.
    fn err(&self, text: &str) -> io::Result<()>;

    /// Writes a line to stdout.
    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }

    /// Writes a line to stderr.
    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(line)?;
        self.err("\n")
    }
}

/// Production [`Io`] implementation.
struct RealIo;

/// Shared `Io` for the real CLI.
static REAL_IO: RealIo = RealIo;

impl Io for RealIo {
    fn out(&self, text: &str) -> io::Result<()> {
        use io::Write as _;
        let mut out = io::stdout().lock();
        out.write_all(text.as_bytes())
    }

    fn err(&self, text: &str) -> io::Result<()> {
        use io::Write as _;
        let mut err = io::stderr().lock();
        err.write_all(text.as_bytes())
    }
}

/// Environment access (current directory, current executable, etc.).
trait Env {
    /// Returns the current working directory.
    fn current_dir(&self) -> io::Result<PathBuf>;

    /// Returns the path of the currently running executable.
    fn current_exe(&self) -> io::Result<PathBuf>;

    /// Returns the value of an environment variable, if present.
    fn var_os(&self, key: &str) -> Option<OsString>;
}

/// Production [`Env`] implementation.
struct RealEnv;

/// Shared `Env` for the real CLI.
static REAL_ENV: RealEnv = RealEnv;

impl Env for RealEnv {
    fn current_dir(&self) -> io::Result<PathBuf> {
        env::current_dir()
    }

    fn current_exe(&self) -> io::Result<PathBuf> {
        env::current_exe()
    }

    fn var_os(&self, key: &str) -> Option<OsString> {
        env::var_os(key)
    }
}

/// Filesystem access.
trait Fs {
    /// Creates a directory and all missing parent components.
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;

    /// Removes a directory tree.
    fn remove_dir_all(&self, path: &Path) -> io::Result<()>;

    /// Removes a file.
    fn remove_file(&self, path: &Path) -> io::Result<()>;

    /// Reads a UTF-8 text file into a string.
    fn read_to_string(&self, path: &Path) -> io::Result<String>;

    /// Writes a UTF-8 text file from a string.
    fn write_string(&self, path: &Path, content: &str) -> io::Result<()>;

    /// Canonicalizes a path.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;

    /// Returns true when the path exists and is a directory.
    fn is_dir(&self, path: &Path) -> bool;

    /// Returns true when the path exists.
    fn exists(&self, path: &Path) -> bool;
}

/// Production [`Fs`] implementation.
struct RealFs;

/// Shared `Fs` for the real CLI.
static REAL_FS: RealFs = RealFs;

impl Fs for RealFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        fs::write(path, content)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        fs::canonicalize(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

/// Runs external processes (git, bash, etc.).
///
/// Production uses [`RealRunner`]. Tests can inject a fake runner to force
/// specific internal behaviors without relying on environment variables.
trait Runner {
    /// Runs a process and returns its captured output.
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output>;

    /// Runs a process and returns its exit status.
    ///
    /// `quiet=true` discards stdout/stderr to avoid noisy subprocess output.
    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus>;
}

/// Production [`Runner`] implementation.
struct RealRunner;

/// Shared `Runner` for the real CLI.
static REAL_RUNNER: RealRunner = RealRunner;

impl Runner for RealRunner {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        Command::new(bin).args(args).current_dir(cwd).output()
    }

    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        let mut command = Command::new(bin);
        command.args(args).current_dir(cwd);
        for &(key, value) in envs {
            command.env(key, value);
        }
        if quiet {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }
        command.status()
    }
}

/// Execution context for git-factor operations.
#[derive(Clone)]
struct Ctx<'runner> {
    /// Working directory for command execution.
    cwd: PathBuf,
    /// Command runner implementation (real or test double).
    runner: &'runner dyn Runner,
    /// User-facing IO (stdout/stderr).
    io: &'runner dyn Io,
    /// Environment access.
    env: &'runner dyn Env,
    /// Filesystem access.
    fs: &'runner dyn Fs,
}

impl Ctx<'_> {
    /// Writes raw text to stdout.
    fn out(&self, text: &str) -> Result<(), FactorError> {
        self.io.out(text).map_err(FactorError::Io)
    }

    /// Writes a line to stdout.
    fn outln(&self, line: &str) -> Result<(), FactorError> {
        self.io.outln(line).map_err(FactorError::Io)
    }

    /// Writes raw text to stderr.
    fn err(&self, text: &str) -> Result<(), FactorError> {
        self.io.err(text).map_err(FactorError::Io)
    }

    /// Writes a line to stderr.
    fn errln(&self, line: &str) -> Result<(), FactorError> {
        self.io.errln(line).map_err(FactorError::Io)
    }
}

/// Builds a [`Ctx`] using the real runner and the current working directory.
fn ctx_from_parts<'ctx>(
    env: &'ctx dyn Env,
    runner: &'ctx dyn Runner,
    io: &'ctx dyn Io,
    fs: &'ctx dyn Fs,
) -> Result<Ctx<'ctx>, FactorError> {
    let cwd = env
        .current_dir()
        .map_err(|err| FactorError::GitCommand(format!("cannot resolve cwd: {err}")))?;
    Ok(Ctx {
        cwd,
        runner,
        io,
        env,
        fs,
    })
}

/// Builds a [`Ctx`] using the real implementations.
fn real_ctx() -> Result<Ctx<'static>, FactorError> {
    ctx_from_parts(&REAL_ENV, &REAL_RUNNER, &REAL_IO, &REAL_FS)
}

/// Aborts the current factor session.
#[expect(
    clippy::single_call_fn,
    reason = "Command handler dispatched from run()"
)]
fn cmd_abort_in(ctx: &Ctx<'_>) -> Result<i32, FactorError> {
    trace_note(ctx, "factor_cmd_abort", &[]);

    if !is_factor_active_in(ctx) {
        return Err(FactorError::NoActiveSession);
    }

    let state_dir = factor_dir_in(ctx)?;
    let fallback_requires_rebase =
        read_state_bool_or_default(ctx, &state_dir, "requires_rebase", false)?;
    let started_rebase =
        read_state_bool_or_default(ctx, &state_dir, "started_rebase", fallback_requires_rebase)?;
    if started_rebase && is_mid_rebase_in(ctx) {
        run_git_non_interactive(ctx, &["rebase", "--abort"])?;
    }
    let reset_target = match read_state(ctx, &state_dir, "start_head") {
        Ok(start_head) => start_head.to_string(),
        Err(FactorError::StateRead(err)) if err.kind() == io::ErrorKind::NotFound => {
            current_commit_from_state(ctx, &state_dir)?.to_string()
        }
        Err(err) => return Err(err),
    };
    run_git(ctx, &["reset", "--hard", "--quiet", reset_target.as_str()])?;
    run_git(ctx, &["clean", "--force", "--quiet", "-d"])?;
    drop(ctx.fs.remove_dir_all(&state_dir));

    ctx.outln("FACTOR: Session aborted for current commit step.")?;
    if is_mid_rebase_in(ctx) {
        ctx.outln("FACTOR: Rebase still active. To abort full rebase, run: git rebase --abort")?;
    }

    Ok(EXIT_OK)
}

/// Shows status for the current factor session.
#[expect(
    clippy::single_call_fn,
    reason = "Command handler dispatched from run()"
)]
fn cmd_status_in(ctx: &Ctx<'_>) -> Result<i32, FactorError> {
    trace_note(ctx, "factor_cmd_status", &[]);

    if !is_factor_active_in(ctx) {
        ctx.outln("FACTOR: No active session.")?;
        return Ok(EXIT_OK);
    }

    let state_dir = factor_dir_in(ctx)?;
    let current_commit = current_commit_from_state(ctx, &state_dir)?;
    let current_index = read_state_parsed::<usize>(ctx, &state_dir, "current_index")?;
    let split_count = read_state_parsed::<u32>(ctx, &state_dir, "split_count")?;
    let requires_rebase = read_state_bool_or_default(ctx, &state_dir, "requires_rebase", true)?;
    let is_root = read_state_bool_or_default(ctx, &state_dir, "is_root", false)?;
    let rebase_in_progress = is_mid_rebase_in(ctx);

    ctx.outln("FACTOR: Active session.")?;
    ctx.outln(&format!("CURRENT_COMMIT: {current_commit}"))?;
    ctx.outln(&format!("CURRENT_INDEX: {current_index}"))?;
    ctx.outln(&format!("SPLIT_COUNT: {split_count}"))?;
    ctx.outln(&format!("REQUIRES_REBASE: {requires_rebase}"))?;
    ctx.outln(&format!("REBASE_IN_PROGRESS: {rebase_in_progress}"))?;
    ctx.outln(&format!("IS_ROOT: {is_root}"))?;

    Ok(EXIT_OK)
}

fn increment_split_count_in_state(ctx: &Ctx<'_>, state_dir: &Path) -> Result<u32, FactorError> {
    let split_count = read_state_parsed::<u32>(ctx, state_dir, "split_count")?
        .checked_add(1)
        .ok_or(FactorError::GitCommand("split_count overflow".to_owned()))?;
    write_state(ctx, state_dir, "split_count", &split_count.to_string())?;
    Ok(split_count)
}

fn capture_expected_tree_in_state(ctx: &Ctx<'_>, state_dir: &Path) -> Result<(), FactorError> {
    let expected_tree = git_output(ctx, &["rev-parse", "HEAD^{tree}"])?;
    write_state(ctx, state_dir, "expected_tree", &expected_tree)?;
    Ok(())
}

fn write_state_pairs(
    ctx: &Ctx<'_>,
    state_dir: &Path,
    pairs: &[(&str, &str)],
) -> Result<(), FactorError> {
    for (name, value) in pairs {
        write_state(ctx, state_dir, name, value)?;
    }
    Ok(())
}

/// Advances to the next commit in a multi-commit factor session.
///
/// Called after completing all splits for the current commit. Continues the
/// rebase and checks if another edit stop was reached (next commit to split)
/// or if the rebase finished completely.
///
/// Returns `Ok(true)` if another commit is ready to split, `Ok(false)` if done.
fn advance_to_next_commit_in(ctx: &Ctx<'_>, state_dir: &Path) -> Result<bool, FactorError> {
    let split_count = read_state_parsed::<u32>(ctx, state_dir, "split_count")?;
    let requires_rebase = read_state_bool_or_default(ctx, state_dir, "requires_rebase", true)?;

    if requires_rebase {
        if !is_mid_rebase_in(ctx) {
            return Err(FactorError::GitCommand("no rebase in progress".to_owned()));
        }

        run_git_non_interactive(ctx, &["rebase", "--continue"])?;

        if is_mid_rebase_in(ctx) {
            // Capture the rewritten commit tree for the next edit stop before
            // resetting to unstage its diff for splitting.
            capture_expected_tree_in_state(ctx, state_dir)?;

            // Another edit stop reached: advance to the next commit.
            let current_index = read_state_parsed::<usize>(ctx, state_dir, "current_index")?
                .checked_add(1)
                .ok_or(FactorError::GitCommand("current_index overflow".to_owned()))?;
            let current_index = current_index.to_string();
            write_state_pairs(
                ctx,
                state_dir,
                &[
                    ("current_index", current_index.as_str()),
                    ("split_count", "0"),
                ],
            )?;

            run_git(ctx, &["reset", "--quiet", "HEAD~1"])?;

            let current_commit = current_commit_from_state(ctx, state_dir)?;
            let message = commit_message(ctx, &current_commit)?;
            let stat_output = git_output(ctx, &["diff", "--stat"])?;
            let untracked_output =
                git_output(ctx, &["ls-files", "--others", "--exclude-standard"])?;
            let short_sha = git_output(ctx, &["rev-parse", "--short", current_commit.as_str()])?;

            ctx.outln(&format!(
                "FACTOR: Previous commit split into {split_count} commits."
            ))?;
            ctx.outln(&format!("FACTOR: Now splitting {short_sha}."))?;
            ctx.outln(&format!("ORIGINAL MESSAGE: {message}"))?;
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
            ctx.outln("NEXT: Stage changes for the next commit, then run:")?;
            ctx.outln("  git factor --continue --message \"type: description\"")?;

            ctx.out("\n")?;
            print_hints_in(ctx)?;

            return Ok(true);
        }
    }

    // Session finished (either rebase is done, or single-commit no-rebase mode).
    let is_root = read_state(ctx, state_dir, "is_root").is_ok_and(|value| value.as_str() == "true");
    drop(ctx.fs.remove_dir_all(state_dir));

    if is_root {
        remove_empty_root_in(ctx)?;
    }

    ctx.outln(&format!(
        "FACTOR: Complete. Final commit split into {split_count} commits."
    ))?;

    Ok(false)
}

/// Continues an in-progress factor session.
fn cmd_continue_in(ctx: &Ctx<'_>, messages: &NonEmpty<NonEmptyString>) -> Result<i32, FactorError> {
    trace_note(ctx, "factor_cmd_continue", &[]);

    if !is_factor_active_in(ctx) {
        return Err(FactorError::NoActiveSession);
    }
    let state_dir = factor_dir_in(ctx)?;
    let requires_rebase = read_state_bool_or_default(ctx, &state_dir, "requires_rebase", true)?;
    if requires_rebase && !is_mid_rebase_in(ctx) {
        return Err(FactorError::GitCommand("no rebase in progress".to_owned()));
    }
    let original_commit = current_commit_from_state(ctx, &state_dir)?;
    let exec_command = read_state(ctx, &state_dir, "exec")?;

    // Check for staged changes before mutating the working tree. This avoids
    // deleting the remaining unstaged/untracked pool when nothing is staged.
    let has_staged = git_status(ctx, &["diff", "--quiet", "--staged"])?;

    if has_staged.success() {
        return Err(FactorError::NoStagedChanges);
    }

    // Clean unstaged/untracked changes.
    run_git(ctx, &["checkout", "--quiet", "--", "."])?;
    run_git(ctx, &["clean", "--force", "--quiet", "-d"])?;

    // Materialize the staged slice into the working tree so the exec gate
    // validates the staged/index state (not HEAD).
    materialize_index_to_worktree(ctx)?;

    // Run exec gate on the clean working tree before committing.
    let exec_status = command_status_with(ctx, "bash", &["-c", exec_command.as_str()], &[], false)?;

    if !exec_status.success() {
        // Restore the remaining pool from the original commit while preserving
        // the user's staged slice so they can stage more and retry.
        rehydrate_pool_preserving_index(ctx, &original_commit)?;

        let code = status_code(exec_status);
        ctx.outln("FACTOR: Exec gate failed. No commit created.")?;
        ctx.outln(&format!("EXEC: {exec_command}"))?;
        ctx.outln(&format!("CODE: {code}"))?;
        ctx.out("\n")?;
        ctx.outln("NEXT: Adjust staged changes so the exec gate passes, then retry:")?;
        ctx.outln("  git factor --continue --message \"type: description\"")?;
        return Err(FactorError::ExecFailed {
            code,
            command: exec_command,
        });
    }

    // Create the commit preserving original metadata (exec gate passed).
    git_commit_preserving_metadata(ctx, &original_commit, messages, false)?;

    // Increment split count.
    let split_count = increment_split_count_in_state(ctx, &state_dir)?;

    let head_tree = git_output(ctx, &["rev-parse", "HEAD^{tree}"])?;
    let expected_tree = expected_tree_for_current_step(ctx, &state_dir, &original_commit)?;
    let converged = if head_tree == expected_tree {
        "true"
    } else {
        "false"
    };
    trace_note(
        ctx,
        "tree_compare_continue",
        &[
            ("expected_tree", expected_tree.as_str()),
            ("actual_tree", head_tree.as_str()),
            ("converged", converged),
        ],
    );

    if head_tree == expected_tree {
        // No more changes remain for this commit; advance to next or finish.
        advance_to_next_commit_in(ctx, &state_dir)?;
    } else {
        // More changes remain. Rehydrate the remaining pool from the original
        // commit directly into index/worktree, then unstage to keep the usual
        // "remaining changes are unstaged" workflow.
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
        )?;

        let restored_tree = git_output(ctx, &["write-tree"])?;
        if restored_tree != expected_tree {
            return Err(FactorError::TreeHashMismatch {
                actual: restored_tree,
                expected: expected_tree,
            });
        }

        run_git(ctx, &["reset", "--quiet"])?;
        let stat_output = git_output(ctx, &["diff", "--stat"])?;
        let untracked_output = git_output(ctx, &["ls-files", "--others", "--exclude-standard"])?;

        ctx.outln(&format!("FACTOR: Split {split_count} committed."))?;
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
        ctx.outln("NEXT: Stage changes for the next commit, then run:")?;
        ctx.outln("  git factor --continue --message \"type: description\"")?;

        ctx.out("\n")?;
        print_hints_in(ctx)?;
    }

    Ok(EXIT_OK)
}

/// Rehydrates the full original commit into the working tree while restoring
/// the index back to its prior state. This is used on exec-gate failure so the
/// user can adjust the staged slice without losing the remaining pool.
fn rehydrate_pool_preserving_index(
    ctx: &Ctx<'_>,
    original_commit: &CommitSha,
) -> Result<(), FactorError> {
    let idx_tree = git_output(ctx, &["write-tree"])?;

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
        if !unmerged.is_empty() {
            return Err(FactorError::GitCommand(format!(
                "rehydrate cherry-pick left conflicts:\n{unmerged}"
            )));
        }
    }

    let quit_status = git_status(ctx, &["cherry-pick", "--quit"])?;
    if !quit_status.success() {
        return Err(FactorError::GitCommand(format!(
            "git cherry-pick --quit failed (exit {})",
            status_code(quit_status)
        )));
    }

    let read_tree_status = git_status(ctx, &["read-tree", idx_tree.as_str()])?;
    if !read_tree_status.success() {
        return Err(FactorError::GitCommand(format!(
            "git read-tree failed (exit {})",
            status_code(read_tree_status)
        )));
    }

    Ok(())
}

/// Writes the current index to the working tree (best-effort exact match).
///
/// Git exec gates run on the filesystem, not directly on the index. We clean the
/// worktree first (removing unstaged/untracked changes), then write the index
/// contents back out so the gate validates the staged slice.
fn materialize_index_to_worktree(ctx: &Ctx<'_>) -> Result<(), FactorError> {
    run_git(ctx, &["checkout-index", "--all", "--force", "--quiet"])?;

    // checkout-index does not remove paths deleted in the index, so we must
    // explicitly remove any staged deletions to avoid validating extra files.
    let deleted = git_output(ctx, &["diff", "--diff-filter=D", "--name-only", "--staged"])?;
    for path in deleted.lines().map(str::trim).filter(|p| !p.is_empty()) {
        // `git diff --name-only` returns paths, not directories. Removing the file
        // is sufficient to ensure the worktree matches the staged deletion.
        drop(ctx.fs.remove_file(Path::new(path)));
    }

    Ok(())
}

/// Finishes the factor session by committing all remaining staged changes.
fn cmd_finish_in(ctx: &Ctx<'_>, messages: &[NonEmptyString]) -> Result<i32, FactorError> {
    trace_note(ctx, "factor_cmd_finish", &[]);

    if !is_factor_active_in(ctx) {
        return Err(FactorError::NoActiveSession);
    }
    let state_dir = factor_dir_in(ctx)?;
    let requires_rebase = read_state_bool_or_default(ctx, &state_dir, "requires_rebase", true)?;
    if requires_rebase && !is_mid_rebase_in(ctx) {
        return Err(FactorError::GitCommand("no rebase in progress".to_owned()));
    }
    let original_commit = current_commit_from_state(ctx, &state_dir)?;
    let exec_command = read_state(ctx, &state_dir, "exec")?;
    let expected_tree = expected_tree_for_current_step(ctx, &state_dir, &original_commit)?;

    // Clean unstaged/untracked changes.
    run_git(ctx, &["checkout", "--quiet", "--", "."])?;
    run_git(ctx, &["clean", "--force", "--quiet", "-d"])?;

    // Cherry-pick the original commit without committing to stage remaining changes.
    // With --strategy-option theirs, conflicts are auto-resolved in favor of the
    // original commit.
    let cherry_status = git_status(
        ctx,
        &[
            "cherry-pick",
            "--no-commit",
            "--strategy-option",
            "theirs",
            original_commit.as_str(),
        ],
    )?;

    if !cherry_status.success() {
        let unmerged = git_output(ctx, &["diff", "--name-only", "--diff-filter=U"])?;
        if !unmerged.is_empty() {
            drop(command_status_with(
                ctx,
                "git",
                &["cherry-pick", "--abort"],
                &[],
                true,
            ));
            return Err(FactorError::GitCommand(format!(
                "final cherry-pick left conflicts:\n{unmerged}"
            )));
        }
    }

    // Clear sequencer state while keeping the index/worktree changes.
    run_git(ctx, &["cherry-pick", "--quit"])?;

    // Resolve effective messages: use original commit message when none provided.
    let effective_messages: NonEmpty<NonEmptyString> = if messages.is_empty() {
        let original_msg = commit_message(ctx, &original_commit)?;
        NonEmpty::new(
            NonEmptyString::try_from(original_msg.trim_end().to_owned()).map_err(|_err| {
                FactorError::GitCommand("original commit has empty message".to_owned())
            })?,
        )
    } else {
        let (first, rest) = messages
            .split_first()
            .expect("messages is non-empty in this branch");
        let mut out = NonEmpty::new(first.clone());
        for msg in rest {
            out.push(msg.clone());
        }
        out
    };

    // Verify tree hash matches the original commit before committing.
    let actual_tree = git_output(ctx, &["write-tree"])?;
    let converged = if actual_tree == expected_tree {
        "true"
    } else {
        "false"
    };
    trace_note(
        ctx,
        "tree_compare_finish",
        &[
            ("expected_tree", expected_tree.as_str()),
            ("actual_tree", actual_tree.as_str()),
            ("converged", converged),
        ],
    );
    if actual_tree != expected_tree {
        return Err(FactorError::TreeHashMismatch {
            actual: actual_tree,
            expected: expected_tree,
        });
    }

    // Handle the empty-commit case (or "finish called when nothing remains"):
    // if there are no staged changes, create an empty commit so the rebase edit
    // stop can be satisfied without silently dropping the original commit.
    let has_staged = git_status(ctx, &["diff", "--quiet", "--staged"])?;

    // Run exec gate on the clean working tree before committing.
    let exec_status = command_status_with(ctx, "bash", &["-c", exec_command.as_str()], &[], false)?;

    if !exec_status.success() {
        let code = status_code(exec_status);
        ctx.outln("FACTOR: Exec gate failed on final commit.")?;
        ctx.outln(&format!("EXEC: {exec_command}"))?;
        ctx.outln(&format!("CODE: {code}"))?;
        ctx.out("\n")?;
        ctx.outln("NEXT: Fix the issues, then retry:")?;
        ctx.outln("  git factor --finish --message \"type: description\"")?;
        return Err(FactorError::ExecFailed {
            code,
            command: exec_command,
        });
    }

    // Create the commit preserving original metadata (exec gate passed).
    git_commit_preserving_metadata(
        ctx,
        &original_commit,
        &effective_messages,
        has_staged.success(),
    )?;

    // Update split count and advance to next commit or finish.
    increment_split_count_in_state(ctx, &state_dir)?;
    advance_to_next_commit_in(ctx, &state_dir)?;

    Ok(EXIT_OK)
}

/// Starts a new factor session.
#[expect(
    clippy::single_call_fn,
    reason = "Command handler dispatched from run()"
)]
fn cmd_start_in(
    ctx: &Ctx<'_>,
    exec: &NonEmpty<NonEmptyString>,
    commit_refs: &NonEmpty<NonEmptyString>,
) -> Result<i32, FactorError> {
    trace_note(ctx, "factor_cmd_start", &[]);

    // Probe git dir early to surface NotGitRepo before other checks.
    let state_dir = factor_dir_in(ctx)?;

    if ctx.fs.is_dir(&state_dir) {
        return Err(FactorError::ActiveSession);
    }
    if is_mid_rebase_in(ctx) {
        return Err(FactorError::ActiveRebase);
    }

    let commits = resolve_commit_refs(ctx, commit_refs)?;
    let resolved_commits = sort_topologically(ctx, &commits)?;
    let head_ref = head_ref_literal();
    let head_commit = resolve_commit(ctx, &head_ref)?;

    let base_sha = resolved_commits.first();
    let single_head_session = resolved_commits.len() == 1 && base_sha == &head_commit;

    // Validate all resolved commits.
    for sha in &resolved_commits {
        validate_ancestor(ctx, sha)?;
        validate_not_merge(ctx, sha)?;
    }

    let short_sha = NonEmptyString::try_from(git_output(
        ctx,
        &["rev-parse", "--short", base_sha.as_str()],
    )?)
    .map_err(|_err| FactorError::GitCommand("empty short SHA".to_owned()))?;

    // Extract original commit message for display.
    let message = commit_message(ctx, base_sha)?;
    let exec_combined = exec
        .iter()
        .map(NonEmptyString::as_str)
        .collect::<Vec<&str>>()
        .join(" && ");

    // Validate exec command syntax before starting the session.
    validate_exec_syntax(ctx, &exec_combined)?;

    // In single-commit HEAD mode, validate the gate on the current commit
    // before entering a factor session.
    if single_head_session {
        let exec_status =
            command_status_with(ctx, "bash", &["-c", exec_combined.as_str()], &[], false)?;
        if !exec_status.success() {
            #[expect(
                clippy::expect_used,
                reason = "exec is NonEmpty<NonEmptyString>; join with ' && ' cannot produce empty"
            )]
            let command = NonEmptyString::try_from(exec_combined)
                .expect("joined exec command from non-empty inputs cannot be empty");
            return Err(FactorError::ExecFailed {
                code: status_code(exec_status),
                command,
            });
        }
    }

    // Persist state for the session.
    ctx.fs
        .create_dir_all(&state_dir)
        .map_err(FactorError::StateWrite)?;
    let is_root = is_root_commit_in(ctx, base_sha);
    let requires_rebase = if single_head_session { "false" } else { "true" };
    let started_rebase = if single_head_session { "false" } else { "true" };
    let is_root_value = if is_root { "true" } else { "false" };
    let session_state_pairs = [
        ("current_index", "0"),
        ("exec", exec_combined.as_str()),
        ("split_count", "0"),
        ("requires_rebase", requires_rebase),
        ("started_rebase", started_rebase),
        ("start_head", head_commit.as_str()),
        ("is_root", is_root_value),
    ];
    let commits_content: Vec<&str> = resolved_commits.iter().map(CommitSha::as_str).collect();
    write_state(ctx, &state_dir, "commits", &commits_content.join("\n"))?;
    write_state_pairs(ctx, &state_dir, &session_state_pairs)?;

    if !single_head_session {
        // Build sequence editor command with --edit flags for all commits.
        let editor = editor_path(ctx)?;
        let mut seq_parts: Vec<String> = vec![editor];
        for sha in &resolved_commits {
            let short = git_output(ctx, &["rev-parse", "--short", sha.as_str()])?;
            seq_parts.push("--edit".to_owned());
            seq_parts.push(short);
        }
        let seq_editor = seq_parts
            .iter()
            .map(String::as_str)
            .map(shell_quote)
            .collect::<Vec<String>>()
            .join(" ");

        let parent = format!("{base_sha}^");
        let rebase_args = build_rebase_args(exec, &parent, is_root);

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

        if !status.success() {
            drop(ctx.fs.remove_dir_all(&state_dir));
            return Err(FactorError::GitCommand(format!(
                "git rebase failed (exit {})",
                status_code(status)
            )));
        }
    }

    // Capture the tree of the exact commit currently being split. In
    // interactive rebase sessions this is the rewritten edit-stop commit.
    capture_expected_tree_in_state(ctx, &state_dir)?;

    if is_root {
        mixed_reset_to_empty(ctx)?;
    } else {
        run_git(ctx, &["reset", "--quiet", "HEAD~1"])?;
    }

    print_session_started(ctx, &resolved_commits, &short_sha, &message)?;

    Ok(EXIT_OK)
}

/// Builds deterministic rebase arguments for multi-commit split sessions.
fn build_rebase_args<'arg>(
    exec: &'arg NonEmpty<NonEmptyString>,
    parent: &'arg str,
    is_root: bool,
) -> Vec<&'arg str> {
    let mut rebase_args = vec![
        "rebase",
        "--empty",
        "drop",
        "--interactive",
        "--no-autosquash",
        "--no-autostash",
        "--no-rebase-merges",
        "--no-stat",
        "--quiet",
        "--reschedule-failed-exec",
    ];
    for cmd in exec {
        rebase_args.push("--exec");
        rebase_args.push(cmd.as_str());
    }
    if is_root {
        rebase_args.push("--root");
    } else {
        rebase_args.push(parent);
    }
    rebase_args
}

/// Returns the full commit message for a given commit SHA.
fn commit_message(ctx: &Ctx<'_>, sha: &CommitSha) -> Result<String, FactorError> {
    git_output(ctx, &["show", "--format=%B", "--no-patch", sha.as_str()])
}

/// Returns the current commit SHA from the multi-commit state.
///
/// Reads the `commits` file and `current_index` to determine which commit
/// is currently being split.
fn current_commit_from_state(ctx: &Ctx<'_>, state_dir: &Path) -> Result<CommitSha, FactorError> {
    let commits_raw = read_state(ctx, state_dir, "commits")?;
    let commits: Vec<&str> = commits_raw.as_str().lines().collect();
    let current_index = read_state_parsed::<usize>(ctx, state_dir, "current_index")?;
    let out_of_range = FactorError::GitCommand(format!(
        "commit index {current_index} out of range (have {} commits)",
        commits.len()
    ));
    let sha_str = commits.get(current_index).ok_or(out_of_range)?;

    CommitSha::new((*sha_str).to_owned())
}

/// Returns the expected converged tree for the current split step.
///
/// New sessions persist this as `expected_tree`, derived from the live edit-stop
/// commit (`HEAD^{tree}`), which stays correct after rebase rewrites. For older
/// sessions created before this state key existed, fall back to deriving the
/// tree from the original commit SHA recorded in `commits`.
fn expected_tree_for_current_step(
    ctx: &Ctx<'_>,
    state_dir: &Path,
    original_commit: &CommitSha,
) -> Result<String, FactorError> {
    if let Ok(tree) = read_state(ctx, state_dir, "expected_tree") {
        trace_note(
            ctx,
            "expected_tree_source",
            &[("source", "state"), ("expected_tree", tree.as_str())],
        );
        return Ok(tree.to_string());
    }

    let tree = git_output(ctx, &["rev-parse", &format!("{original_commit}^{{tree}}")])?;
    trace_note(
        ctx,
        "expected_tree_source",
        &[
            ("source", "original_commit"),
            ("original_commit", original_commit.as_str()),
            ("expected_tree", tree.as_str()),
        ],
    );
    Ok(tree)
}

/// Creates a git commit preserving the original author and committer metadata.
#[expect(
    clippy::expect_used,
    reason = "git log format guarantees 6 null-separated fields for a valid commit"
)]
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
    let mut parts = raw.splitn(6, '\0');
    let author_name = parts.next().expect("author name from git log format");
    let author_email = parts.next().expect("author email from git log format");
    let author_date = parts.next().expect("author date from git log format");
    let committer_name = parts.next().expect("committer name from git log format");
    let committer_email = parts.next().expect("committer email from git log format");
    let committer_date = parts.next().expect("committer date from git log format");

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
        Err(FactorError::GitCommand(format!(
            "git commit failed (exit {})",
            status_code(status)
        )))
    }
}

/// Entrypoint for the `git-factor` binary.
///
/// Returns an exit code suitable for `std::process::exit`.
#[inline]
#[must_use]
pub fn main_entry() -> i32 {
    use std::env;

    main_entry_with_vec(
        &REAL_IO,
        real_ctx(),
        env::args_os().collect::<Vec<OsString>>(),
    )
}

/// Runs the CLI entrypoint with a provided `Ctx` build result.
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
///
/// This is the shared implementation for both the real CLI entrypoint and
/// unit tests that assert on exact output.
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

/// Like [`run_and_report_with_args_vec`], but returns structured errors.
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

    // Show help when invoked with no arguments.
    if !cli.abort()
        && !cli.status()
        && !cli.continue_flag()
        && !cli.finish()
        && cli.exec().is_empty()
        && cli.commits().is_empty()
        && cli.message().is_empty()
    {
        let help = Cli::command().render_long_help().to_string();
        ctx.out(&help)?;
        return Ok(EXIT_OK);
    }

    if cli.abort() {
        if cli.status()
            || cli.continue_flag()
            || cli.finish()
            || !cli.exec().is_empty()
            || !cli.commits().is_empty()
        {
            return Err(FactorError::Usage(
                "--abort cannot be combined with other options".to_owned(),
            ));
        }
        return cmd_abort_in(ctx);
    }

    if cli.status() {
        if cli.continue_flag()
            || cli.finish()
            || !cli.exec().is_empty()
            || !cli.commits().is_empty()
            || !cli.message().is_empty()
        {
            return Err(FactorError::Usage(
                "--status cannot be combined with other options".to_owned(),
            ));
        }
        return cmd_status_in(ctx);
    }

    if cli.finish() {
        if cli.continue_flag() || !cli.exec().is_empty() || !cli.commits().is_empty() {
            return Err(FactorError::Usage(
                "--finish cannot be combined with --continue, --exec, or COMMIT".to_owned(),
            ));
        }
        return cmd_finish_in(ctx, cli.message());
    }

    if cli.continue_flag() {
        if !cli.exec().is_empty() || !cli.commits().is_empty() {
            return Err(FactorError::Usage(
                "--continue cannot be combined with --exec or COMMIT".to_owned(),
            ));
        }
        let messages = NonEmpty::from_vec(cli.message().to_vec())
            .ok_or_else(|| FactorError::Usage("--continue requires --message <MSG>".to_owned()))?;
        return cmd_continue_in(ctx, &messages);
    }

    let exec = NonEmpty::from_vec(cli.exec().to_vec()).ok_or_else(|| {
        FactorError::Usage("--exec <COMMAND> is required when starting a factor session".to_owned())
    })?;
    let commits = if cli.commits().is_empty() {
        NonEmpty::new(head_ref_literal())
    } else {
        let mut iter = cli.commits().iter().cloned();
        #[expect(
            clippy::expect_used,
            reason = "branch guarded by is_empty() == false ensures at least one commit ref"
        )]
        let first = iter.next().expect("commit refs are non-empty");
        let mut commits = NonEmpty::new(first);
        for commit in iter {
            commits.push(commit);
        }
        commits
    };

    if !cli.message().is_empty() {
        return Err(FactorError::Usage(
            "--message can only be used with --continue or --finish".to_owned(),
        ));
    }

    cmd_start_in(ctx, &exec, &commits)
}

#[cfg(test)]
mod tests;
