use super::*;
use core::str::FromStr;
use std::collections::{BTreeSet, HashSet};

/// Spawns a command and returns its exit status.
///
/// This is intentionally status-only (not output) to keep error handling and
/// coverage-friendly control flow consistent across all git-factor commands.
pub(super) fn command_status_with(
    ctx: &Ctx<'_>,
    bin: &str,
    args: &[&str],
    envs: &[(&str, &str)],
    quiet: bool,
) -> Result<ExitStatus, FactorError> {
    let first_arg = args.first().copied().unwrap_or("");
    ctx.runner
        .status(bin, args, envs, quiet, &ctx.cwd)
        .map_err(|err| FactorError::GitCommand(format!("{bin} {first_arg}: {err}")))
}

/// Runs `git <args...>` and returns its exit status.
pub(super) fn git_status(ctx: &Ctx<'_>, args: &[&str]) -> Result<ExitStatus, FactorError> {
    command_status_with(ctx, "git", args, &[], false)
}
/// Returns the absolute path to the `.git` directory.
pub(super) fn git_dir_in(ctx: &Ctx<'_>) -> Result<PathBuf, FactorError> {
    let output = ctx
        .runner
        .output("git", &["rev-parse", "--git-dir"], &ctx.cwd)
        .map_err(|error| FactorError::GitDir(error.to_string()))?;

    if !output.status.success() {
        return Err(FactorError::NotGitRepo);
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let git_dir_path = PathBuf::from(path);

    // `git rev-parse --git-dir` can return a relative path (e.g. ".git"). Treat it
    // as relative to the `Ctx` working directory.
    if git_dir_path.is_relative() {
        Ok(ctx.cwd.join(git_dir_path))
    } else {
        Ok(git_dir_path)
    }
}

/// Maps a `FactorError` to an `(exit_code, message)` tuple.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "Matching on reference to enum is idiomatic for borrowed error values"
)]
pub(super) fn error_to_exit(error: &FactorError) -> (i32, String) {
    let code = match error {
        FactorError::ActiveRebase
        | FactorError::ActiveSession
        | FactorError::NoActiveSession
        | FactorError::NoStagedChanges
        | FactorError::Usage(_) => EXIT_USAGE,

        FactorError::GitDir(_)
        | FactorError::InvalidCommit(_)
        | FactorError::InvalidExecSyntax(_)
        | FactorError::MergeCommit(_)
        | FactorError::NotAncestor(_)
        | FactorError::NotGitRepo => EXIT_DATAERR,

        FactorError::ExecFailed { .. } | FactorError::TreeHashMismatch { .. } => EXIT_TEMPFAIL,

        FactorError::GitCommand(_)
        | FactorError::StateRead(_)
        | FactorError::StateWrite(_)
        | FactorError::Io(_) => EXIT_SOFTWARE,
    };
    (code, error.to_string())
}

/// Shell-quotes a single argument using single-quote wrapping.
///
/// This is used when constructing `GIT_SEQUENCE_EDITOR`, which git interprets
/// as a shell command line.
pub(super) fn shell_quote(arg: &str) -> String {
    let mut out = String::with_capacity(arg.len());
    out.push('\'');
    for ch in arg.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Returns the absolute path to `git-sequence-editor`, resolved as a sibling
/// of the current executable.
pub(super) fn editor_path(ctx: &Ctx<'_>) -> Result<String, FactorError> {
    let exe = ctx
        .env
        .current_exe()
        .map_err(|err| FactorError::GitCommand(format!("cannot resolve current exe: {err}")))?;
    let script_path = ctx
        .fs
        .canonicalize(&exe)
        .map_err(|err| FactorError::GitCommand(format!("cannot canonicalize exe: {err}")))?;
    let dir = script_path
        .parent()
        .ok_or_else(|| FactorError::GitCommand("executable has no parent directory".to_owned()))?;
    let editor = dir.join("git-sequence-editor");
    editor
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| FactorError::GitCommand("editor path is not valid UTF-8".to_owned()))
}

/// Returns the path to the factor state directory.
pub(super) fn factor_dir_in(ctx: &Ctx<'_>) -> Result<PathBuf, FactorError> {
    Ok(git_dir_in(ctx)?.join("factor"))
}

/// Runs a git command and returns its trimmed stdout as a `String`.
pub(super) fn git_output(ctx: &Ctx<'_>, args: &[&str]) -> Result<String, FactorError> {
    git_output_with(ctx, "git", args)
}

/// Runs a git command using the provided executable name/path.
pub(super) fn git_output_with(
    ctx: &Ctx<'_>,
    bin: &str,
    args: &[&str],
) -> Result<String, FactorError> {
    let output = ctx.runner.output(bin, args, &ctx.cwd).map_err(|err| {
        FactorError::GitCommand(format!("git {}: {err}", args.first().unwrap_or(&"")))
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(FactorError::GitCommand(stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Returns true if a factor session is currently active.
pub(super) fn is_factor_active_in(ctx: &Ctx<'_>) -> bool {
    factor_dir_in(ctx).is_ok_and(|dir| ctx.fs.is_dir(&dir))
}

/// Like [`is_root_commit`], but uses the provided [`Ctx`] for command execution.
pub(super) fn is_root_commit_in(ctx: &Ctx<'_>, sha: &CommitSha) -> bool {
    command_status_with(
        ctx,
        "git",
        &["rev-parse", "--verify", "--quiet", &format!("{sha}^")],
        &[],
        true,
    )
    .is_ok_and(|status| !status.success())
}

/// Returns true if a rebase is currently in progress.
pub(super) fn is_mid_rebase_in(ctx: &Ctx<'_>) -> bool {
    git_dir_in(ctx).is_ok_and(|dir| {
        ctx.fs.is_dir(&dir.join("rebase-merge")) || ctx.fs.is_dir(&dir.join("rebase-apply"))
    })
}

/// Prints contextual hints to guide the next commit.
///
/// Displays remaining diff size, reference file locations, and recovery
/// instructions after each successful split or session start. When running
/// under Claude Code (`CLAUDECODE=1`), adds LLM-specific guidance.
pub(super) fn print_hints_in(ctx: &Ctx<'_>) -> Result<(), FactorError> {
    let stat = git_output(ctx, &["diff", "--stat"])?;
    let remaining = stat.lines().last().unwrap_or_default().to_owned();

    let toplevel = git_output(ctx, &["rev-parse", "--show-toplevel"])?;
    let rust_ref = Path::new(&toplevel).join("references/rust.md");

    ctx.outln("HINTS:")?;
    ctx.outln("  - Find the ONE smallest addition nothing depends on")?;
    ctx.outln("  - Target 15-30 lines (50 max)")?;
    ctx.outln("  - Message: single concrete action, no \"and\"/\"or\"")?;
    ctx.outln("  - Verify: git log --oneline | wc -l")?;
    ctx.outln("  - NEVER use git commit. ONLY use git factor --continue.")?;
    if !remaining.is_empty() {
        ctx.outln(&format!("  REMAINING: {remaining}"))?;
    }
    if ctx.fs.exists(&rust_ref) {
        ctx.outln(&format!("  REFERENCE: {}", rust_ref.display()))?;
    }
    ctx.outln("  RECOVERY: git factor --abort")?;

    if ctx.env.var_os("CLAUDECODE").is_some() {
        ctx.outln("<claude>")?;
        ctx.outln("- If context is above 50%, pause and ask the user to /compact.")?;
        ctx.outln("- Do NOT stop early. Keep committing until \"Complete\".")?;
        ctx.outln("- Do NOT use git commit directly. ONLY use git-factor --continue.")?;
        ctx.outln("- Each commit MUST pass the exec gate. No shortcuts.")?;
        ctx.outln("</claude>")?;
    }

    Ok(())
}

/// Prints the session-started guide with pending file summary.
pub(super) fn print_session_started(
    ctx: &Ctx<'_>,
    resolved_commits: &NonEmpty<CommitSha>,
    short_sha: &NonEmptyString,
    message: &str,
) -> Result<(), FactorError> {
    let stat_output = git_output(ctx, &["diff", "--stat"])?;
    let untracked_output = git_output(ctx, &["ls-files", "--others", "--exclude-standard"])?;
    let commit_count = resolved_commits.len();

    if commit_count > 1 {
        ctx.outln(&format!(
            "FACTOR: Split session started for {commit_count} commits (first: {short_sha})."
        ))?;
    } else {
        ctx.outln(&format!("FACTOR: Split session started for {short_sha}."))?;
    }
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
    ctx.outln("NEXT: Stage changes for the first atomic commit, then run:")?;
    ctx.outln("  git factor --continue --message \"type: description\"")?;
    ctx.out("\n")?;
    ctx.outln("Run git factor --help for the full workflow guide.")?;

    ctx.out("\n")?;
    print_hints_in(ctx)?;

    Ok(())
}

/// Reads a state file from the factor state directory.
pub(super) fn read_state(
    ctx: &Ctx<'_>,
    state_dir: &Path,
    name: &str,
) -> Result<NonEmptyString, FactorError> {
    let path = state_dir.join(name);
    let content = ctx
        .fs
        .read_to_string(&path)
        .map_err(FactorError::StateRead)?;
    NonEmptyString::try_from(content.trim().to_owned()).map_err(|_err| {
        FactorError::GitCommand(format!("corrupted state file '{name}': file is empty"))
    })
}

/// Reads and parses a numeric state file, returning an error on corruption.
pub(super) fn read_state_parsed<T: FromStr>(
    ctx: &Ctx<'_>,
    state_dir: &Path,
    name: &str,
) -> Result<T, FactorError> {
    let value = read_state(ctx, state_dir, name)?;
    value.as_str().parse::<T>().map_err(|_err| {
        FactorError::GitCommand(format!(
            "corrupted state file '{name}': invalid value '{value}'"
        ))
    })
}

/// Reads a `true`/`false` state value, with a default when the file is missing.
pub(super) fn read_state_bool_or_default(
    ctx: &Ctx<'_>,
    state_dir: &Path,
    name: &str,
    default: bool,
) -> Result<bool, FactorError> {
    match read_state(ctx, state_dir, name) {
        Ok(value) => match value.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(FactorError::GitCommand(format!(
                "corrupted state file '{name}': invalid value '{value}'"
            ))),
        },
        Err(FactorError::StateRead(err)) if err.kind() == io::ErrorKind::NotFound => Ok(default),
        Err(err) => Err(err),
    }
}

/// Removes the empty root commit left by `mixed_reset_to_empty()`.
///
/// After a root-commit factor session completes, the history contains an empty
/// commit at the root. This function rebases `--root --interactive` with a
/// sequence editor that drops the empty commit by SHA.
pub(super) fn remove_empty_root_in(ctx: &Ctx<'_>) -> Result<(), FactorError> {
    let root = git_output(ctx, &["rev-list", "--max-parents=0", "HEAD"])?;

    if !git_output(ctx, &["ls-tree", &root])?.is_empty() {
        return Ok(());
    }

    let short_root = git_output(ctx, &["rev-parse", "--short", &root])?;
    let editor = editor_path(ctx)?;
    let seq_editor = format!(
        "{} {} {}",
        shell_quote(editor.as_str()),
        shell_quote("--drop"),
        shell_quote(short_root.as_str())
    );

    drop(command_status_with(
        ctx,
        "git",
        &["rebase", "--quiet", "--root", "--interactive"],
        &[("GIT_SEQUENCE_EDITOR", &seq_editor)],
        false,
    ));

    Ok(())
}

/// Resets HEAD to an empty commit so all files appear as unstaged additions.
///
/// Used for root commits where `git reset --mixed HEAD~1` is not possible.
#[expect(
    clippy::single_call_fn,
    reason = "Isolates root-commit reset logic for clarity"
)]
pub(super) fn mixed_reset_to_empty(ctx: &Ctx<'_>) -> Result<(), FactorError> {
    /// The well-known SHA-1 hash of an empty tree object in git.
    const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

    let commit_sha = git_output(ctx, &["commit-tree", EMPTY_TREE, "-m", "empty"])?;
    run_git(ctx, &["reset", "--quiet", "--mixed", &commit_sha])
}

/// Resolves a commit reference to a full SHA.
pub(super) fn resolve_commit(
    ctx: &Ctx<'_>,
    commit: &NonEmptyString,
) -> Result<CommitSha, FactorError> {
    let sha = git_output(ctx, &["rev-parse", "--verify", commit.as_str()])
        .map_err(|_git_err| FactorError::InvalidCommit(commit.to_string()))?;
    CommitSha::new(sha)
}

/// Resolves commit refs and ranges into a deduplicated, chronologically
/// ordered list of full SHAs.
///
/// Refs containing `..` are expanded via `git rev-list`. Single refs are
/// resolved via `git rev-parse --verify`. The final list is sorted in
/// chronological order (oldest first) to match the order that interactive
/// rebase will stop at each `edit` commit.
pub(super) fn resolve_commit_refs(
    ctx: &Ctx<'_>,
    refs: &NonEmpty<NonEmptyString>,
) -> Result<Commits, FactorError> {
    let mut set = BTreeSet::new();

    for commit_ref in refs {
        let ref_str = commit_ref.as_str();
        if ref_str.contains("...") {
            return Err(FactorError::InvalidCommit(format!(
                "{ref_str} (symmetric diff '...' is not supported, use '..')"
            )));
        }
        if ref_str.contains("..") {
            // Range: expand via git rev-list.
            let output = git_output(ctx, &["rev-list", ref_str])
                .map_err(|_err| FactorError::InvalidCommit(ref_str.to_owned()))?;
            for line in output.lines() {
                if let Ok(sha) = CommitSha::new(line.to_owned()) {
                    set.insert(sha);
                }
            }
        } else {
            set.insert(resolve_commit(ctx, commit_ref)?);
        }
    }

    Commits::new(set)
}

/// Sorts commits topologically (parent before child) to match rebase stop order.
///
/// Uses a single `git rev-list --topo-order --reverse` call with all target
/// SHAs as tips, then filters the output to only the target commits. This
/// ensures the order matches how interactive rebase processes commits.
#[expect(
    clippy::single_call_fn,
    reason = "Isolates topological sort logic for clarity"
)]
pub(super) fn sort_topologically(
    ctx: &Ctx<'_>,
    commits: &Commits,
) -> Result<NonEmpty<CommitSha>, FactorError> {
    let target_shas: HashSet<&str> = commits.iter().map(CommitSha::as_str).collect();

    let mut args = vec!["rev-list", "--reverse", "--topo-order"];
    args.extend(commits.iter().map(CommitSha::as_str));

    let sorted: Vec<CommitSha> = git_output(ctx, &args)?
        .lines()
        .filter(|line| target_shas.contains(line))
        .filter_map(|line| CommitSha::new(line.to_owned()).ok())
        .collect();

    NonEmpty::from_vec(sorted)
        .ok_or_else(|| FactorError::GitCommand("no commits after sorting".to_owned()))
}

/// Runs a git command and returns success/failure.
pub(super) fn run_git(ctx: &Ctx<'_>, args: &[&str]) -> Result<(), FactorError> {
    run_git_with(ctx, "git", args)
}

/// Runs a git command using the provided executable name/path.
pub(super) fn run_git_with(ctx: &Ctx<'_>, bin: &str, args: &[&str]) -> Result<(), FactorError> {
    let status = ctx
        .runner
        .status(bin, args, &[], false, &ctx.cwd)
        .map_err(|err| {
            FactorError::GitCommand(format!("git {}: {err}", args.first().unwrap_or(&"")))
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(FactorError::GitCommand(format!(
            "git {} failed (exit {})",
            args.first().unwrap_or(&""),
            status_code(status)
        )))
    }
}

/// Extracts exit code from process status.
pub(super) fn status_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(EXIT_SOFTWARE)
}

/// Validates that a commit is an ancestor of HEAD.
#[expect(
    clippy::single_call_fn,
    reason = "Isolates ancestry validation for clarity"
)]
pub(super) fn validate_ancestor(ctx: &Ctx<'_>, sha: &CommitSha) -> Result<(), FactorError> {
    let status = command_status_with(
        ctx,
        "git",
        &["merge-base", "--is-ancestor", sha.as_str(), "HEAD"],
        &[],
        true,
    )
    .map_err(|err| FactorError::GitCommand(err.to_string()))?;

    if status.success() {
        Ok(())
    } else {
        Err(FactorError::NotAncestor(sha.clone()))
    }
}

/// Validates that an exec command has valid bash syntax.
pub(super) fn validate_exec_syntax(ctx: &Ctx<'_>, command: &str) -> Result<(), FactorError> {
    let status = command_status_with(
        ctx,
        "bash",
        &["--norc", "--noprofile", "-n", "-c", command],
        &[],
        true,
    )
    .map_err(|err| FactorError::GitCommand(format!("bash syntax check: {err}")))?;

    if status.success() {
        Ok(())
    } else {
        Err(FactorError::InvalidExecSyntax(command.to_owned()))
    }
}

/// Validates that a commit is not a merge commit.
#[expect(clippy::single_call_fn, reason = "Isolates merge check for clarity")]
pub(super) fn validate_not_merge(ctx: &Ctx<'_>, sha: &CommitSha) -> Result<(), FactorError> {
    let has_second_parent = command_status_with(
        ctx,
        "git",
        &["rev-parse", "--verify", "--quiet", &format!("{sha}^2")],
        &[],
        true,
    )
    .is_ok_and(|status| status.success());

    if has_second_parent {
        Err(FactorError::MergeCommit(sha.clone()))
    } else {
        Ok(())
    }
}

/// Writes a state file to the factor state directory.
pub(super) fn write_state(
    ctx: &Ctx<'_>,
    state_dir: &Path,
    name: &str,
    content: &str,
) -> Result<(), FactorError> {
    let path = state_dir.join(name);
    ctx.fs
        .write_string(&path, &format!("{content}\n"))
        .map_err(FactorError::StateWrite)
}

/// Returns the literal `HEAD` reference as a validated non-empty string.
#[expect(clippy::expect_used, reason = "hardcoded HEAD is non-empty")]
pub(super) fn head_ref_literal() -> NonEmptyString {
    NonEmptyString::try_from("HEAD".to_owned()).expect("HEAD is non-empty")
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn shell_quote_wraps_and_escapes_single_quotes() {
        assert_eq!(shell_quote("abc"), "'abc'");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn error_to_exit_maps_variants_to_expected_exit_codes() {
        let sha = CommitSha::new("a".repeat(40)).expect("valid sha");
        let exec = NonEmptyString::try_from("true".to_owned()).expect("non-empty");
        let one: i32 = 1;

        let (code_active_session, _msg_active_session) = error_to_exit(&FactorError::ActiveSession);
        assert_eq!(code_active_session, EXIT_USAGE);

        let (code_no_staged, _msg_no_staged) = error_to_exit(&FactorError::NoStagedChanges);
        assert_eq!(code_no_staged, EXIT_USAGE);

        let (code_invalid_commit, _msg_invalid_commit) =
            error_to_exit(&FactorError::InvalidCommit("bad".to_owned()));
        assert_eq!(code_invalid_commit, EXIT_DATAERR);

        let sha_merge = CommitSha::new("a".repeat(40)).expect("valid sha");
        let (code_merge, _msg_merge) = error_to_exit(&FactorError::MergeCommit(sha_merge));
        assert_eq!(code_merge, EXIT_DATAERR);

        let (code_not_ancestor, _msg_not_ancestor) = error_to_exit(&FactorError::NotAncestor(sha));
        assert_eq!(code_not_ancestor, EXIT_DATAERR);

        let (code_exec_failed, _msg_exec_failed) = error_to_exit(&FactorError::ExecFailed {
            code: one,
            command: exec,
        });
        assert_eq!(code_exec_failed, EXIT_TEMPFAIL);

        let (code_git_command, _msg_git_command) =
            error_to_exit(&FactorError::GitCommand("nope".to_owned()));
        assert_eq!(code_git_command, EXIT_SOFTWARE);
    }

    #[test]
    fn git_output_reports_spawn_errors_as_git_command() {
        let dir = TempDir::new().expect("tempdir");
        let ctx = ctx_for(dir.path());
        let result = git_output_with(&ctx, "git-factor-not-a-real-git-binary", &["rev-parse"]);
        let err = result.expect_err("git should fail to spawn");
        assert!(
            matches!(&err, FactorError::GitCommand(msg) if msg.contains("git")),
            "err was: {err:?}"
        );
    }

    #[test]
    fn run_git_reports_spawn_errors_as_git_command() {
        let dir = TempDir::new().expect("tempdir");
        let ctx = ctx_for(dir.path());
        let result = run_git_with(&ctx, "git-factor-not-a-real-git-binary", &["rev-parse"]);
        let err = result.expect_err("git should fail to spawn");
        assert!(
            matches!(&err, FactorError::GitCommand(msg) if msg.contains("git")),
            "err was: {err:?}"
        );
    }

    #[test]
    fn command_status_with_can_run_in_quiet_mode() {
        let dir = TempDir::new().expect("tempdir");
        let ctx = ctx_for(dir.path());
        let status = command_status_with(
            &ctx,
            "bash",
            &["-c", "echo hi; echo err 1>&2; exit 0"],
            &[],
            true,
        )
        .expect("command should run");

        assert!(status.success());
    }

    #[test]
    fn command_status_with_reports_spawn_errors_as_git_command() {
        let dir = TempDir::new().expect("tempdir");
        let ctx = ctx_for(dir.path());
        let err = command_status_with(
            &ctx,
            "git-factor-not-a-real-binary",
            &["rev-parse"],
            &[],
            false,
        )
        .expect_err("spawn should fail");

        assert!(
            matches!(&err, FactorError::GitCommand(msg) if msg.contains("git-factor-not-a-real-binary")),
            "err was: {err:?}"
        );
    }
}
