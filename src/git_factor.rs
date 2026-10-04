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
        reason = "test-target Clippy applies the module-file style lint to the crate entry"
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
/// Isolated staged-candidate validation.
#[path = "git_factor/candidate.rs"]
mod candidate;
/// CLI argument model for `git-factor`.
#[path = "git_factor/cli.rs"]
mod cli;
/// Execution context, IO traits, and real implementations.
#[path = "git_factor/ctx.rs"]
mod ctx;
/// Durable completed-round checkpoint orchestration.
#[path = "git_factor/engine.rs"]
mod engine;
/// Error model for `git-factor`.
#[path = "git_factor/error.rs"]
mod error;
/// Tree-bound ordered gate verification.
#[path = "git_factor/gate.rs"]
mod gate;
/// Git command execution and process management.
#[path = "git_factor/git.rs"]
mod git;
/// Shared utility helpers for `git-factor`.
#[path = "git_factor/helpers.rs"]
mod helpers;
/// Serialized command results.
#[path = "git_factor/output.rs"]
mod output;
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

use std::ffi::OsString;
use std::io;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

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
use clap::CommandFactory as _;
use nonempty::NonEmpty;

use self::cli::Cli;
use self::error::{FactorError, non_empty_msg};
use self::git::{
    REBASE_APPLY_DIR, REBASE_MERGE_DIR, command_output_with, command_status_with, git_dir_in,
    git_output, git_raw_output, run_git,
};
use self::helpers::{error_to_exit, shell_quote, status_code};
#[cfg(test)]
use self::types::COMMIT_SHA_HEX_LEN;
use self::types::{BaseParent, CommitSha, CommitSpan, StateDir, TreeHash};
use self::ui::is_mid_rebase_in;
use self::validation::{resolve_commit_span, validate_exec_syntax};

#[cfg(test)]
use crate::test_support::{OrAbort as _, ResultOrAbort as _};

/// Formats nonempty captured streams with diagnostic labels and complete lines.
#[expect(
    clippy::single_call_fn,
    reason = "status diagnostics retain one named stream-formatting boundary with exact status observers"
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

/// Decodes captured output and error with replacement for invalid UTF-8 bytes.
fn output_text(output: &Output) -> (String, String) {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (stdout, stderr)
}

/// Observes porcelain status and refuses failed commands or unexpected standard error.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "status observation retains its independently exercised output and stderr admission boundary"
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

/// Reads the selected commit message for a remainder that reuses its original metadata.
#[expect(
    clippy::single_call_fn,
    reason = "remainder preparation retains a named metadata query instead of embedding Git arguments in the lifecycle"
)]
fn commit_message(ctx: &Ctx<'_>, sha: &CommitSha) -> Result<String, FactorError> {
    git_output(ctx, &["show", "--format=%B", "--no-patch", sha.as_str()])
}

/// Runs the actual checkpoint command-line interface.
#[inline]
#[must_use]
pub fn main_entry() -> i32 {
    use std::env;
    let args = env::args_os().collect::<Vec<OsString>>();
    main_entry_with_vec(&REAL_IO, build_ctx_from_cwd(REAL_ENV.current_dir()), &args)
}

/// Builds real capabilities from the working directory or preserves its resolution error.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "context construction retains an independently exercised working-directory failure boundary"
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

/// Runs the supplied CLI arguments or reports a context construction failure.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "injected CLI entry retains the same dispatch boundary for native and direct contracts"
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

/// Returns whether porcelain output contains only empty lines.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "the named porcelain admission predicate has direct changed-status contracts"
    )
)]
fn repo_status_is_clean(stdout: &str) -> bool {
    stdout.lines().all(str::is_empty)
}

/// Refuses changed porcelain status while preserving it in the requested diagnostic.
fn ensure_repo_clean(ctx: &Ctx<'_>, message: &str) -> Result<(), FactorError> {
    let stdout = repo_status_stdout(ctx)?;
    if repo_status_is_clean(stdout.as_str()) {
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

/// Runs CLI dispatch and reports errors with admitted session diagnostics.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "error reporting remains a named boundary exercised through injected CLI contracts"
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

/// Dispatches checkpoint callbacks or parses and runs the public CLI command.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "argument parsing and callback dispatch remain one named independently exercised boundary"
    )
)]
fn run_with_args_vec(ctx: &Ctx<'_>, args: Vec<OsString>) -> Result<i32, FactorError> {
    if let Some(internal) = args.get(1..).filter(|arguments| {
        arguments
            .first()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.starts_with("checkpoint-"))
    }) {
        return engine::entry(ctx, internal);
    }
    let matches = match Cli::command().try_get_matches_from(args) {
        Ok(matches) => matches,
        Err(error) => {
            let code = if error.use_stderr() {
                EXIT_USAGE
            } else {
                EXIT_OK
            };
            if error.use_stderr() {
                ctx.err(&error.to_string())?;
            } else {
                ctx.out(&error.to_string())?;
            }
            return Ok(code);
        }
    };
    let cli = <Cli as clap::FromArgMatches>::from_arg_matches(&matches)
        .map_err(|error| FactorError::Usage(non_empty_msg(error.to_string())))?;
    if cli.exec().is_empty()
        && cli.gate().is_empty()
        && cli.commits().is_empty()
        && cli.message().is_empty()
        && !cli.abort()
        && !cli.status()
        && !cli.continue_flag()
        && !cli.retry()
        && !cli.finish()
    {
        ctx.out(&Cli::command().render_long_help().to_string())?;
        return Ok(EXIT_OK);
    }
    engine::run(ctx, &cli, &matches)
}

#[cfg(test)]
#[path = "git_factor/tests.rs"]
mod tests;

/// Preserves existing unexpected-error diagnostics without modifying observational status.
#[expect(
    clippy::single_call_fn,
    reason = "session diagnostics retain a separate authority check so observational status never writes an error log"
)]
fn persist_unexpected_session_error(ctx: &Ctx<'_>, error: &FactorError, args: &[OsString]) {
    if args.iter().skip(1).any(|argument| argument == "--status")
        || !error.should_persist_error_log()
    {
        return;
    }
    if let Some(directory) = engine::error_log_directory(ctx) {
        drop(write_error_log(ctx, &directory, args, error));
    }
}

#[cfg(test)]
mod proptests;
