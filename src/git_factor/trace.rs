use core::error::Error as _;
use core::fmt::{Arguments, Write as _};
use core::str::FromStr;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

/// Parses the first actionable line from rebase todo text.
macro_rules! first_rebase_todo_line_inline {
    ($text:expr) => {
        $text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_owned)
    };
}

/// Parses the last non-empty line from text.
macro_rules! last_non_empty_line_inline {
    ($text:expr) => {
        $text
            .lines()
            .rev()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_owned)
    };
}

/// Parses `git status --porcelain=v1` output into staged/unstaged/untracked sets.
macro_rules! collect_status_paths_inline {
    ($ctx:expr) => {{
        let mut staged = Vec::new();
        let mut unstaged = Vec::new();
        let mut untracked = Vec::new();

        if let Some((_code, status, _stderr)) =
            maybe_git_output($ctx, &["status", "--porcelain=v1", "--untracked-files=all"])
        {
            for line in status.lines() {
                let bytes = line.as_bytes();
                let &[index_status, worktree_status, ..] = bytes else {
                    continue;
                };
                let Some(path) = line.get(3..) else {
                    continue;
                };
                let path_text = path.to_owned();
                if index_status == b'?' && worktree_status == b'?' {
                    if untracked.len() < TRACE_MAX_PATHS {
                        untracked.push(path_text);
                    }
                    continue;
                }
                if index_status != b'?' && index_status != b' ' && staged.len() < TRACE_MAX_PATHS {
                    staged.push(path_text.clone());
                }
                if worktree_status != b'?'
                    && worktree_status != b' '
                    && unstaged.len() < TRACE_MAX_PATHS
                {
                    unstaged.push(path_text);
                }
            }
        }

        (staged, unstaged, untracked)
    }};
}

/// Environment variable enabling JSONL trace logging.
pub(in crate::git_factor) const TRACE_LOG_ENV: &str = "GIT_FACTOR_TRACE_LOG";

/// File name used for persisted unexpected-session failures.
pub(in crate::git_factor) const ERROR_LOG_FILE: &str = "error.log";

/// Maximum number of path entries captured per status category.
pub(in crate::git_factor) const TRACE_MAX_PATHS: usize = 200;

/// Maximum number of bytes persisted for stdout/stderr payloads.
pub(in crate::git_factor) const TRACE_MAX_TEXT_BYTES: usize = 8192;

#[derive(Debug, Default)]
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "fields must be visible to sibling modules within git_factor"
)]
/// Snapshot of repository and factor-session state for trace logging.
pub(in crate::git_factor) struct RepoSnapshot {
    /// Current commit being split, derived from state.
    pub(in crate::git_factor) factor_current_commit: Option<String>,
    /// Persisted factor current index.
    pub(in crate::git_factor) factor_current_index: Option<usize>,
    /// Persisted expected tree hash.
    pub(in crate::git_factor) factor_expected_tree: Option<String>,
    /// Persisted requires-rebase flag.
    pub(in crate::git_factor) factor_requires_rebase: Option<StateBool>,
    /// Persisted split-count value.
    pub(in crate::git_factor) factor_split_count: Option<u32>,
    /// Absolute path to `.git` when resolvable.
    pub(in crate::git_factor) git_dir: Option<String>,
    /// Current `HEAD` commit SHA when resolvable.
    pub(in crate::git_factor) head: Option<String>,
    /// Tree hash for `HEAD` when resolvable.
    pub(in crate::git_factor) head_tree: Option<String>,
    /// Last non-empty line in the rebase done file.
    pub(in crate::git_factor) rebase_done_tail: Option<String>,
    /// Rebase total message count.
    pub(in crate::git_factor) rebase_end: Option<RebaseCounter>,
    /// Current rebase message index.
    pub(in crate::git_factor) rebase_msgnum: Option<RebaseCounter>,
    /// Active rebase state directory name.
    pub(in crate::git_factor) rebase_state: Option<RebaseState>,
    /// First actionable line in the rebase todo file.
    pub(in crate::git_factor) rebase_todo_head: Option<String>,
    /// Paths with staged changes.
    pub(in crate::git_factor) staged_paths: Vec<String>,
    /// Repository top-level path when resolvable.
    pub(in crate::git_factor) toplevel: Option<String>,
    /// Paths with unstaged tracked changes.
    pub(in crate::git_factor) unstaged_paths: Vec<String>,
    /// Paths reported as untracked.
    pub(in crate::git_factor) untracked_paths: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Active rebase backend directory type.
pub(in crate::git_factor) enum RebaseState {
    /// `.git/rebase-apply` is active.
    Apply,
    /// `.git/rebase-merge` is active.
    Merge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "field must be visible to sibling modules within git_factor"
)]
/// Rebase message counter value read from git's rebase state files.
pub(in crate::git_factor) struct RebaseCounter(pub(in crate::git_factor) u32);

impl RebaseCounter {
    /// Returns the numeric counter value.
    const fn as_u32(self) -> u32 {
        self.0
    }
}

/// Trace payload fields for a spawned process execution.
#[derive(Clone, Copy)]
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "fields must be visible to sibling modules within git_factor"
)]
pub(in crate::git_factor) struct ProcessTrace<'trace> {
    /// Repository snapshot after process execution.
    pub(in crate::git_factor) after: &'trace RepoSnapshot,
    /// Command arguments.
    pub(in crate::git_factor) args: &'trace [&'trace str],
    /// Repository snapshot before process execution.
    pub(in crate::git_factor) before: &'trace RepoSnapshot,
    /// Executable/binary name.
    pub(in crate::git_factor) bin: &'trace str,
    /// Execution duration in milliseconds.
    pub(in crate::git_factor) duration_ms: u64,
    /// Environment variable pairs.
    pub(in crate::git_factor) envs: &'trace [(&'trace str, &'trace str)],
    /// Exit code when process launched successfully.
    pub(in crate::git_factor) exit_code: Option<i32>,
    /// Trace mode (`status` or `output`).
    pub(in crate::git_factor) mode: &'trace str,
    /// Whether command output was quieted.
    pub(in crate::git_factor) quiet: bool,
    /// Whether the process was successfully spawned.
    pub(in crate::git_factor) spawned: bool,
    /// Captured stderr payload.
    pub(in crate::git_factor) stderr: Option<&'trace str>,
    /// Captured stdout payload.
    pub(in crate::git_factor) stdout: Option<&'trace str>,
}

/// Returns the configured trace-log path from `GIT_FACTOR_TRACE_LOG`.
pub(in crate::git_factor) fn trace_log_path(ctx: &Ctx<'_>) -> Option<PathBuf> {
    let raw = ctx.env.var_os(TRACE_LOG_ENV)?;
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

/// Returns the current Unix epoch timestamp in milliseconds.
pub(in crate::git_factor) fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |dur| u64::try_from(dur.as_millis()).unwrap_or(u64::MAX))
}

/// Truncates text to the trace payload byte limit on UTF-8 boundaries.
pub(in crate::git_factor) fn trace_text_limit(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        let next_len = ch.len_utf8();
        let total_len = out.len().checked_add(next_len);
        if total_len.is_none_or(|len| len > TRACE_MAX_TEXT_BYTES) {
            break;
        }
        out.push(ch);
    }
    out
}

/// Escapes a string for JSON string literal embedding.
pub(in crate::git_factor) fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ if ch.is_control() => {
                let _wrote_escape = write!(out, "\\u{:04X}", u32::from(ch)).is_ok();
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Appends a JSON string field to `buf`.
pub(in crate::git_factor) fn push_json_str(buf: &mut String, key: &str, value: &str) {
    let _wrote_pair = write!(buf, "\"{}\":\"{}\"", json_escape(key), json_escape(value)).is_ok();
}

/// Appends an optional JSON string field to `buf`.
pub(in crate::git_factor) fn push_json_opt_str(buf: &mut String, key: &str, value: Option<&str>) {
    match value {
        Some(non_null_value) => push_json_str(buf, key, non_null_value),
        None => {
            let _wrote_null = write!(buf, "\"{}\":null", json_escape(key)).is_ok();
        }
    }
}

/// Appends a JSON `u64` field to `buf`.
pub(in crate::git_factor) fn push_json_u64(buf: &mut String, key: &str, value: u64) {
    let _wrote_u64 = write!(buf, "\"{}\":{}", json_escape(key), value).is_ok();
}

/// Appends a JSON boolean field to `buf`.
pub(in crate::git_factor) fn push_json_bool(buf: &mut String, key: &str, value: bool) {
    let _wrote_bool = write!(
        buf,
        "\"{}\":{}",
        json_escape(key),
        if value { "true" } else { "false" }
    )
    .is_ok();
}

/// Appends a JSON string-array field to `buf`.
pub(in crate::git_factor) fn push_json_array(buf: &mut String, key: &str, values: &[String]) {
    let _wrote_open = write!(buf, "\"{}\":[", json_escape(key)).is_ok();
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            buf.push(',');
        }
        let _wrote_value = write!(buf, "\"{}\"", json_escape(value)).is_ok();
    }
    buf.push(']');
}

/// Runs a git command and returns `(exit_code, stdout, stderr)` on success.
pub(in crate::git_factor) fn maybe_git_output(
    ctx: &Ctx<'_>,
    args: &[&str],
) -> Option<(i32, String, String)> {
    let output = ctx.runner.output("git", args, &ctx.cwd).ok()?;
    let code = status_code(output.status);
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Some((code, stdout, stderr))
}

/// Reads a file when present and returns its trimmed contents.
pub(in crate::git_factor) fn read_trimmed_optional(ctx: &Ctx<'_>, path: &Path) -> Option<String> {
    let content = ctx.fs.read_to_string(path).ok()?;
    Some(content.trim().to_owned())
}

/// Reads a file when present, trims it, and parses to a typed value.
pub(in crate::git_factor) fn read_trimmed_optional_parsed<T>(
    ctx: &Ctx<'_>,
    path: &Path,
) -> Option<T>
where
    T: FromStr,
{
    read_trimmed_optional(ctx, path)?.parse().ok()
}

/// Reads an optional rebase counter from `path`.
pub(in crate::git_factor) fn read_rebase_counter(
    ctx: &Ctx<'_>,
    path: &Path,
) -> Option<RebaseCounter> {
    read_trimmed_optional_parsed::<u32>(ctx, path).map(RebaseCounter)
}

/// Returns the first non-empty, non-comment line from a rebase todo file.
#[cfg(test)]
pub(in crate::git_factor) fn first_rebase_todo_line(text: &str) -> Option<String> {
    first_rebase_todo_line_inline!(text)
}

/// Returns the last non-empty line in `text`.
#[cfg(test)]
pub(in crate::git_factor) fn last_non_empty_line(text: &str) -> Option<String> {
    last_non_empty_line_inline!(text)
}

/// Collects staged, unstaged, and untracked paths from porcelain status output.
#[cfg(test)]
pub(in crate::git_factor) fn collect_status_paths(
    ctx: &Ctx<'_>,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    collect_status_paths_inline!(ctx)
}

/// Captures repository/factor/rebase state for process trace records.
pub(in crate::git_factor) fn collect_repo_snapshot(ctx: &Ctx<'_>) -> RepoSnapshot {
    let mut snapshot = RepoSnapshot::default();

    if let Some((_code, head, _stderr)) = maybe_git_output(ctx, &["rev-parse", "--verify", "HEAD"])
        && !head.is_empty()
    {
        snapshot.head = Some(head);
    }

    let head_tree_spec = format!("HEAD^{}tree{}", '{', '}');
    if let Some((_code, tree, _stderr)) =
        maybe_git_output(ctx, &["rev-parse", "--verify", head_tree_spec.as_str()])
        && !tree.is_empty()
    {
        snapshot.head_tree = Some(tree);
    }

    let git_dir = git_dir_in(ctx).ok();
    if let Some(dir) = git_dir.as_ref() {
        snapshot.git_dir = Some(dir.to_string_lossy().into_owned());

        if let Some((_code, toplevel, _stderr)) =
            maybe_git_output(ctx, &["rev-parse", "--show-toplevel"])
            && !toplevel.is_empty()
        {
            snapshot.toplevel = Some(toplevel);
        }

        let factor_dir = dir.join("factor");
        snapshot.factor_current_index =
            read_trimmed_optional_parsed::<usize>(ctx, &factor_dir.join("current_index"));
        snapshot.factor_split_count =
            read_trimmed_optional_parsed::<u32>(ctx, &factor_dir.join("split_count"));
        snapshot.factor_requires_rebase =
            read_trimmed_optional_parsed::<bool>(ctx, &factor_dir.join("requires_rebase"))
                .map(StateBool::from_bool);
        snapshot.factor_expected_tree =
            read_trimmed_optional(ctx, &factor_dir.join("expected_tree"));

        if let (Some(commits), Some(index)) = (
            read_trimmed_optional(ctx, &factor_dir.join("commits")),
            snapshot.factor_current_index,
        ) {
            let commit = commits
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .nth(index)
                .map(str::to_owned);
            snapshot.factor_current_commit = commit;
        }

        let rebase_merge = dir.join(REBASE_MERGE_DIR);
        let rebase_apply = dir.join(REBASE_APPLY_DIR);
        if ctx.fs.is_dir(&rebase_merge) {
            snapshot.rebase_state = Some(RebaseState::Merge);
            snapshot.rebase_msgnum = read_rebase_counter(ctx, &rebase_merge.join("msgnum"));
            snapshot.rebase_end = read_rebase_counter(ctx, &rebase_merge.join("end"));
            snapshot.rebase_todo_head =
                read_trimmed_optional(ctx, &rebase_merge.join("git-rebase-todo"))
                    .as_deref()
                    .and_then(|text| first_rebase_todo_line_inline!(text));
            snapshot.rebase_done_tail = read_trimmed_optional(ctx, &rebase_merge.join("done"))
                .as_deref()
                .and_then(|text| last_non_empty_line_inline!(text));
        } else if ctx.fs.is_dir(&rebase_apply) {
            snapshot.rebase_state = Some(RebaseState::Apply);
            snapshot.rebase_msgnum = read_rebase_counter(ctx, &rebase_apply.join("next"));
            snapshot.rebase_end = read_rebase_counter(ctx, &rebase_apply.join("last"));
            snapshot.rebase_todo_head =
                read_trimmed_optional(ctx, &rebase_apply.join("patch")).map(|_| "patch".to_owned());
            snapshot.rebase_done_tail = None;
        } else {
            snapshot.rebase_state = None;
            snapshot.rebase_msgnum = None;
            snapshot.rebase_end = None;
            snapshot.rebase_todo_head = None;
            snapshot.rebase_done_tail = None;
        }
    }

    let (staged, unstaged, untracked) = collect_status_paths_inline!(ctx);
    snapshot.staged_paths = staged;
    snapshot.unstaged_paths = unstaged;
    snapshot.untracked_paths = untracked;
    snapshot
}

/// Appends snapshot fields to a JSON object under the given key prefix.
pub(in crate::git_factor) fn push_snapshot_fields(
    buf: &mut String,
    prefix: &str,
    snapshot: &RepoSnapshot,
) {
    macro_rules! push_opt {
        ($suffix:literal, $value:expr) => {{
            push_json_opt_str(buf, &format!("{prefix}_{}", $suffix), $value);
            buf.push(',');
        }};
    }
    macro_rules! push_array {
        ($suffix:literal, $value:expr) => {{
            push_json_array(buf, &format!("{prefix}_{}", $suffix), $value);
            buf.push(',');
        }};
    }
    let factor_current_index_text = snapshot.factor_current_index.map(|value| value.to_string());
    let factor_split_count_text = snapshot.factor_split_count.map(|value| value.to_string());
    let rebase_msgnum_text = snapshot
        .rebase_msgnum
        .map(|value| value.as_u32().to_string());
    let rebase_end_text = snapshot.rebase_end.map(|value| value.as_u32().to_string());

    push_opt!("head", snapshot.head.as_deref());
    push_opt!("head_tree", snapshot.head_tree.as_deref());
    push_opt!("git_dir", snapshot.git_dir.as_deref());
    push_opt!("toplevel", snapshot.toplevel.as_deref());
    push_array!("staged_paths", &snapshot.staged_paths);
    push_array!("unstaged_paths", &snapshot.unstaged_paths);
    push_array!("untracked_paths", &snapshot.untracked_paths);
    push_opt!("factor_current_index", factor_current_index_text.as_deref());
    push_opt!("factor_split_count", factor_split_count_text.as_deref());
    push_opt!(
        "factor_requires_rebase",
        snapshot
            .factor_requires_rebase
            .map(|value| if value.as_bool() { "true" } else { "false" })
    );
    push_opt!(
        "factor_expected_tree",
        snapshot.factor_expected_tree.as_deref()
    );
    push_opt!(
        "factor_current_commit",
        snapshot.factor_current_commit.as_deref()
    );
    push_opt!(
        "rebase_state",
        snapshot.rebase_state.map(|state| match state {
            RebaseState::Apply => REBASE_APPLY_DIR,
            RebaseState::Merge => REBASE_MERGE_DIR,
        })
    );
    push_opt!("rebase_msgnum", rebase_msgnum_text.as_deref());
    push_opt!("rebase_end", rebase_end_text.as_deref());
    push_opt!("rebase_todo_head", snapshot.rebase_todo_head.as_deref());
    push_json_opt_str(
        buf,
        &format!("{prefix}_rebase_done_tail"),
        snapshot.rebase_done_tail.as_deref(),
    );
}

/// Appends one newline-terminated JSONL trace record to disk.
pub(in crate::git_factor) fn append_trace_line(ctx: &Ctx<'_>, line: &str) {
    let Some(path) = trace_log_path(ctx) else {
        return;
    };
    if let Some(parent) = path.parent() {
        drop(fs::create_dir_all(parent));
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    drop(file.write_all(line.as_bytes()));
    drop(file.write_all(b"\n"));
}

/// Emits a structured trace record for a spawned process execution.
pub(in crate::git_factor) fn trace_process_command(ctx: &Ctx<'_>, trace: ProcessTrace<'_>) {
    let mut line = String::new();
    line.push('{');
    push_json_u64(&mut line, "ts_unix_ms", now_unix_ms());
    line.push(',');
    push_json_str(&mut line, "event", "process");
    line.push(',');
    push_json_str(&mut line, "mode", trace.mode);
    line.push(',');
    push_json_str(&mut line, "bin", trace.bin);
    line.push(',');
    let arg_values = trace
        .args
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect::<Vec<String>>();
    push_json_array(&mut line, "args", &arg_values);
    line.push(',');
    let mut env_arr = Vec::with_capacity(trace.envs.len());
    for &(key, value) in trace.envs {
        env_arr.push(format!("{key}={value}"));
    }
    push_json_array(&mut line, "env", &env_arr);
    line.push(',');
    push_json_bool(&mut line, "quiet", trace.quiet);
    line.push(',');
    push_json_bool(&mut line, "spawned", trace.spawned);
    line.push(',');
    push_json_u64(&mut line, "duration_ms", trace.duration_ms);
    line.push(',');
    if let Some(code) = trace.exit_code {
        let _wrote_i32 = write!(line, "\"{}\":{}", json_escape("exit_code"), code).is_ok();
    } else {
        push_json_opt_str(&mut line, "exit_code", None);
    }
    line.push(',');
    push_json_opt_str(
        &mut line,
        "stdout",
        trace.stdout.map(trace_text_limit).as_deref(),
    );
    line.push(',');
    push_json_opt_str(
        &mut line,
        "stderr",
        trace.stderr.map(trace_text_limit).as_deref(),
    );
    line.push(',');
    push_snapshot_fields(&mut line, "before", trace.before);
    line.push(',');
    push_snapshot_fields(&mut line, "after", trace.after);
    line.push('}');
    append_trace_line(ctx, &line);
}

/// Writes a note event into the trace log, if tracing is enabled.
pub(in crate::git_factor) fn trace_note(ctx: &Ctx<'_>, event: &str, fields: &[(&str, &str)]) {
    if trace_log_path(ctx).is_none() {
        return;
    }
    let snapshot = collect_repo_snapshot(ctx);
    let mut line = String::new();
    line.push('{');
    push_json_u64(&mut line, "ts_unix_ms", now_unix_ms());
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

/// Appends one best-effort line to the error-log buffer.
fn push_log_line(content: &mut String, args: Arguments<'_>) {
    let _ignored_write_result = content.write_fmt(args);
    let _ignored_newline_result = content.write_char('\n');
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "error-log formatting is isolated from control-flow handling"
    )
)]
#[expect(
    clippy::too_many_lines,
    reason = "error log intentionally emits a complete session snapshot in one place"
)]
/// Overwrites `.git/factor/error.log` with the latest unexpected session failure.
pub(in crate::git_factor) fn write_error_log(
    ctx: &Ctx<'_>,
    args: &[OsString],
    error: &FactorError,
) -> Result<(), FactorError> {
    let state_dir = factor_dir_in(ctx)?;
    let snapshot = collect_repo_snapshot(ctx);
    let trace_log = trace_log_path(ctx);
    let argv_words = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<String>>();
    let mut content = String::new();

    macro_rules! push_opt_line {
        ($key:literal, $value:expr) => {{
            if let Some(value) = $value {
                push_log_line(&mut content, format_args!("{}={value}", $key));
            }
        }};
    }

    push_log_line(&mut content, format_args!("ts_unix_ms={}", now_unix_ms()));
    push_log_line(&mut content, format_args!("argv={}", argv_words.join(" ")));
    push_log_line(&mut content, format_args!("cwd={}", ctx.cwd.display()));
    push_log_line(&mut content, format_args!("error={error}"));
    if let Some(path) = trace_log.as_ref() {
        push_log_line(&mut content, format_args!("trace_log={}", path.display()));
    }
    let mut maybe_source = error.source();
    let mut source_text = Vec::new();
    while let Some(cause) = maybe_source {
        source_text.push(cause.to_string());
        maybe_source = cause.source();
    }
    for (source_index, cause) in source_text.iter().enumerate() {
        push_log_line(&mut content, format_args!("source_{source_index}={cause}"));
    }

    push_opt_line!("head", snapshot.head.as_deref());
    push_opt_line!("head_tree", snapshot.head_tree.as_deref());
    push_opt_line!("git_dir", snapshot.git_dir.as_deref());
    push_opt_line!("toplevel", snapshot.toplevel.as_deref());
    push_opt_line!(
        "factor_current_commit",
        snapshot.factor_current_commit.as_deref()
    );
    push_opt_line!(
        "factor_current_index",
        snapshot
            .factor_current_index
            .map(|value| value.to_string())
            .as_deref()
    );
    push_opt_line!(
        "factor_split_count",
        snapshot
            .factor_split_count
            .map(|value| value.to_string())
            .as_deref()
    );
    push_opt_line!(
        "factor_requires_rebase",
        snapshot
            .factor_requires_rebase
            .map(|value| if value.as_bool() { "true" } else { "false" })
    );
    push_opt_line!(
        "factor_expected_tree",
        snapshot.factor_expected_tree.as_deref()
    );
    push_opt_line!(
        "rebase_state",
        snapshot.rebase_state.map(|state| match state {
            RebaseState::Apply => REBASE_APPLY_DIR,
            RebaseState::Merge => REBASE_MERGE_DIR,
        })
    );
    push_opt_line!(
        "rebase_msgnum",
        snapshot
            .rebase_msgnum
            .map(|value| value.as_u32().to_string())
            .as_deref()
    );
    push_opt_line!(
        "rebase_end",
        snapshot
            .rebase_end
            .map(|value| value.as_u32().to_string())
            .as_deref()
    );
    push_opt_line!("rebase_todo_head", snapshot.rebase_todo_head.as_deref());
    push_opt_line!("rebase_done_tail", snapshot.rebase_done_tail.as_deref());
    push_log_line(
        &mut content,
        format_args!("staged_paths={}", snapshot.staged_paths.join("\t")),
    );
    push_log_line(
        &mut content,
        format_args!("unstaged_paths={}", snapshot.unstaged_paths.join("\t")),
    );
    push_log_line(
        &mut content,
        format_args!("untracked_paths={}", snapshot.untracked_paths.join("\t")),
    );

    ctx.fs
        .write_string(&state_dir.as_path().join(ERROR_LOG_FILE), &content)
        .map_err(FactorError::StateWrite)
}
