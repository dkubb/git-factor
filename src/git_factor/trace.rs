use core::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

/// Environment variable enabling JSONL trace logging.
pub(in crate::git_factor) const TRACE_LOG_ENV: &str = "GIT_FACTOR_TRACE_LOG";

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
