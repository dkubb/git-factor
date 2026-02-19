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

    let Some((_code, status, _stderr)) =
        maybe_git_output(ctx, &["status", "--porcelain=v1", "--untracked-files=all"])
    else {
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

        if let Some((_code, toplevel, _stderr)) =
            maybe_git_output(ctx, &["rev-parse", "--show-toplevel"])
            && !toplevel.is_empty()
        {
            snapshot.toplevel = Some(toplevel);
        }

        let factor_dir = dir.join("factor");
        snapshot.factor_current_index =
            read_trimmed_optional(ctx, &factor_dir.join("current_index"));
        snapshot.factor_split_count = read_trimmed_optional(ctx, &factor_dir.join("split_count"));
        snapshot.factor_requires_rebase =
            read_trimmed_optional(ctx, &factor_dir.join("requires_rebase"));
        snapshot.factor_expected_tree =
            read_trimmed_optional(ctx, &factor_dir.join("expected_tree"));

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
            snapshot.rebase_todo_head =
                read_trimmed_optional(ctx, &rebase_merge.join("git-rebase-todo"))
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
    push_json_opt_str(buf, &format!("{prefix}_head"), snapshot.head.as_deref());
    buf.push(',');
    push_json_opt_str(
        buf,
        &format!("{prefix}_head_tree"),
        snapshot.head_tree.as_deref(),
    );
    buf.push(',');
    push_json_opt_str(
        buf,
        &format!("{prefix}_git_dir"),
        snapshot.git_dir.as_deref(),
    );
    buf.push(',');
    push_json_opt_str(
        buf,
        &format!("{prefix}_toplevel"),
        snapshot.toplevel.as_deref(),
    );
    buf.push(',');
    push_json_array(
        buf,
        &format!("{prefix}_staged_paths"),
        &snapshot.staged_paths,
    );
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
    let argv = args
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect::<Vec<String>>();
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
    push_json_opt_str(&mut line, "stdout", stdout.map(trace_text_limit).as_deref());
    line.push(',');
    push_json_opt_str(&mut line, "stderr", stderr.map(trace_text_limit).as_deref());
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

fn out_lines(ctx: &Ctx<'_>, lines: &[&str]) -> Result<(), FactorError> {
    for line in lines {
        ctx.outln(line)?;
    }
    Ok(())
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

    out_lines(
        ctx,
        &[
            "HINTS:",
            "  - Find the ONE smallest addition nothing depends on",
            "  - Target 15-30 lines (50 max)",
            "  - Message: single concrete action, no \"and\"/\"or\"",
            "  - Verify: git log --oneline | wc -l",
            "  - NEVER use git commit. ONLY use git factor --continue.",
        ],
    )?;
    if !remaining.is_empty() {
        ctx.outln(&format!("  REMAINING: {remaining}"))?;
    }
    if ctx.fs.exists(&rust_ref) {
        ctx.outln(&format!("  REFERENCE: {}", rust_ref.display()))?;
    }
    ctx.outln("  RECOVERY: git factor --abort")?;

    if ctx.env.var_os("CLAUDECODE").is_some() {
        out_lines(
            ctx,
            &[
                "<claude>",
                "- If context is above 50%, pause and ask the user to /compact.",
                "- Do NOT stop early. Keep committing until \"Complete\".",
                "- Do NOT use git commit directly. ONLY use git-factor --continue.",
                "- Each commit MUST pass the exec gate. No shortcuts.",
                "</claude>",
            ],
        )?;
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

    let started = if commit_count > 1 {
        format!("FACTOR: Split session started for {commit_count} commits (first: {short_sha}).")
    } else {
        format!("FACTOR: Split session started for {short_sha}.")
    };
    let original_message = format!("ORIGINAL MESSAGE: {message}");
    out_lines(
        ctx,
        &[started.as_str(), original_message.as_str(), "UNSTAGED:"],
    )?;
    for line in stat_output.lines() {
        ctx.outln(&format!("  {line}"))?;
    }
    if !untracked_output.is_empty() {
        ctx.outln("UNTRACKED:")?;
        for line in untracked_output.lines() {
            ctx.outln(&format!("  {line}"))?;
        }
    }
    out_lines(
        ctx,
        &[
            "",
            "NEXT: Stage changes for the first atomic commit, then run:",
            "  git factor --continue --message \"type: description\"",
            "",
            "Run git factor --help for the full workflow guide.",
            "",
        ],
    )?;
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

    let rebase_status = command_status_with(
        ctx,
        "git",
        &[
            "rebase",
            "--empty",
            "drop",
            "--interactive",
            "--quiet",
            "--root",
        ],
        &[
            ("GIT_EDITOR", "false"),
            ("GIT_SEQUENCE_EDITOR", &seq_editor),
        ],
        false,
    )?;

    if !rebase_status.success() {
        return Err(FactorError::GitCommand(format!(
            "rebase to remove empty root failed (exit {})",
            status_code(rebase_status)
        )));
    }

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
    use std::ffi::OsString;
    use std::os::unix::process::ExitStatusExt as _;
    use std::process::Command;
    use std::process::Output;
    use std::sync::Mutex;
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

    struct TestEnv {
        cwd: PathBuf,
        trace_log: Option<OsString>,
    }

    impl Env for TestEnv {
        fn current_dir(&self) -> io::Result<PathBuf> {
            Ok(self.cwd.clone())
        }

        fn current_exe(&self) -> io::Result<PathBuf> {
            std::env::current_exe()
        }

        fn var_os(&self, key: &str) -> Option<OsString> {
            if key == TRACE_LOG_ENV {
                return self.trace_log.clone();
            }
            None
        }
    }

    struct ToggleTraceEnv {
        cwd: PathBuf,
        trace_path: OsString,
        calls: Mutex<usize>,
    }

    impl Env for ToggleTraceEnv {
        fn current_dir(&self) -> io::Result<PathBuf> {
            Ok(self.cwd.clone())
        }

        fn current_exe(&self) -> io::Result<PathBuf> {
            std::env::current_exe()
        }

        fn var_os(&self, key: &str) -> Option<OsString> {
            if key != TRACE_LOG_ENV {
                return None;
            }
            let mut calls = self.calls.lock().expect("toggle env lock");
            *calls = calls.saturating_add(1);
            if *calls == 1 {
                Some(self.trace_path.clone())
            } else {
                None
            }
        }
    }

    struct HintEnv {
        cwd: PathBuf,
        claude_code: bool,
    }

    impl Env for HintEnv {
        fn current_dir(&self) -> io::Result<PathBuf> {
            Ok(self.cwd.clone())
        }

        fn current_exe(&self) -> io::Result<PathBuf> {
            std::env::current_exe()
        }

        fn var_os(&self, key: &str) -> Option<OsString> {
            if key == "CLAUDECODE" && self.claude_code {
                return Some(OsString::from("1"));
            }
            None
        }
    }

    #[derive(Default)]
    struct BufferIo {
        stdout: Mutex<String>,
        stderr: Mutex<String>,
    }

    impl BufferIo {
        fn stdout(&self) -> String {
            self.stdout
                .lock()
                .expect("stdout lock should not be poisoned")
                .clone()
        }
    }

    impl Io for BufferIo {
        fn out(&self, text: &str) -> io::Result<()> {
            self.stdout
                .lock()
                .expect("stdout lock should not be poisoned")
                .push_str(text);
            Ok(())
        }

        fn err(&self, text: &str) -> io::Result<()> {
            self.stderr
                .lock()
                .expect("stderr lock should not be poisoned")
                .push_str(text);
            Ok(())
        }
    }

    #[derive(Copy, Clone, Eq, PartialEq)]
    enum HintFailure {
        DiffStat,
        Untracked,
        TopLevel,
    }

    struct HintRunner {
        diff_stat: String,
        untracked: String,
        toplevel: PathBuf,
        fail_on: Option<HintFailure>,
    }

    impl Runner for HintRunner {
        fn output(&self, _bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
            if args == ["diff", "--stat"] && self.fail_on == Some(HintFailure::DiffStat) {
                return Err(io::Error::other("forced diff failure"));
            }
            if args == ["ls-files", "--others", "--exclude-standard"]
                && self.fail_on == Some(HintFailure::Untracked)
            {
                return Err(io::Error::other("forced untracked failure"));
            }
            if args == ["rev-parse", "--show-toplevel"]
                && self.fail_on == Some(HintFailure::TopLevel)
            {
                return Err(io::Error::other("forced show-toplevel failure"));
            }
            let stdout = match args {
                ["diff", "--stat"] => self.diff_stat.as_bytes().to_vec(),
                ["ls-files", "--others", "--exclude-standard"] => {
                    self.untracked.as_bytes().to_vec()
                }
                ["rev-parse", "--show-toplevel"] => {
                    format!("{}\n", self.toplevel.display()).into_bytes()
                }
                _ => {
                    return Err(io::Error::other(format!(
                        "unexpected args: {}",
                        args.join(" ")
                    )));
                }
            };

            Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout,
                stderr: Vec::new(),
            })
        }

        fn status(
            &self,
            _bin: &str,
            _args: &[&str],
            _envs: &[(&str, &str)],
            _quiet: bool,
            _cwd: &Path,
        ) -> io::Result<ExitStatus> {
            Ok(ExitStatus::from_raw(0))
        }
    }

    fn ctx_with_env<'a>(path: &Path, env: &'a dyn Env) -> Ctx<'a> {
        Ctx {
            runner: &REAL_RUNNER,
            cwd: path.to_path_buf(),
            io: &REAL_IO,
            env,
            fs: &REAL_FS,
        }
    }

    fn git_command_message(error: &FactorError) -> Option<&str> {
        if let FactorError::GitCommand(message) = error {
            Some(message.as_str())
        } else {
            None
        }
    }

    fn assert_git_command(error: &FactorError) {
        assert!(
            matches!(error, FactorError::GitCommand(_)),
            "error was: {error:?}"
        );
    }

    fn invalid_commit_message(error: &FactorError) -> Option<&str> {
        if let FactorError::InvalidCommit(message) = error {
            Some(message.as_str())
        } else {
            None
        }
    }

    fn merge_commit_sha(error: &FactorError) -> Option<&CommitSha> {
        if let FactorError::MergeCommit(sha) = error {
            Some(sha)
        } else {
            None
        }
    }

    fn init_git_repo(path: &Path) {
        let init = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(path)
            .status()
            .expect("run git init");
        assert!(init.success(), "git init failed: {init:?}");

        let config_name = Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .status()
            .expect("run git config user.name");
        assert!(config_name.success(), "git config user.name failed");

        let config_email = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .status()
            .expect("run git config user.email");
        assert!(config_email.success(), "git config user.email failed");

        fs::write(path.join("file.txt"), "base\n").expect("write file");
        let add = Command::new("git")
            .args(["add", "file.txt"])
            .current_dir(path)
            .status()
            .expect("run git add");
        assert!(add.success(), "git add failed");

        let commit = Command::new("git")
            .args(["commit", "--quiet", "-m", "base"])
            .current_dir(path)
            .status()
            .expect("run git commit");
        assert!(commit.success(), "git commit failed");
    }

    fn init_empty_root_repo(path: &Path) {
        let init = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(path)
            .status()
            .expect("run git init");
        assert!(init.success(), "git init failed: {init:?}");

        let config_name = Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .status()
            .expect("run git config user.name");
        assert!(config_name.success(), "git config user.name failed");

        let config_email = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .status()
            .expect("run git config user.email");
        assert!(config_email.success(), "git config user.email failed");

        let commit = Command::new("git")
            .args(["commit", "--allow-empty", "--quiet", "-m", "empty root"])
            .current_dir(path)
            .status()
            .expect("run git commit --allow-empty");
        assert!(commit.success(), "git commit --allow-empty failed");
    }

    #[derive(Default)]
    struct OutputOnlyRunner {
        output: Option<Output>,
        fail_output: bool,
    }

    impl Runner for OutputOnlyRunner {
        fn output(&self, _bin: &str, _args: &[&str], _cwd: &Path) -> io::Result<Output> {
            if self.fail_output {
                return Err(io::Error::other("forced output failure"));
            }
            self.output
                .clone()
                .ok_or_else(|| io::Error::other("missing scripted output"))
        }

        fn status(
            &self,
            _bin: &str,
            _args: &[&str],
            _envs: &[(&str, &str)],
            _quiet: bool,
            _cwd: &Path,
        ) -> io::Result<ExitStatus> {
            Ok(ExitStatus::from_raw(0))
        }
    }

    struct ShowTopLevelFailRunner;

    impl Runner for ShowTopLevelFailRunner {
        fn output(&self, _bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
            if args == ["rev-parse", "--show-toplevel"] {
                return Err(io::Error::other("forced show-toplevel failure"));
            }

            let stdout = match args {
                ["rev-parse", "--git-dir"] => b".git\n".to_vec(),
                ["status", "--porcelain=v1", "--untracked-files=all"] => Vec::new(),
                _ => b"deadbeef\n".to_vec(),
            };

            Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout,
                stderr: Vec::new(),
            })
        }

        fn status(
            &self,
            _bin: &str,
            _args: &[&str],
            _envs: &[(&str, &str)],
            _quiet: bool,
            _cwd: &Path,
        ) -> io::Result<ExitStatus> {
            Ok(ExitStatus::from_raw(0))
        }
    }

    struct NonInteractiveRunner;

    impl Runner for NonInteractiveRunner {
        fn output(&self, _bin: &str, _args: &[&str], _cwd: &Path) -> io::Result<Output> {
            Err(io::Error::other("output is not expected"))
        }

        fn status(
            &self,
            bin: &str,
            args: &[&str],
            envs: &[(&str, &str)],
            quiet: bool,
            _cwd: &Path,
        ) -> io::Result<ExitStatus> {
            assert_eq!(bin, "git");
            assert_eq!(args, ["status"]);
            assert!(!quiet);
            assert_eq!(
                envs,
                [("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")]
            );
            Ok(ExitStatus::from_raw(0))
        }
    }

    struct FailOnExactTextIo {
        text: String,
    }

    impl Io for FailOnExactTextIo {
        fn out(&self, text: &str) -> io::Result<()> {
            if text == self.text {
                Err(io::Error::other("io fail"))
            } else {
                Ok(())
            }
        }

        fn err(&self, _text: &str) -> io::Result<()> {
            Ok(())
        }
    }

    struct FailingExeEnv {
        cwd: PathBuf,
    }

    impl Env for FailingExeEnv {
        fn current_dir(&self) -> io::Result<PathBuf> {
            Ok(self.cwd.clone())
        }

        fn current_exe(&self) -> io::Result<PathBuf> {
            Err(io::Error::other("forced current_exe failure"))
        }

        fn var_os(&self, _key: &str) -> Option<OsString> {
            None
        }
    }

    #[derive(Copy, Clone, Eq, PartialEq)]
    enum RootFailure {
        RevList,
        LsTree,
        ShortRoot,
        CommitTree,
    }

    struct RootRunner {
        fail_on: Option<RootFailure>,
    }

    impl Runner for RootRunner {
        fn output(&self, _bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
            let stdout = match args {
                ["rev-list", "--max-parents=0", "HEAD"] => {
                    if self.fail_on == Some(RootFailure::RevList) {
                        return Err(io::Error::other("forced rev-list failure"));
                    }
                    "a".repeat(40).into_bytes()
                }
                ["ls-tree", _] => {
                    if self.fail_on == Some(RootFailure::LsTree) {
                        return Err(io::Error::other("forced ls-tree failure"));
                    }
                    Vec::new()
                }
                ["rev-parse", "--short", _] => {
                    if self.fail_on == Some(RootFailure::ShortRoot) {
                        return Err(io::Error::other("forced short-root failure"));
                    }
                    b"aaaaaaa\n".to_vec()
                }
                ["commit-tree", _, "-m", "empty"] => {
                    if self.fail_on == Some(RootFailure::CommitTree) {
                        return Err(io::Error::other("forced commit-tree failure"));
                    }
                    format!("{}\n", "b".repeat(40)).into_bytes()
                }
                _ => {
                    return Err(io::Error::other(format!(
                        "unexpected args: {}",
                        args.join(" ")
                    )));
                }
            };

            Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout,
                stderr: Vec::new(),
            })
        }

        fn status(
            &self,
            _bin: &str,
            _args: &[&str],
            _envs: &[(&str, &str)],
            _quiet: bool,
            _cwd: &Path,
        ) -> io::Result<ExitStatus> {
            Ok(ExitStatus::from_raw(0))
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
        let message = git_command_message(&err).expect("expected GitCommand");
        assert!(message.contains("git"), "err was: {err:?}");
    }

    #[test]
    fn run_git_reports_spawn_errors_as_git_command() {
        let dir = TempDir::new().expect("tempdir");
        let ctx = ctx_for(dir.path());
        let result = run_git_with(&ctx, "git-factor-not-a-real-git-binary", &["rev-parse"]);
        let err = result.expect_err("git should fail to spawn");
        let message = git_command_message(&err).expect("expected GitCommand");
        assert!(message.contains("git"), "err was: {err:?}");
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

        let message = git_command_message(&err).expect("expected GitCommand");
        assert!(
            message.contains("git-factor-not-a-real-binary"),
            "err was: {err:?}"
        );
    }

    #[test]
    fn output_only_runner_reports_missing_scripted_output() {
        let runner = OutputOnlyRunner {
            output: None,
            fail_output: false,
        };
        let err = runner
            .output("git", &["status"], Path::new("."))
            .expect_err("missing scripted output should fail");
        assert!(
            err.to_string().contains("missing scripted output"),
            "err was: {err:?}"
        );

        let status = runner
            .status("git", &["status"], &[], false, Path::new("."))
            .expect("status path should be callable");
        assert!(status.success());
    }

    #[test]
    fn error_variant_extractors_cover_matching_and_non_matching_paths() {
        let git_command = FactorError::GitCommand("boom".to_owned());
        assert_eq!(git_command_message(&git_command), Some("boom"));
        assert_eq!(invalid_commit_message(&git_command), None);
        assert_eq!(merge_commit_sha(&git_command), None);

        let invalid = FactorError::InvalidCommit("bad ref".to_owned());
        assert_eq!(git_command_message(&invalid), None);
        assert_eq!(invalid_commit_message(&invalid), Some("bad ref"));
        assert_eq!(merge_commit_sha(&invalid), None);

        let merge_sha = CommitSha::new("a".repeat(40)).expect("valid sha");
        let merge = FactorError::MergeCommit(merge_sha.clone());
        assert_eq!(git_command_message(&merge), None);
        assert_eq!(invalid_commit_message(&merge), None);
        assert_eq!(merge_commit_sha(&merge), Some(&merge_sha));
    }

    #[test]
    fn trace_helpers_cover_edge_cases() {
        let dir = TempDir::new().expect("tempdir");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(OsString::from("")),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        assert_eq!(trace_log_path(&ctx), None);
        assert_eq!(
            trace_text_limit(&"x".repeat(TRACE_MAX_TEXT_BYTES + 1)).len(),
            TRACE_MAX_TEXT_BYTES
        );
        assert_eq!(
            json_escape("\\\"\n\r\t\u{0001}"),
            "\\\\\\\"\\n\\r\\t\\u0001"
        );
        assert_eq!(
            first_rebase_todo_line("\n # c\n pick a\n"),
            Some("pick a".to_owned())
        );
        assert_eq!(last_non_empty_line("\n a\n\n"), Some("a".to_owned()));
    }

    #[test]
    fn trace_helpers_cover_non_empty_path_and_none_todo_line() {
        let dir = TempDir::new().expect("tempdir");
        let trace_path = dir.path().join("trace-extra.jsonl");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        assert_eq!(trace_log_path(&ctx), Some(trace_path));
        assert_eq!(trace_text_limit("short"), "short");
        assert_eq!(json_escape("\u{001F}"), "\\u001F");
        assert_eq!(first_rebase_todo_line("  # only comment\n\t# second"), None);
    }

    #[test]
    fn trace_helpers_cover_env_limit_escape_and_todo_branches() {
        let dir = TempDir::new().expect("tempdir");
        let trace_path = dir.path().join("trace-branches.jsonl");
        let env_none = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let env_empty = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(OsString::from("")),
        };
        let env_non_empty = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx_none = ctx_with_env(dir.path(), &env_none);
        let ctx_empty = ctx_with_env(dir.path(), &env_empty);
        let ctx_non_empty = ctx_with_env(dir.path(), &env_non_empty);

        assert_eq!(trace_log_path(&ctx_none), None);
        assert_eq!(trace_log_path(&ctx_empty), None);
        assert_eq!(trace_log_path(&ctx_non_empty), Some(trace_path));

        let within_limit = "x".repeat(TRACE_MAX_TEXT_BYTES);
        let at_limit = trace_text_limit(&within_limit);
        assert_eq!(at_limit.len(), TRACE_MAX_TEXT_BYTES);

        let over_limit = format!("{}y", "x".repeat(TRACE_MAX_TEXT_BYTES));
        let limited = trace_text_limit(&over_limit);
        assert_eq!(limited.len(), TRACE_MAX_TEXT_BYTES);

        let wide_over_limit = format!("{}é", "x".repeat(TRACE_MAX_TEXT_BYTES));
        let wide_limited = trace_text_limit(&wide_over_limit);
        assert_eq!(wide_limited.len(), TRACE_MAX_TEXT_BYTES);

        assert_eq!(json_escape("\u{0007}"), "\\u0007");
        assert_eq!(
            first_rebase_todo_line("# one\n\n# two\npick deadbeef message"),
            Some("pick deadbeef message".to_owned())
        );
        assert_eq!(first_rebase_todo_line("# one\n\t# two\n"), None);
    }

    #[test]
    fn test_env_var_os_returns_none_for_non_trace_keys() {
        let env = TestEnv {
            cwd: PathBuf::from("/tmp"),
            trace_log: Some(OsString::from("/tmp/trace.log")),
        };
        assert!(env.var_os("SOME_OTHER_ENV").is_none());
    }

    #[test]
    fn output_only_runner_status_returns_success_status() {
        let runner = OutputOnlyRunner::default();
        let status = runner
            .status("git", &["status"], &[], true, Path::new("."))
            .expect("status");
        assert!(status.success());
    }

    #[test]
    fn run_git_non_interactive_sets_editor_env() {
        let dir = TempDir::new().expect("tempdir");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let runner = NonInteractiveRunner;
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        run_git_non_interactive(&ctx, &["status"]).expect("status should succeed");
    }

    #[test]
    fn non_interactive_runner_output_is_not_expected() {
        let runner = NonInteractiveRunner;
        let err = runner
            .output("git", &["status"], Path::new("."))
            .expect_err("output should fail");
        assert_eq!(err.to_string(), "output is not expected");
    }

    #[test]
    fn trace_note_records_rebase_merge_and_custom_fields() {
        let dir = TempDir::new().expect("tempdir");
        init_git_repo(dir.path());

        let git_dir = dir.path().join(".git");
        let rebase_merge = git_dir.join("rebase-merge");
        fs::create_dir_all(&rebase_merge).expect("create rebase-merge");
        fs::write(rebase_merge.join("msgnum"), "2\n").expect("write msgnum");
        fs::write(rebase_merge.join("end"), "5\n").expect("write end");
        fs::write(
            rebase_merge.join("git-rebase-todo"),
            "# c\npick deadbeef step\n",
        )
        .expect("write todo");
        fs::write(rebase_merge.join("done"), "pick a\n\n").expect("write done");

        let trace_path = dir.path().join("trace/log.jsonl");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        trace_note(&ctx, "note", &[("key", "value")]);
        let trace = fs::read_to_string(&trace_path).expect("read trace");
        assert!(trace.contains("\"event\":\"note\""), "trace: {trace}");
        assert!(
            trace.contains("\"state_rebase_state\":\"rebase-merge\""),
            "trace: {trace}"
        );
        assert!(
            trace.contains("\"state_rebase_todo_head\":\"pick deadbeef step\""),
            "trace: {trace}"
        );
        assert!(
            trace.contains("\"state_rebase_done_tail\":\"pick a\""),
            "trace: {trace}"
        );
        assert!(trace.contains("\"key\":\"value\""), "trace: {trace}");
    }

    #[test]
    fn trace_note_records_rebase_apply_state() {
        let dir = TempDir::new().expect("tempdir");
        init_git_repo(dir.path());

        let git_dir = dir.path().join(".git");
        let rebase_apply = git_dir.join("rebase-apply");
        fs::create_dir_all(&rebase_apply).expect("create rebase-apply");
        fs::write(rebase_apply.join("next"), "3\n").expect("write next");
        fs::write(rebase_apply.join("last"), "9\n").expect("write last");
        fs::write(rebase_apply.join("patch"), "diff --git\n").expect("write patch");

        let trace_path = dir.path().join("trace-apply.jsonl");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        trace_note(&ctx, "note_apply", &[]);
        let trace = fs::read_to_string(&trace_path).expect("read trace");
        assert!(
            trace.contains("\"state_rebase_state\":\"rebase-apply\""),
            "trace: {trace}"
        );
        assert!(
            trace.contains("\"state_rebase_todo_head\":\"patch\""),
            "trace: {trace}"
        );
    }

    #[test]
    fn command_and_output_trace_spawn_errors_when_enabled() {
        let dir = TempDir::new().expect("tempdir");
        let trace_path = dir.path().join("trace-spawn.jsonl");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        let status_err = command_status_with(
            &ctx,
            "git-factor-not-a-real-status-binary",
            &["status"],
            &[],
            false,
        )
        .expect_err("status spawn should fail");
        assert_git_command(&status_err);

        let output_err = git_output_with(&ctx, "git-factor-not-a-real-output-binary", &["status"])
            .expect_err("output spawn should fail");
        assert_git_command(&output_err);

        let trace = fs::read_to_string(&trace_path).expect("read trace");
        assert!(trace.contains("\"mode\":\"status\""), "trace: {trace}");
        assert!(trace.contains("\"mode\":\"output\""), "trace: {trace}");
        assert!(trace.contains("\"spawned\":false"), "trace: {trace}");
    }

    #[test]
    fn run_git_wrappers_report_nonzero_exit_status() {
        let dir = TempDir::new().expect("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        let non_interactive = run_git_non_interactive(&ctx, &["definitely-not-a-command"])
            .expect_err("expected git failure");
        let non_interactive_message =
            git_command_message(&non_interactive).expect("non-interactive must return GitCommand");
        assert!(
            non_interactive_message.contains("failed (exit"),
            "unexpected error: {non_interactive:?}"
        );

        let run_with = run_git_with(&ctx, "git", &["definitely-not-a-command"])
            .expect_err("expected git failure");
        let run_with_message =
            git_command_message(&run_with).expect("run_git_with must return GitCommand");
        assert!(
            run_with_message.contains("failed (exit"),
            "unexpected error: {run_with:?}"
        );
    }

    #[test]
    fn collect_status_paths_covers_parser_branches_and_spawn_failure() {
        let dir = TempDir::new().expect("tempdir");
        let status_text = "?\n M unstaged.txt\nA  staged.txt\n?? untracked.txt\n";
        let runner = OutputOnlyRunner {
            output: Some(Output {
                status: ExitStatus::from_raw(0),
                stdout: status_text.as_bytes().to_vec(),
                stderr: Vec::new(),
            }),
            fail_output: false,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        };

        let (staged, unstaged, untracked) = collect_status_paths(&ctx);
        assert_eq!(staged, vec!["staged.txt".to_owned()]);
        assert_eq!(unstaged, vec!["unstaged.txt".to_owned()]);
        assert_eq!(untracked, vec!["untracked.txt".to_owned()]);

        let failing_runner = OutputOnlyRunner {
            output: None,
            fail_output: true,
        };
        let failing_ctx = Ctx {
            runner: &failing_runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        };
        let (staged_empty, unstaged_empty, untracked_empty) = collect_status_paths(&failing_ctx);
        assert!(staged_empty.is_empty());
        assert!(unstaged_empty.is_empty());
        assert!(untracked_empty.is_empty());
    }

    #[test]
    fn collect_status_paths_handles_short_and_question_mark_second_column() {
        let dir = TempDir::new().expect("tempdir");
        let status_text = "\nA? staged-only.txt\n?M odd.txt\n?? untracked.txt\n";
        let runner = OutputOnlyRunner {
            output: Some(Output {
                status: ExitStatus::from_raw(0),
                stdout: status_text.as_bytes().to_vec(),
                stderr: Vec::new(),
            }),
            fail_output: false,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        };

        let (staged, unstaged, untracked) = collect_status_paths(&ctx);
        assert_eq!(staged, vec!["staged-only.txt".to_owned()]);
        assert_eq!(unstaged, vec!["odd.txt".to_owned()]);
        assert_eq!(untracked, vec!["untracked.txt".to_owned()]);
    }

    #[test]
    fn collect_status_paths_respects_path_limits_and_short_lines() {
        let dir = TempDir::new().expect("tempdir");
        let mut lines = vec!["?".to_owned(), "A? staged-only.txt".to_owned()];
        for idx in 0..(TRACE_MAX_PATHS + 5) {
            lines.push(format!(" M unstaged-{idx}.txt"));
        }
        for idx in 0..(TRACE_MAX_PATHS + 5) {
            lines.push(format!("A  staged-{idx}.txt"));
        }
        for idx in 0..(TRACE_MAX_PATHS + 5) {
            lines.push(format!("?? untracked-{idx}.txt"));
        }
        let status_text = lines.join("\n");

        let runner = OutputOnlyRunner {
            output: Some(Output {
                status: ExitStatus::from_raw(0),
                stdout: status_text.into_bytes(),
                stderr: Vec::new(),
            }),
            fail_output: false,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        };

        let (staged, unstaged, untracked) = collect_status_paths(&ctx);
        assert_eq!(staged.len(), TRACE_MAX_PATHS);
        assert_eq!(unstaged.len(), TRACE_MAX_PATHS);
        assert_eq!(untracked.len(), TRACE_MAX_PATHS);
        assert!(staged.iter().any(|path| path == "staged-only.txt"));
        assert!(
            !unstaged.iter().any(|path| path == "staged-only.txt"),
            "A? lines should not be counted as unstaged"
        );
    }

    #[test]
    fn collect_repo_snapshot_handles_invalid_index_and_rebase_precedence() {
        let dir = TempDir::new().expect("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        let git_dir = dir.path().join(".git");
        let factor_dir = git_dir.join("factor");
        fs::create_dir_all(&factor_dir).expect("create factor dir");
        fs::write(factor_dir.join("commits"), "a\nb\n").expect("write commits");
        fs::write(factor_dir.join("current_index"), "not-a-number\n").expect("write index");

        let rebase_merge = git_dir.join("rebase-merge");
        fs::create_dir_all(&rebase_merge).expect("create rebase-merge");
        fs::write(rebase_merge.join("msgnum"), "2\n").expect("write msgnum");
        fs::write(rebase_merge.join("end"), "3\n").expect("write end");
        fs::write(
            rebase_merge.join("git-rebase-todo"),
            "pick deadbeef first\n",
        )
        .expect("write todo");
        fs::write(rebase_merge.join("done"), "pick feedface done\n").expect("write done");

        let rebase_apply = git_dir.join("rebase-apply");
        fs::create_dir_all(&rebase_apply).expect("create rebase-apply");
        fs::write(rebase_apply.join("next"), "9\n").expect("write next");
        fs::write(rebase_apply.join("last"), "10\n").expect("write last");

        let snapshot = collect_repo_snapshot(&ctx);
        assert_eq!(snapshot.factor_current_commit, None);
        assert_eq!(snapshot.rebase_state.as_deref(), Some("rebase-merge"));
        assert_eq!(
            snapshot.rebase_todo_head.as_deref(),
            Some("pick deadbeef first")
        );
    }

    #[test]
    fn collect_repo_snapshot_sets_current_commit_and_handles_rebase_absence() {
        let dir = TempDir::new().expect("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        let git_dir = dir.path().join(".git");
        let factor_dir = git_dir.join("factor");
        fs::create_dir_all(&factor_dir).expect("create factor dir");
        fs::write(factor_dir.join("commits"), "one\ntwo\nthree\n").expect("write commits");
        fs::write(factor_dir.join("current_index"), "1\n").expect("write index");

        let snapshot = collect_repo_snapshot(&ctx);
        assert_eq!(snapshot.factor_current_commit.as_deref(), Some("two"));
        assert_eq!(snapshot.rebase_state, None);
    }

    #[test]
    fn collect_repo_snapshot_reads_rebase_apply_state_when_merge_is_absent() {
        let dir = TempDir::new().expect("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        let git_dir = dir.path().join(".git");
        let rebase_apply = git_dir.join("rebase-apply");
        fs::create_dir_all(&rebase_apply).expect("create rebase-apply");
        fs::write(rebase_apply.join("next"), "4\n").expect("write next");
        fs::write(rebase_apply.join("last"), "8\n").expect("write last");
        fs::write(rebase_apply.join("patch"), "diff --git\n").expect("write patch");

        let snapshot = collect_repo_snapshot(&ctx);
        assert_eq!(snapshot.rebase_state.as_deref(), Some("rebase-apply"));
        assert_eq!(snapshot.rebase_todo_head.as_deref(), Some("patch"));
    }

    #[test]
    fn collect_repo_snapshot_skips_empty_toplevel_output() {
        let dir = TempDir::new().expect("tempdir");
        let runner = OutputOnlyRunner {
            output: Some(Output {
                status: ExitStatus::from_raw(0),
                stdout: b"\n".to_vec(),
                stderr: Vec::new(),
            }),
            fail_output: false,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        };

        let snapshot = collect_repo_snapshot(&ctx);
        assert_eq!(snapshot.head, None);
        assert_eq!(snapshot.head_tree, None);
        let git_dir = PathBuf::from(snapshot.git_dir.expect("git_dir from snapshot"));
        assert_eq!(git_dir, dir.path());
        assert_eq!(snapshot.toplevel, None);
    }

    #[test]
    fn collect_repo_snapshot_handles_show_toplevel_output_failure() {
        let dir = TempDir::new().expect("tempdir");
        let runner = ShowTopLevelFailRunner;
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        };

        let status = runner
            .status("git", &["status"], &[], false, dir.path())
            .expect("runner status");
        assert!(status.success(), "status should report success");

        let snapshot = collect_repo_snapshot(&ctx);
        let expected_git_dir = dir.path().join(".git");
        assert_eq!(snapshot.head.as_deref(), Some("deadbeef"));
        assert_eq!(snapshot.head_tree.as_deref(), Some("deadbeef"));
        assert_eq!(
            snapshot.git_dir.as_deref(),
            Some(expected_git_dir.to_string_lossy().as_ref())
        );
        assert_eq!(snapshot.toplevel, None);
    }

    #[test]
    fn trace_process_command_writes_when_tracing_is_enabled() {
        let dir = TempDir::new().expect("tempdir");
        init_git_repo(dir.path());

        let trace_path = dir.path().join("trace-process.jsonl");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx = ctx_with_env(dir.path(), &env);
        let before = collect_repo_snapshot(&ctx);
        let after = collect_repo_snapshot(&ctx);

        trace_process_command(
            &ctx,
            "status",
            "git",
            &["status"],
            &[],
            false,
            1,
            Some(0),
            None,
            None,
            true,
            &before,
            &after,
        );

        let trace = fs::read_to_string(&trace_path).expect("read trace");
        assert!(trace.contains("\"event\":\"process\""), "trace: {trace}");
        assert!(trace.contains("\"mode\":\"status\""), "trace: {trace}");
    }

    #[test]
    fn trace_process_command_returns_early_without_trace_env() {
        let dir = TempDir::new().expect("tempdir");
        init_git_repo(dir.path());
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let ctx = ctx_with_env(dir.path(), &env);
        let before = collect_repo_snapshot(&ctx);
        let after = collect_repo_snapshot(&ctx);

        trace_process_command(
            &ctx,
            "status",
            "git",
            &["status"],
            &[],
            false,
            1,
            Some(0),
            None,
            None,
            true,
            &before,
            &after,
        );

        let has_trace_file = fs::read_dir(dir.path())
            .expect("read directory")
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "jsonl")
            });
        assert!(
            !has_trace_file,
            "trace file should not be created when trace env is disabled"
        );
    }

    #[test]
    fn git_output_with_tracing_covers_success_and_nonzero_status_paths() {
        let dir = TempDir::new().expect("tempdir");
        init_git_repo(dir.path());

        let trace_path = dir.path().join("trace-output-status.jsonl");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        let status_out = git_output_with(&ctx, "git", &["status", "--porcelain"])
            .expect("status should succeed");
        assert!(
            !status_out.contains("fatal:"),
            "unexpected status output: {status_out}"
        );

        let err = git_output_with(&ctx, "git", &["definitely-not-a-command"])
            .expect_err("unknown git command should fail");
        let message = git_command_message(&err).expect("expected GitCommand");
        assert!(
            message.contains("definitely-not-a-command"),
            "unexpected error: {err:?}"
        );

        let trace = fs::read_to_string(&trace_path).expect("read trace");
        assert!(trace.contains("\"mode\":\"output\""), "trace: {trace}");
        assert!(trace.contains("\"spawned\":true"), "trace: {trace}");
    }

    #[test]
    fn trace_spawn_error_paths_cover_enabled_and_disabled_tracing() {
        let dir = TempDir::new().expect("tempdir");
        let env_disabled = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let ctx_disabled = ctx_with_env(dir.path(), &env_disabled);

        let status_err_disabled = command_status_with(
            &ctx_disabled,
            "git-factor-not-a-real-status-binary",
            &["status"],
            &[],
            false,
        )
        .expect_err("status spawn should fail");
        assert_git_command(&status_err_disabled);

        let output_err_disabled = git_output_with(
            &ctx_disabled,
            "git-factor-not-a-real-output-binary",
            &["status"],
        )
        .expect_err("output spawn should fail");
        assert_git_command(&output_err_disabled);

        let trace_path = dir.path().join("trace-errors.jsonl");
        let env_enabled = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx_enabled = ctx_with_env(dir.path(), &env_enabled);

        let status_err_enabled = command_status_with(
            &ctx_enabled,
            "git-factor-not-a-real-status-binary",
            &["status"],
            &[],
            false,
        )
        .expect_err("status spawn should fail");
        assert_git_command(&status_err_enabled);

        let output_err_enabled = git_output_with(
            &ctx_enabled,
            "git-factor-not-a-real-output-binary",
            &["status"],
        )
        .expect_err("output spawn should fail");
        assert_git_command(&output_err_enabled);

        let trace = fs::read_to_string(&trace_path).expect("read trace");
        assert!(trace.contains("\"mode\":\"status\""), "trace: {trace}");
        assert!(trace.contains("\"mode\":\"output\""), "trace: {trace}");
    }

    #[test]
    fn resolve_commit_refs_rejects_symmetric_diff_ranges() {
        let dir = TempDir::new().expect("tempdir");
        let ctx = ctx_for(dir.path());
        let commit_ref = NonEmptyString::try_from("HEAD...HEAD".to_owned()).expect("non-empty");
        let refs = NonEmpty::singleton(commit_ref);
        let err = resolve_commit_refs(&ctx, &refs).expect_err("symmetric diff must be rejected");

        let message = invalid_commit_message(&err).expect("expected InvalidCommit");
        assert!(
            message.contains("symmetric diff"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn validate_not_merge_rejects_merge_commit() {
        let dir = TempDir::new().expect("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        let branch =
            git_output(&ctx, &["rev-parse", "--abbrev-ref", "HEAD"]).expect("current branch");

        run_git(&ctx, &["checkout", "--quiet", "-b", "topic"]).expect("create branch");
        fs::write(dir.path().join("topic.txt"), "topic\n").expect("write topic file");
        run_git(&ctx, &["add", "topic.txt"]).expect("add topic");
        run_git(&ctx, &["commit", "--quiet", "-m", "topic change"]).expect("commit topic");

        run_git(&ctx, &["checkout", "--quiet", branch.as_str()]).expect("return branch");
        fs::write(dir.path().join("main.txt"), "main\n").expect("write main file");
        run_git(&ctx, &["add", "main.txt"]).expect("add main");
        run_git(&ctx, &["commit", "--quiet", "-m", "main change"]).expect("commit main");
        run_git(&ctx, &["merge", "--quiet", "--no-ff", "--no-edit", "topic"]).expect("merge topic");

        let merge_sha = CommitSha::new(
            git_output(&ctx, &["rev-parse", "--verify", "HEAD"]).expect("merge sha"),
        )
        .expect("valid merge sha");
        let err =
            validate_not_merge(&ctx, &merge_sha).expect_err("merge commit should be rejected");

        assert_eq!(
            merge_commit_sha(&err),
            Some(&merge_sha),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn append_and_trace_process_return_early_when_tracing_is_disabled_or_unwritable() {
        let dir = TempDir::new().expect("tempdir");
        let env_none = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let ctx_none = ctx_with_env(dir.path(), &env_none);
        append_trace_line(&ctx_none, "ignored");

        let env_bad = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(dir.path().as_os_str().to_os_string()),
        };
        let ctx_bad = ctx_with_env(dir.path(), &env_bad);
        append_trace_line(&ctx_bad, "ignored");

        let before = RepoSnapshot::default();
        let after = RepoSnapshot::default();
        trace_process_command(
            &ctx_none,
            "status",
            "git",
            &["status"],
            &[],
            false,
            0,
            None,
            None,
            None,
            false,
            &before,
            &after,
        );
    }

    #[test]
    fn append_trace_line_handles_trace_path_without_parent() {
        let dir = TempDir::new().expect("tempdir");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(OsString::from("/")),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        append_trace_line(&ctx, "{\"event\":\"noop\"}");
        let entries = fs::read_dir(dir.path()).expect("read tempdir");
        assert_eq!(entries.count(), 0);
    }

    #[test]
    fn test_env_methods_and_run_git_non_interactive_success_path() {
        let dir = TempDir::new().expect("tempdir");
        init_git_repo(dir.path());

        let trace_path = dir.path().join("trace.jsonl");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        let cwd = env.current_dir().expect("cwd");
        assert_eq!(cwd, dir.path().to_path_buf());
        let exe = env.current_exe().expect("current exe");
        assert!(exe.is_absolute(), "current_exe should be absolute: {exe:?}");
        assert!(env.var_os(TRACE_LOG_ENV).is_some());

        run_git_non_interactive(&ctx, &["status"]).expect("git status should succeed");
    }

    #[test]
    fn git_output_with_spawn_error_covers_trace_toggle_and_before_none_paths() {
        let dir = TempDir::new().expect("tempdir");
        let runner = OutputOnlyRunner {
            output: None,
            fail_output: true,
        };

        let toggle_env = ToggleTraceEnv {
            cwd: dir.path().to_path_buf(),
            trace_path: dir
                .path()
                .join("trace-toggle.jsonl")
                .as_os_str()
                .to_os_string(),
            calls: Mutex::new(0),
        };
        assert_eq!(
            toggle_env.current_dir().expect("toggle current_dir"),
            dir.path().to_path_buf()
        );
        let exe = toggle_env.current_exe().expect("toggle current_exe");
        assert!(exe.is_absolute(), "current_exe should be absolute: {exe:?}");
        assert!(toggle_env.var_os("UNRELATED_KEY").is_none());

        let ctx_toggle = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &toggle_env,
            fs: &REAL_FS,
        };
        let err =
            git_output_with(&ctx_toggle, "git", &["status"]).expect_err("expected output failure");
        let message = git_command_message(&err).expect("expected GitCommand");
        assert!(
            message.contains("forced output failure"),
            "err was: {err:?}"
        );

        let no_trace_env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let ctx_no_trace = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &no_trace_env,
            fs: &REAL_FS,
        };
        let err = git_output_with(&ctx_no_trace, "git", &["status"])
            .expect_err("expected output failure");
        let message = git_command_message(&err).expect("expected GitCommand");
        assert!(
            message.contains("forced output failure"),
            "err was: {err:?}"
        );
    }

    #[test]
    fn print_hints_in_includes_reference_and_claude_guidance() {
        let dir = TempDir::new().expect("tempdir");
        let references_dir = dir.path().join("references");
        fs::create_dir_all(&references_dir).expect("create references dir");
        let rust_reference = references_dir.join("rust.md");
        fs::write(&rust_reference, "# rust\n").expect("write rust reference");

        let runner = HintRunner {
            diff_stat: " file.txt | 1 +\n 1 file changed, 1 insertion(+)\n".to_owned(),
            untracked: String::new(),
            toplevel: dir.path().to_path_buf(),
            fail_on: None,
        };
        let io = BufferIo::default();
        let env = HintEnv {
            cwd: dir.path().to_path_buf(),
            claude_code: true,
        };
        assert_eq!(
            env.current_dir().expect("hint env cwd"),
            dir.path().to_path_buf()
        );
        let exe = env.current_exe().expect("hint env current_exe");
        assert!(exe.is_absolute(), "current_exe should be absolute: {exe:?}");
        assert_eq!(env.var_os("CLAUDECODE"), Some(OsString::from("1")));
        assert!(env.var_os("OTHER_ENV").is_none());
        io.err("note").expect("write stderr");
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        print_hints_in(&ctx).expect("print_hints_in should succeed");
        let stdout = io.stdout();
        assert!(stdout.contains("HINTS:"), "stdout: {stdout}");
        assert!(stdout.contains("REMAINING:"), "stdout: {stdout}");
        assert!(
            stdout.contains("1 file changed, 1 insertion(+)"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains(&format!("REFERENCE: {}", rust_reference.display())),
            "stdout: {stdout}"
        );
        assert!(stdout.contains("<claude>"), "stdout: {stdout}");
        assert!(
            stdout.contains("ONLY use git-factor --continue"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn print_session_started_reports_single_commit_and_untracked_paths() {
        let dir = TempDir::new().expect("tempdir");
        let references_dir = dir.path().join("references");
        fs::create_dir_all(&references_dir).expect("create references dir");
        fs::write(references_dir.join("rust.md"), "# rust\n").expect("write rust reference");

        let runner = HintRunner {
            diff_stat: " file.txt | 1 +\n 1 file changed, 1 insertion(+)\n".to_owned(),
            untracked: "new.txt\n".to_owned(),
            toplevel: dir.path().to_path_buf(),
            fail_on: None,
        };
        let io = BufferIo::default();
        let env = HintEnv {
            cwd: dir.path().to_path_buf(),
            claude_code: false,
        };
        let status = runner
            .status("git", &["status"], &[], true, dir.path())
            .expect("runner status");
        assert!(status.success(), "status should be success");
        let unexpected = runner
            .output("git", &["unexpected"], dir.path())
            .expect_err("unexpected command should fail");
        assert!(
            unexpected.to_string().contains("unexpected args"),
            "unexpected error: {unexpected:?}"
        );
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        let commit_sha = CommitSha::new("a".repeat(40)).expect("valid sha");
        let commits = NonEmpty::new(commit_sha);
        let short_sha = NonEmptyString::try_from("aaaaaaa".to_owned()).expect("non-empty");

        print_session_started(&ctx, &commits, &short_sha, "feat: example")
            .expect("print_session_started should succeed");
        let stdout = io.stdout();
        assert!(
            stdout.contains("FACTOR: Split session started for aaaaaaa."),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("ORIGINAL MESSAGE: feat: example"),
            "stdout: {stdout}"
        );
        assert!(stdout.contains("UNTRACKED:"), "stdout: {stdout}");
        assert!(stdout.contains("new.txt"), "stdout: {stdout}");
        assert!(
            stdout.contains("Run git factor --help for the full workflow guide."),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn read_trimmed_optional_returns_none_when_read_fails() {
        let dir = TempDir::new().expect("tempdir");
        let unreadable = dir.path().join("unreadable-dir");
        fs::create_dir_all(&unreadable).expect("create directory");
        let ctx = ctx_for(dir.path());

        let value = read_trimmed_optional(&ctx, &unreadable);
        assert_eq!(value, None);
    }

    #[test]
    fn read_state_parsed_reports_invalid_value() {
        let dir = TempDir::new().expect("tempdir");
        let ctx = ctx_for(dir.path());
        let state_dir = dir.path().join("factor");
        fs::create_dir_all(&state_dir).expect("create state dir");
        fs::write(state_dir.join("current_index"), "not-a-number\n").expect("write state");

        let err = read_state_parsed::<usize>(&ctx, &state_dir, "current_index")
            .expect_err("invalid state should fail");
        let message = git_command_message(&err).expect("expected GitCommand");
        assert!(
            message.contains("invalid value"),
            "unexpected error: {err:?}"
        );
    }

    fn assert_state_read_error(err: FactorError) {
        assert!(
            matches!(err, FactorError::StateRead(_)),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn read_state_parsed_propagates_read_errors() {
        let dir = TempDir::new().expect("tempdir");
        let ctx = ctx_for(dir.path());
        let state_dir = dir.path().join("factor");
        fs::create_dir_all(&state_dir).expect("create state dir");

        let err = read_state_parsed::<usize>(&ctx, &state_dir, "missing")
            .expect_err("missing state should fail");
        assert_state_read_error(err);
    }

    #[test]
    #[should_panic(expected = "unexpected error")]
    fn assert_state_read_error_panics_on_non_state_read_errors() {
        assert_state_read_error(FactorError::NotGitRepo);
    }

    #[test]
    fn print_hints_in_reports_io_failures_for_reference_and_claude_lines() {
        let dir = TempDir::new().expect("tempdir");
        let references_dir = dir.path().join("references");
        fs::create_dir_all(&references_dir).expect("create references dir");
        let rust_reference = references_dir.join("rust.md");
        fs::write(&rust_reference, "# rust\n").expect("write rust reference");

        let runner = HintRunner {
            diff_stat: " file.txt | 1 +\n 1 file changed, 1 insertion(+)\n".to_owned(),
            untracked: String::new(),
            toplevel: dir.path().to_path_buf(),
            fail_on: None,
        };
        let env = HintEnv {
            cwd: dir.path().to_path_buf(),
            claude_code: false,
        };
        let io_reference = FailOnExactTextIo {
            text: format!("  REFERENCE: {}", rust_reference.display()),
        };
        io_reference.err("stderr").expect("err should succeed");
        let ctx_reference = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &io_reference,
            env: &env,
            fs: &REAL_FS,
        };

        let reference_err =
            print_hints_in(&ctx_reference).expect_err("reference output should fail");
        assert!(
            reference_err.to_string().contains("io fail"),
            "unexpected error: {reference_err:?}"
        );

        let claude_env = HintEnv {
            cwd: dir.path().to_path_buf(),
            claude_code: true,
        };
        let io_claude = FailOnExactTextIo {
            text: "</claude>".to_owned(),
        };
        let ctx_claude = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &io_claude,
            env: &claude_env,
            fs: &REAL_FS,
        };

        let claude_err = print_hints_in(&ctx_claude).expect_err("claude output should fail");
        assert!(
            claude_err.to_string().contains("io fail"),
            "unexpected error: {claude_err:?}"
        );

        let toplevel_failure_runner = HintRunner {
            diff_stat: " file.txt | 1 +\n".to_owned(),
            untracked: String::new(),
            toplevel: dir.path().to_path_buf(),
            fail_on: Some(HintFailure::TopLevel),
        };
        let toplevel_ctx = Ctx {
            runner: &toplevel_failure_runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        let toplevel_err = print_hints_in(&toplevel_ctx).expect_err("show-toplevel should fail");
        let toplevel_message = git_command_message(&toplevel_err).expect("expected GitCommand");
        assert!(
            toplevel_message.contains("forced show-toplevel failure"),
            "unexpected error: {toplevel_err:?}"
        );
    }

    #[test]
    fn print_session_started_reports_runner_and_output_failures() {
        let dir = TempDir::new().expect("tempdir");
        let commit_sha = CommitSha::new("a".repeat(40)).expect("valid sha");
        let commits = NonEmpty::new(commit_sha);
        let short_sha = NonEmptyString::try_from("aaaaaaa".to_owned()).expect("non-empty");
        let env = HintEnv {
            cwd: dir.path().to_path_buf(),
            claude_code: false,
        };

        let diff_failure_runner = HintRunner {
            diff_stat: String::new(),
            untracked: String::new(),
            toplevel: dir.path().to_path_buf(),
            fail_on: Some(HintFailure::DiffStat),
        };
        let diff_ctx = Ctx {
            runner: &diff_failure_runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        let diff_err = print_session_started(&diff_ctx, &commits, &short_sha, "feat: test")
            .expect_err("diff command should fail");
        let diff_message = git_command_message(&diff_err).expect("expected GitCommand");
        assert!(
            diff_message.contains("forced diff failure"),
            "unexpected error: {diff_err:?}"
        );

        let untracked_failure_runner = HintRunner {
            diff_stat: " file.txt | 1 +\n".to_owned(),
            untracked: String::new(),
            toplevel: dir.path().to_path_buf(),
            fail_on: Some(HintFailure::Untracked),
        };
        let untracked_ctx = Ctx {
            runner: &untracked_failure_runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        let untracked_err =
            print_session_started(&untracked_ctx, &commits, &short_sha, "feat: test")
                .expect_err("untracked command should fail");
        let untracked_message = git_command_message(&untracked_err).expect("expected GitCommand");
        assert!(
            untracked_message.contains("forced untracked failure"),
            "unexpected error: {untracked_err:?}"
        );

        let io_runner = HintRunner {
            diff_stat: " file.txt | 1 +\n".to_owned(),
            untracked: "new.txt\n".to_owned(),
            toplevel: dir.path().to_path_buf(),
            fail_on: None,
        };

        let stat_line_io = FailOnExactTextIo {
            text: "  file.txt | 1 +".to_owned(),
        };
        let stat_line_ctx = Ctx {
            runner: &io_runner,
            cwd: dir.path().to_path_buf(),
            io: &stat_line_io,
            env: &env,
            fs: &REAL_FS,
        };
        let stat_line_err =
            print_session_started(&stat_line_ctx, &commits, &short_sha, "feat: test")
                .expect_err("stat line should fail");
        assert!(
            stat_line_err.to_string().contains("io fail"),
            "unexpected error: {stat_line_err:?}"
        );

        let header_io = FailOnExactTextIo {
            text: "UNTRACKED:".to_owned(),
        };
        let header_ctx = Ctx {
            runner: &io_runner,
            cwd: dir.path().to_path_buf(),
            io: &header_io,
            env: &env,
            fs: &REAL_FS,
        };
        let header_err = print_session_started(&header_ctx, &commits, &short_sha, "feat: test")
            .expect_err("untracked header should fail");
        assert!(
            header_err.to_string().contains("io fail"),
            "unexpected error: {header_err:?}"
        );

        let line_io = FailOnExactTextIo {
            text: "  new.txt".to_owned(),
        };
        let line_ctx = Ctx {
            runner: &io_runner,
            cwd: dir.path().to_path_buf(),
            io: &line_io,
            env: &env,
            fs: &REAL_FS,
        };
        let line_err = print_session_started(&line_ctx, &commits, &short_sha, "feat: test")
            .expect_err("untracked line should fail");
        assert!(
            line_err.to_string().contains("io fail"),
            "unexpected error: {line_err:?}"
        );

        let footer_io = FailOnExactTextIo {
            text: "Run git factor --help for the full workflow guide.".to_owned(),
        };
        let footer_ctx = Ctx {
            runner: &io_runner,
            cwd: dir.path().to_path_buf(),
            io: &footer_io,
            env: &env,
            fs: &REAL_FS,
        };
        let footer_err = print_session_started(&footer_ctx, &commits, &short_sha, "feat: test")
            .expect_err("footer output should fail");
        assert!(
            footer_err.to_string().contains("io fail"),
            "unexpected error: {footer_err:?}"
        );
    }

    #[test]
    fn remove_empty_root_in_reports_git_output_and_editor_path_failures() {
        let dir = TempDir::new().expect("tempdir");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };

        for (failure, expected) in [
            (RootFailure::RevList, "forced rev-list failure"),
            (RootFailure::LsTree, "forced ls-tree failure"),
            (RootFailure::ShortRoot, "forced short-root failure"),
        ] {
            let runner = RootRunner {
                fail_on: Some(failure),
            };
            let ctx = Ctx {
                runner: &runner,
                cwd: dir.path().to_path_buf(),
                io: &REAL_IO,
                env: &env,
                fs: &REAL_FS,
            };
            let err = remove_empty_root_in(&ctx).expect_err("remove_empty_root_in should fail");
            let message = git_command_message(&err).expect("expected GitCommand");
            assert!(message.contains(expected), "unexpected error: {err:?}");
        }

        let runner = RootRunner { fail_on: None };
        let failing_env = FailingExeEnv {
            cwd: dir.path().to_path_buf(),
        };
        assert_eq!(
            failing_env.current_dir().expect("cwd"),
            dir.path().to_path_buf()
        );
        assert!(failing_env.var_os("TRACE").is_none());
        let editor_ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &failing_env,
            fs: &REAL_FS,
        };
        let editor_err =
            remove_empty_root_in(&editor_ctx).expect_err("editor path resolution should fail");
        let editor_message = git_command_message(&editor_err).expect("expected GitCommand");
        assert!(
            editor_message.contains("forced current_exe failure"),
            "unexpected error: {editor_err:?}"
        );
    }

    #[test]
    fn root_runner_helpers_cover_unexpected_and_status_paths() {
        let runner = RootRunner { fail_on: None };
        let status = runner
            .status("git", &["status"], &[], false, Path::new("."))
            .expect("status");
        assert!(status.success());
        let unexpected = runner
            .output("git", &["status"], Path::new("."))
            .expect_err("unexpected args should fail");
        assert!(
            unexpected.to_string().contains("unexpected args"),
            "unexpected error: {unexpected:?}"
        );
    }

    #[test]
    fn mixed_reset_to_empty_with_root_runner_uses_commit_tree_output() {
        let dir = TempDir::new().expect("tempdir");
        let runner = RootRunner { fail_on: None };
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        mixed_reset_to_empty(&ctx).expect("reset should succeed with scripted commit tree");
    }

    #[test]
    fn mixed_reset_to_empty_reports_commit_tree_failure() {
        let dir = TempDir::new().expect("tempdir");
        let runner = RootRunner {
            fail_on: Some(RootFailure::CommitTree),
        };
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };

        let err = mixed_reset_to_empty(&ctx).expect_err("commit-tree failure should be returned");
        let message = git_command_message(&err).expect("expected GitCommand");
        assert!(
            message.contains("forced commit-tree failure"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn sort_topologically_orders_commits_from_oldest_to_newest() {
        let dir = TempDir::new().expect("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        fs::write(dir.path().join("a.txt"), "a\n").expect("write a");
        run_git(&ctx, &["add", "a.txt"]).expect("add a");
        run_git(&ctx, &["commit", "--quiet", "-m", "a"]).expect("commit a");
        let a =
            CommitSha::new(git_output(&ctx, &["rev-parse", "--verify", "HEAD"]).expect("sha a"))
                .expect("valid sha a");

        fs::write(dir.path().join("b.txt"), "b\n").expect("write b");
        run_git(&ctx, &["add", "b.txt"]).expect("add b");
        run_git(&ctx, &["commit", "--quiet", "-m", "b"]).expect("commit b");
        let b =
            CommitSha::new(git_output(&ctx, &["rev-parse", "--verify", "HEAD"]).expect("sha b"))
                .expect("valid sha b");

        let commits = Commits::new(BTreeSet::from([a.clone(), b.clone()])).expect("non-empty set");
        let sorted = sort_topologically(&ctx, &commits).expect("sort should succeed");
        let sorted_vec: Vec<_> = sorted.into_iter().collect();
        assert_eq!(sorted_vec, vec![a, b]);
    }

    #[test]
    fn sort_topologically_reports_git_output_failure_for_unknown_commits() {
        let dir = TempDir::new().expect("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        let fake = CommitSha::new("f".repeat(40)).expect("valid sha");
        let commits = Commits::new(BTreeSet::from([fake])).expect("non-empty set");
        let err = sort_topologically(&ctx, &commits).expect_err("git rev-list should fail");
        assert_git_command(&err);
    }

    #[test]
    #[should_panic(expected = "error was: Usage(\"oops\")")]
    fn assert_git_command_panics_for_non_git_errors() {
        assert_git_command(&FactorError::Usage("oops".to_owned()));
    }

    #[test]
    fn remove_empty_root_in_returns_early_when_root_has_content() {
        let dir = TempDir::new().expect("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        remove_empty_root_in(&ctx).expect("remove_empty_root_in should return early");
    }

    #[test]
    fn remove_empty_root_in_reports_rebase_failure_for_empty_root() {
        let dir = TempDir::new().expect("tempdir");
        init_empty_root_repo(dir.path());
        let ctx = ctx_for(dir.path());

        let err =
            remove_empty_root_in(&ctx).expect_err("rebase should fail without sequence editor");
        assert!(
            matches!(&err, FactorError::GitCommand(msg) if msg.contains("rebase to remove empty root failed")),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn remove_empty_root_in_passes_empty_drop_to_rebase() {
        struct RebaseArgsRunner;

        impl Runner for RebaseArgsRunner {
            fn output(&self, _bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
                let stdout = match args {
                    ["rev-list", "--max-parents=0", "HEAD"] => {
                        format!("{}\n", "a".repeat(40)).into_bytes()
                    }
                    ["ls-tree", _] => Vec::new(),
                    ["rev-parse", "--short", _] => b"aaaaaaa\n".to_vec(),
                    _ => {
                        return Err(io::Error::other(format!(
                            "unexpected args: {}",
                            args.join(" ")
                        )));
                    }
                };

                Ok(Output {
                    status: ExitStatus::from_raw(0),
                    stdout,
                    stderr: Vec::new(),
                })
            }

            fn status(
                &self,
                _bin: &str,
                args: &[&str],
                envs: &[(&str, &str)],
                _quiet: bool,
                _cwd: &Path,
            ) -> io::Result<ExitStatus> {
                assert_eq!(
                    args,
                    [
                        "rebase",
                        "--empty",
                        "drop",
                        "--interactive",
                        "--quiet",
                        "--root",
                    ],
                    "unexpected status args"
                );

                assert!(
                    envs.contains(&("GIT_EDITOR", "false")),
                    "expected GIT_EDITOR=false env var"
                );
                assert!(
                    envs.iter()
                        .any(|(key, value)| *key == "GIT_SEQUENCE_EDITOR"
                            && value.contains("--drop")),
                    "expected GIT_SEQUENCE_EDITOR with --drop"
                );

                Ok(ExitStatus::from_raw(0))
            }
        }

        let dir = TempDir::new().expect("tempdir");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let runner = RebaseArgsRunner;
        let unexpected = runner
            .output("git", &["unexpected"], dir.path())
            .expect_err("unexpected args should error");
        assert!(
            unexpected.to_string().contains("unexpected args"),
            "err was: {unexpected}"
        );

        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };

        remove_empty_root_in(&ctx).expect("remove_empty_root_in should pass expected rebase args");
    }

    #[test]
    fn mixed_reset_to_empty_resets_index_to_empty_tree() {
        const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

        let dir = TempDir::new().expect("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        mixed_reset_to_empty(&ctx).expect("mixed_reset_to_empty should succeed");
        let tree = git_output(&ctx, &["write-tree"]).expect("write-tree");
        assert_eq!(tree, EMPTY_TREE);
    }
}
