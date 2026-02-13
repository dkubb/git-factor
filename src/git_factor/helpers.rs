use super::*;
use core::str::FromStr;
use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Environment variable enabling JSONL trace logging.
const TRACE_LOG_ENV: &str = "GIT_FACTOR_TRACE_LOG";

/// Maximum number of path entries captured per status category.
const TRACE_MAX_PATHS: usize = 200;

/// Maximum number of bytes persisted for stdout/stderr payloads.
const TRACE_MAX_TEXT_BYTES: usize = 8192;

#[derive(Debug, Default)]
struct RepoSnapshot {
    head: Option<String>,
    head_tree: Option<String>,
    git_dir: Option<String>,
    toplevel: Option<String>,
    staged_paths: Vec<String>,
    unstaged_paths: Vec<String>,
    untracked_paths: Vec<String>,
    factor_current_index: Option<String>,
    factor_split_count: Option<String>,
    factor_requires_rebase: Option<String>,
    factor_expected_tree: Option<String>,
    factor_current_commit: Option<String>,
    rebase_state: Option<String>,
    rebase_msgnum: Option<String>,
    rebase_end: Option<String>,
    rebase_todo_head: Option<String>,
    rebase_done_tail: Option<String>,
}

fn trace_log_path(ctx: &Ctx<'_>) -> Option<PathBuf> {
    let raw = ctx.env.var_os(TRACE_LOG_ENV)?;
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |dur| dur.as_millis())
}

fn trace_text_limit(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        let next_len = ch.len_utf8();
        if out.len().saturating_add(next_len) > TRACE_MAX_TEXT_BYTES {
            break;
        }
        out.push(ch);
    }
    out
}

fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ if ch.is_control() => {
                let _ = write!(out, "\\u{:04X}", ch as u32);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn push_json_str(buf: &mut String, key: &str, value: &str) {
    let _ = write!(buf, "\"{}\":\"{}\"", json_escape(key), json_escape(value));
}

fn push_json_opt_str(buf: &mut String, key: &str, value: Option<&str>) {
    match value {
        Some(value) => push_json_str(buf, key, value),
        None => {
            let _ = write!(buf, "\"{}\":null", json_escape(key));
        }
    }
}

fn push_json_u128(buf: &mut String, key: &str, value: u128) {
    let _ = write!(buf, "\"{}\":{}", json_escape(key), value);
}

fn push_json_i32(buf: &mut String, key: &str, value: i32) {
    let _ = write!(buf, "\"{}\":{}", json_escape(key), value);
}

fn push_json_bool(buf: &mut String, key: &str, value: bool) {
    let _ = write!(
        buf,
        "\"{}\":{}",
        json_escape(key),
        if value { "true" } else { "false" }
    );
}

fn push_json_array(buf: &mut String, key: &str, values: &[String]) {
    let _ = write!(buf, "\"{}\":[", json_escape(key));
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            buf.push(',');
        }
        let _ = write!(buf, "\"{}\"", json_escape(value));
    }
    buf.push(']');
}

fn maybe_git_output(ctx: &Ctx<'_>, args: &[&str]) -> Option<(i32, String, String)> {
    let output = ctx.runner.output("git", args, &ctx.cwd).ok()?;
    let code = status_code(output.status);
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Some((code, stdout, stderr))
}

fn read_trimmed_optional(ctx: &Ctx<'_>, path: &Path) -> Option<String> {
    if !ctx.fs.exists(path) {
        return None;
    }
    let content = ctx.fs.read_to_string(path).ok()?;
    Some(content.trim().to_owned())
}

fn first_rebase_todo_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
}

fn last_non_empty_line(text: &str) -> Option<String> {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

fn collect_status_paths(ctx: &Ctx<'_>) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();

    let Some((_code, status, _stderr)) = maybe_git_output(
        ctx,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    ) else {
        return (staged, unstaged, untracked);
    };

    for line in status.lines() {
        if line.len() < 3 {
            continue;
        }
        let bytes = line.as_bytes();
        let path = line[3..].to_owned();
        if bytes[0] == b'?' && bytes[1] == b'?' {
            if untracked.len() < TRACE_MAX_PATHS {
                untracked.push(path);
            }
            continue;
        }
        if bytes[0] != b' ' && bytes[0] != b'?' && staged.len() < TRACE_MAX_PATHS {
            staged.push(path.clone());
        }
        if bytes[1] != b' ' && bytes[1] != b'?' && unstaged.len() < TRACE_MAX_PATHS {
            unstaged.push(path);
        }
    }

    (staged, unstaged, untracked)
}

fn collect_repo_snapshot(ctx: &Ctx<'_>) -> RepoSnapshot {
    let mut snapshot = RepoSnapshot::default();

    if let Some((_code, head, _stderr)) = maybe_git_output(ctx, &["rev-parse", "--verify", "HEAD"])
        && !head.is_empty()
    {
        snapshot.head = Some(head);
    }

    if let Some((_code, tree, _stderr)) =
        maybe_git_output(ctx, &["rev-parse", "--verify", "HEAD^{tree}"])
            && !tree.is_empty()
    {
        snapshot.head_tree = Some(tree);
    }

    let git_dir = git_dir_in(ctx).ok();
    if let Some(ref dir) = git_dir {
        snapshot.git_dir = Some(dir.to_string_lossy().into_owned());

        if let Some((_code, toplevel, _stderr)) = maybe_git_output(ctx, &["rev-parse", "--show-toplevel"])
            && !toplevel.is_empty()
        {
            snapshot.toplevel = Some(toplevel);
        }

        let factor_dir = dir.join("factor");
        snapshot.factor_current_index = read_trimmed_optional(ctx, &factor_dir.join("current_index"));
        snapshot.factor_split_count = read_trimmed_optional(ctx, &factor_dir.join("split_count"));
        snapshot.factor_requires_rebase =
            read_trimmed_optional(ctx, &factor_dir.join("requires_rebase"));
        snapshot.factor_expected_tree = read_trimmed_optional(ctx, &factor_dir.join("expected_tree"));

        if let (Some(commits), Some(index)) = (
            read_trimmed_optional(ctx, &factor_dir.join("commits")),
            snapshot
                .factor_current_index
                .as_deref()
                .and_then(|value| value.parse::<usize>().ok()),
        ) {
            let commit = commits
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .nth(index)
                .map(str::to_owned);
            snapshot.factor_current_commit = commit;
        }

        let rebase_merge = dir.join("rebase-merge");
        let rebase_apply = dir.join("rebase-apply");
        if ctx.fs.is_dir(&rebase_merge) {
            snapshot.rebase_state = Some("rebase-merge".to_owned());
            snapshot.rebase_msgnum = read_trimmed_optional(ctx, &rebase_merge.join("msgnum"));
            snapshot.rebase_end = read_trimmed_optional(ctx, &rebase_merge.join("end"));
            snapshot.rebase_todo_head = read_trimmed_optional(ctx, &rebase_merge.join("git-rebase-todo"))
                .as_deref()
                .and_then(first_rebase_todo_line);
            snapshot.rebase_done_tail = read_trimmed_optional(ctx, &rebase_merge.join("done"))
                .as_deref()
                .and_then(last_non_empty_line);
        } else if ctx.fs.is_dir(&rebase_apply) {
            snapshot.rebase_state = Some("rebase-apply".to_owned());
            snapshot.rebase_msgnum = read_trimmed_optional(ctx, &rebase_apply.join("next"));
            snapshot.rebase_end = read_trimmed_optional(ctx, &rebase_apply.join("last"));
            snapshot.rebase_todo_head =
                read_trimmed_optional(ctx, &rebase_apply.join("patch")).map(|_| "patch".to_owned());
            snapshot.rebase_done_tail = None;
        }
    }

    let (staged, unstaged, untracked) = collect_status_paths(ctx);
    snapshot.staged_paths = staged;
    snapshot.unstaged_paths = unstaged;
    snapshot.untracked_paths = untracked;
    snapshot
}

fn push_snapshot_fields(buf: &mut String, prefix: &str, snapshot: &RepoSnapshot) {
    push_json_opt_str(
        buf,
        &format!("{prefix}_head"),
        snapshot.head.as_deref(),
    );
    buf.push(',');
    push_json_opt_str(
        buf,
        &format!("{prefix}_head_tree"),
        snapshot.head_tree.as_deref(),
    );
    buf.push(',');
    push_json_opt_str(buf, &format!("{prefix}_git_dir"), snapshot.git_dir.as_deref());
    buf.push(',');
    push_json_opt_str(
        buf,
        &format!("{prefix}_toplevel"),
        snapshot.toplevel.as_deref(),
    );
    buf.push(',');
    push_json_array(buf, &format!("{prefix}_staged_paths"), &snapshot.staged_paths);
    buf.push(',');
    push_json_array(
        buf,
        &format!("{prefix}_unstaged_paths"),
        &snapshot.unstaged_paths,
    );
    buf.push(',');
    push_json_array(
        buf,
        &format!("{prefix}_untracked_paths"),
        &snapshot.untracked_paths,
    );
    buf.push(',');
    push_json_opt_str(
        buf,
        &format!("{prefix}_factor_current_index"),
        snapshot.factor_current_index.as_deref(),
    );
    buf.push(',');
    push_json_opt_str(
        buf,
        &format!("{prefix}_factor_split_count"),
        snapshot.factor_split_count.as_deref(),
    );
    buf.push(',');
    push_json_opt_str(
        buf,
        &format!("{prefix}_factor_requires_rebase"),
        snapshot.factor_requires_rebase.as_deref(),
    );
    buf.push(',');
    push_json_opt_str(
        buf,
        &format!("{prefix}_factor_expected_tree"),
        snapshot.factor_expected_tree.as_deref(),
    );
    buf.push(',');
    push_json_opt_str(
        buf,
        &format!("{prefix}_factor_current_commit"),
        snapshot.factor_current_commit.as_deref(),
    );
    buf.push(',');
    push_json_opt_str(
        buf,
        &format!("{prefix}_rebase_state"),
        snapshot.rebase_state.as_deref(),
    );
    buf.push(',');
    push_json_opt_str(
        buf,
        &format!("{prefix}_rebase_msgnum"),
        snapshot.rebase_msgnum.as_deref(),
    );
    buf.push(',');
    push_json_opt_str(
        buf,
        &format!("{prefix}_rebase_end"),
        snapshot.rebase_end.as_deref(),
    );
    buf.push(',');
    push_json_opt_str(
        buf,
        &format!("{prefix}_rebase_todo_head"),
        snapshot.rebase_todo_head.as_deref(),
    );
    buf.push(',');
    push_json_opt_str(
        buf,
        &format!("{prefix}_rebase_done_tail"),
        snapshot.rebase_done_tail.as_deref(),
    );
}

fn append_trace_line(ctx: &Ctx<'_>, line: &str) {
    let Some(path) = trace_log_path(ctx) else {
        return;
    };
    if let Some(parent) = path.parent() {
        drop(std::fs::create_dir_all(parent));
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    drop(file.write_all(line.as_bytes()));
    drop(file.write_all(b"\n"));
}

fn trace_process_command(
    ctx: &Ctx<'_>,
    mode: &str,
    bin: &str,
    args: &[&str],
    envs: &[(&str, &str)],
    quiet: bool,
    duration_ms: u128,
    exit_code: Option<i32>,
    stdout: Option<&str>,
    stderr: Option<&str>,
    spawned: bool,
    before: &RepoSnapshot,
    after: &RepoSnapshot,
) {
    if trace_log_path(ctx).is_none() {
        return;
    }

    let mut line = String::new();
    line.push('{');
    push_json_u128(&mut line, "ts_unix_ms", now_unix_ms());
    line.push(',');
    push_json_str(&mut line, "event", "process");
    line.push(',');
    push_json_str(&mut line, "mode", mode);
    line.push(',');
    push_json_str(&mut line, "bin", bin);
    line.push(',');
    let argv = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<String>>();
    push_json_array(&mut line, "args", &argv);
    line.push(',');
    let env_arr = envs
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<String>>();
    push_json_array(&mut line, "env", &env_arr);
    line.push(',');
    push_json_bool(&mut line, "quiet", quiet);
    line.push(',');
    push_json_bool(&mut line, "spawned", spawned);
    line.push(',');
    push_json_u128(&mut line, "duration_ms", duration_ms);
    line.push(',');
    if let Some(code) = exit_code {
        push_json_i32(&mut line, "exit_code", code);
    } else {
        push_json_opt_str(&mut line, "exit_code", None);
    }
    line.push(',');
    push_json_opt_str(
        &mut line,
        "stdout",
        stdout.map(trace_text_limit).as_deref(),
    );
    line.push(',');
    push_json_opt_str(
        &mut line,
        "stderr",
        stderr.map(trace_text_limit).as_deref(),
    );
    line.push(',');
    push_snapshot_fields(&mut line, "before", before);
    line.push(',');
    push_snapshot_fields(&mut line, "after", after);
    line.push('}');
    append_trace_line(ctx, &line);
}

/// Writes a note event into the trace log, if tracing is enabled.
pub(super) fn trace_note(ctx: &Ctx<'_>, event: &str, fields: &[(&str, &str)]) {
    if trace_log_path(ctx).is_none() {
        return;
    }
    let snapshot = collect_repo_snapshot(ctx);
    let mut line = String::new();
    line.push('{');
    push_json_u128(&mut line, "ts_unix_ms", now_unix_ms());
    line.push(',');
    push_json_str(&mut line, "event", event);
    line.push(',');
    push_snapshot_fields(&mut line, "state", &snapshot);
    for &(key, value) in fields {
        line.push(',');
        push_json_str(&mut line, key, value);
    }
    line.push('}');
    append_trace_line(ctx, &line);
}

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
    let trace_enabled = trace_log_path(ctx).is_some();
    let before = if trace_enabled {
        Some(collect_repo_snapshot(ctx))
    } else {
        None
    };
    let started = Instant::now();
    let result = ctx.runner.status(bin, args, envs, quiet, &ctx.cwd);
    let duration_ms = started.elapsed().as_millis();
    match result {
        Ok(status) => {
            if let Some(before) = before.as_ref() {
                let after = collect_repo_snapshot(ctx);
                trace_process_command(
                    ctx,
                    "status",
                    bin,
                    args,
                    envs,
                    quiet,
                    duration_ms,
                    Some(status.code().unwrap_or(EXIT_SOFTWARE)),
                    None,
                    None,
                    true,
                    before,
                    &after,
                );
            }
            Ok(status)
        }
        Err(err) => {
            if let Some(before) = before.as_ref() {
                let after = collect_repo_snapshot(ctx);
                let err_text = err.to_string();
                trace_process_command(
                    ctx,
                    "status",
                    bin,
                    args,
                    envs,
                    quiet,
                    duration_ms,
                    None,
                    None,
                    Some(err_text.as_str()),
                    false,
                    before,
                    &after,
                );
            }
            Err(FactorError::GitCommand(format!("{bin} {first_arg}: {err}")))
        }
    }
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
    let trace_enabled = trace_log_path(ctx).is_some();
    let before = if trace_enabled {
        Some(collect_repo_snapshot(ctx))
    } else {
        None
    };
    let started = Instant::now();
    let output = ctx.runner.output(bin, args, &ctx.cwd);
    let duration_ms = started.elapsed().as_millis();
    let output = output.map_err(|err| {
        if let Some(before) = before.as_ref() {
            let after = collect_repo_snapshot(ctx);
            let err_text = err.to_string();
            trace_process_command(
                ctx,
                "output",
                bin,
                args,
                &[],
                false,
                duration_ms,
                None,
                None,
                Some(err_text.as_str()),
                false,
                before,
                &after,
            );
        }
        FactorError::GitCommand(format!("git {}: {err}", args.first().unwrap_or(&"")))
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if let Some(before) = before.as_ref() {
        let after = collect_repo_snapshot(ctx);
        trace_process_command(
            ctx,
            "output",
            bin,
            args,
            &[],
            false,
            duration_ms,
            Some(output.status.code().unwrap_or(EXIT_SOFTWARE)),
            Some(stdout.as_str()),
            Some(stderr.as_str()),
            true,
            before,
            &after,
        );
    }

    if !output.status.success() {
        return Err(FactorError::GitCommand(stderr.trim().to_owned()));
    }

    Ok(stdout.trim().to_owned())
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
        &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
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
        &["rebase", "--interactive", "--quiet", "--root"],
        &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", &seq_editor)],
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
    run_git(ctx, &["reset", "--quiet", &commit_sha])
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

/// Runs a git command with editor invocations disabled.
///
/// Use this for flows where opening an editor is unexpected and should fail
/// fast (for example `git rebase --continue` in automated factor sessions).
pub(super) fn run_git_non_interactive(ctx: &Ctx<'_>, args: &[&str]) -> Result<(), FactorError> {
    let status = command_status_with(
        ctx,
        "git",
        args,
        &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
        false,
    )?;

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

/// Runs a git command using the provided executable name/path.
pub(super) fn run_git_with(ctx: &Ctx<'_>, bin: &str, args: &[&str]) -> Result<(), FactorError> {
    let status = command_status_with(ctx, bin, args, &[], false)?;

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
        &["rev-parse", "--quiet", "--verify", &format!("{sha}^2")],
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
