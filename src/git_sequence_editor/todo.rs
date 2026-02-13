use core::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::process;

use non_empty_string::NonEmptyString;

use super::cli::Cli;

/// Supported todo actions that can be enforced for a commit line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Action {
    /// Skip the commit.
    Drop,
    /// Stop at the commit for editing.
    Edit,
    /// Apply the commit normally.
    Pick,
}

impl Action {
    /// Returns the rebase-todo keyword for this action.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Drop => "drop",
            Self::Edit => "edit",
            Self::Pick => "pick",
        }
    }
}

/// SHA token as it appears in the todo file.
///
/// This is intentionally opaque: we match what git wrote, and requested SHAs
/// must match exactly (after any `git rev-parse` normalization for 40-hex SHAs).
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct TodoSha(NonEmptyString);

impl TodoSha {
    /// Returns this token as a string slice.
    pub(super) fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Constructs a `TodoSha` from a non-empty token.
    pub(super) fn new(value: &str) -> Result<Self, String> {
        NonEmptyString::try_from(value.to_owned())
            .map(Self)
            .map_err(|_err| "internal error: todo sha was unexpectedly empty".to_owned())
    }
}

impl Borrow<str> for TodoSha {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

/// Returns true if the byte is a lowercase hex digit.
const fn is_hex_lower(ch: u8) -> bool {
    matches!(ch, b'0'..=b'9' | b'a'..=b'f')
}

/// Returns true if the string is exactly 40 hex characters (case-insensitive).
pub(super) fn is_hex40(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 40
        && bytes
            .iter()
            .copied()
            .all(|ch| is_hex_lower(ch) || matches!(ch, b'A'..=b'F'))
}

/// Returns true if the action denotes a commit line containing a SHA token.
fn is_commit_action(action: &str) -> bool {
    matches!(
        action,
        "pick" | "reword" | "edit" | "squash" | "fixup" | "drop"
    )
}

/// Returns true if the action is supported by this parser.
fn is_supported_action(action: &str) -> bool {
    // Full `git-rebase-todo` action set (we may expand as git adds more).
    // Non-commit actions do not have a SHA token.
    is_commit_action(action)
        || matches!(
            action,
            "exec" | "break" | "label" | "reset" | "merge" | "noop" | "update-ref"
        )
}

/// Validates that every non-comment line starts with a supported action.
pub(super) fn validate_todo_format(content: &str) -> Result<(), String> {
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let action = trimmed.split_whitespace().next().unwrap_or_default();
        if !is_supported_action(action) {
            return Err(format!("unsupported todo action: {action}"));
        }
    }

    Ok(())
}

/// Returns the commit-ish token from a todo line when present.
pub(super) fn parse_todo_sha(line: &str) -> Option<&str> {
    let action = parse_todo_action(line)?;
    if is_commit_action(action) {
        line.split_whitespace().nth(1)
    } else {
        None
    }
}

/// Returns the action keyword token from a todo line when present.
pub(super) fn parse_todo_action(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    trimmed.split_whitespace().next()
}

/// Errors if `shas` contains duplicates.
fn validate_no_duplicates(label: &str, shas: &[NonEmptyString]) -> Result<(), String> {
    let mut set = BTreeSet::new();
    for sha in shas {
        if !set.insert(sha.as_str()) {
            return Err(format!("duplicate {label} sha: {}", sha.as_str()));
        }
    }
    Ok(())
}

/// Resolves a 40-hex commit reference using `git rev-parse --verify`.
fn git_rev_parse_verify_full(sha40: &str) -> Result<String, String> {
    let output = process::Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", sha40])
        .output()
        .map_err(|err| format!("failed to run git rev-parse: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "git rev-parse failed (exit {}): {}",
            output.status.code().unwrap_or(1),
            stderr.trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Resolves a requested SHA to the exact token used in the todo file.
pub(super) fn resolve_requested_sha(
    requested: &NonEmptyString,
    todo_shas: &BTreeSet<TodoSha>,
) -> Result<TodoSha, String> {
    let raw = requested.as_str();

    if todo_shas.contains(raw) {
        return TodoSha::new(raw);
    }

    // Only accept 40-char hex SHAs for the "long SHA" path.
    if !is_hex40(raw) {
        return Err(format!("sha not present in todo: {raw}"));
    }

    let full = git_rev_parse_verify_full(raw)?;
    if todo_shas.contains(full.as_str()) {
        return TodoSha::new(full.as_str());
    }

    // Find a unique todo SHA that is a prefix of the resolved full SHA.
    let matches = todo_shas
        .iter()
        .filter(|short| full.starts_with(short.as_str()))
        .cloned()
        .collect::<Vec<TodoSha>>();

    if matches.is_empty() {
        return Err(format!("sha not present in todo: {full}"));
    }
    if matches.len() > 1 {
        return Err(format!("sha is ambiguous in todo: {full}"));
    }

    Ok(matches
        .into_iter()
        .next()
        .expect("non-empty matches after emptiness check"))
}

/// Builds the requested action map, rejecting duplicates and contradictions.
pub(super) fn build_requested_actions(
    cli: &Cli,
    todo_shas: &BTreeSet<TodoSha>,
) -> Result<BTreeMap<TodoSha, Action>, String> {
    validate_no_duplicates("pick", cli.pick())?;
    validate_no_duplicates("edit", cli.edit())?;
    validate_no_duplicates("drop", cli.drop())?;

    let mut out = BTreeMap::<TodoSha, Action>::new();

    for sha in cli.pick() {
        let resolved = resolve_requested_sha(sha, todo_shas)?;
        if out.insert(resolved.clone(), Action::Pick).is_some() {
            return Err(format!(
                "sha specified multiple times: {}",
                resolved.as_str()
            ));
        }
    }
    for sha in cli.edit() {
        let resolved = resolve_requested_sha(sha, todo_shas)?;
        if out.insert(resolved.clone(), Action::Edit).is_some() {
            return Err(format!(
                "sha specified multiple times: {}",
                resolved.as_str()
            ));
        }
    }
    for sha in cli.drop() {
        let resolved = resolve_requested_sha(sha, todo_shas)?;
        if out.insert(resolved.clone(), Action::Drop).is_some() {
            return Err(format!(
                "sha specified multiple times: {}",
                resolved.as_str()
            ));
        }
    }

    Ok(out)
}

/// Rewrites todo content and returns (`new_content`, warnings).
pub(super) fn rewrite_todo(
    content: &str,
    requested: &BTreeMap<TodoSha, Action>,
) -> (String, Vec<String>) {
    let mut warnings = Vec::<String>::new();
    let mut output = String::with_capacity(content.len());

    for line in content.lines() {
        let Some(sha) = parse_todo_sha(line) else {
            output.push_str(line);
            output.push('\n');
            continue;
        };

        let Some(target) = requested.get(sha) else {
            output.push_str(line);
            output.push('\n');
            continue;
        };

        let current_action = parse_todo_action(line).unwrap_or_default();
        let target_action = target.as_str();

        if current_action == target_action {
            warnings.push(format!(
                "WARN: {sha}: requested '{target_action}', but todo already had '{target_action}'"
            ));
            output.push_str(line);
            output.push('\n');
            continue;
        }

        // Replace only the leading action token to preserve spacing and the rest of the line.
        let trimmed = line.trim_start();
        let old_action = trimmed.split_whitespace().next().unwrap_or_default();

        // Keep any leading indentation exactly as-is.
        let indent_len = line
            .len()
            .checked_sub(trimmed.len())
            .expect("internal invariant: trim_start result cannot exceed input length");
        let (indent, rest) = line.split_at(indent_len);
        let rest_without_action = rest.strip_prefix(old_action).unwrap_or(rest);
        let rewritten = format!("{indent}{target_action}{rest_without_action}");
        output.push_str(&rewritten);
        output.push('\n');
    }

    (output, warnings)
}

/// Returns the set of commit tokens present in the todo file.
pub(super) fn todo_shas_in(content: &str) -> BTreeSet<TodoSha> {
    let mut set = BTreeSet::<TodoSha>::new();
    for todo_sha in content.lines().filter_map(parse_todo_sha) {
        let sha_tok = TodoSha::new(todo_sha)
            .expect("internal invariant: split_whitespace token should be non-empty");
        let _inserted: bool = set.insert(sha_tok);
    }
    set
}
