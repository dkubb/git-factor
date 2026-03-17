use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use core::borrow::Borrow;
use core::cell::Cell;
use core::fmt;
use core::str::FromStr;
use std::process;
use thiserror::Error;

use crate::non_empty_string::NonEmptyString;
#[cfg(test)]
use crate::test_support::{OrAbort, ResultOrAbort};

use super::cli::Cli;

/// Exact length of a full hexadecimal SHA token.
pub(in crate::git_sequence_editor) const FULL_HEX_SHA_LEN: usize = 40;

/// Supported todo actions that can be enforced for a commit line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::git_sequence_editor) enum Action {
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

/// Canonical requested-action labels used in duplicate-action diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::git_sequence_editor) enum RequestedActionKind {
    /// `drop`.
    Drop,
    /// `edit`.
    Edit,
    /// `pick`.
    Pick,
}

impl RequestedActionKind {
    /// Returns the lowercase CLI label for this action kind.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Drop => "drop",
            Self::Edit => "edit",
            Self::Pick => "pick",
        }
    }
}

impl fmt::Display for RequestedActionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Typed domain error for todo parsing and rewrite validation.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub(in crate::git_sequence_editor) enum TodoError {
    /// A resolved full SHA matches multiple todo SHA prefixes.
    #[error("sha is ambiguous in todo: {sha}")]
    AmbiguousShaInTodo {
        /// SHA token that matches multiple todo lines.
        sha: String,
    },
    /// Duplicate SHA was provided for the same requested action.
    #[error("duplicate {label} sha: {sha}")]
    DuplicateRequestedActionSha {
        /// Requested action that had a duplicate SHA.
        label: RequestedActionKind,
        /// Duplicated SHA token.
        sha: String,
    },
    /// A SHA was specified multiple times across requested actions.
    #[error("sha specified multiple times: {sha}")]
    DuplicateRequestedSha {
        /// SHA token duplicated across requested actions.
        sha: String,
    },
    /// A SHA token was unexpectedly empty.
    #[error("internal error: todo sha was unexpectedly empty")]
    EmptyShaToken,
    /// `git rev-parse` exited with failure.
    #[error("git rev-parse failed (exit {code}): {stderr}")]
    GitRevParseFailed {
        /// Exit code from `git rev-parse`.
        code: i32,
        /// Captured stderr from `git rev-parse`.
        stderr: String,
    },
    /// Running `git rev-parse` failed before execution completed.
    #[error("failed to run git rev-parse: {message}")]
    GitRevParseSpawn {
        /// Spawn/OS error while trying to execute `git rev-parse`.
        message: String,
    },
    /// Factor-target arguments were incomplete or mismatched.
    #[error("invalid factor arguments: {message}")]
    InvalidFactorArguments {
        /// Human-readable validation detail.
        message: String,
    },
    /// A SHA token was present but not hexadecimal.
    #[error("invalid todo sha token: {token}")]
    InvalidShaToken {
        /// Raw SHA token parsed from todo content.
        token: String,
    },
    /// A requested SHA does not appear in the todo list.
    #[error("sha not present in todo: {sha}")]
    ShaNotPresentInTodo {
        /// Requested SHA token not found in todo entries.
        sha: String,
    },
    /// A todo action token is unsupported.
    #[error("unsupported todo action: {action}")]
    UnsupportedTodoAction {
        /// Unsupported action token parsed from the todo line.
        action: String,
    },
}

/// SHA token as it appears in the todo file.
///
/// This is intentionally opaque: we match what git wrote, and requested SHAs
/// must match exactly (after any `git rev-parse` normalization for full-length SHAs).
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::git_sequence_editor) struct TodoSha(NonEmptyString);

impl TodoSha {
    /// Returns this token as a string slice.
    pub(in crate::git_sequence_editor) const fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Constructs a `TodoSha` from a non-empty token.
    pub(in crate::git_sequence_editor) fn new(value: &str) -> Result<Self, TodoError> {
        let non_empty = match NonEmptyString::try_from(value.to_owned()) {
            Ok(non_empty) => non_empty,
            Err(_err) => return Err(TodoError::EmptyShaToken),
        };
        if !is_hex_token(non_empty.as_str()) {
            return Err(TodoError::InvalidShaToken {
                token: non_empty.to_string(),
            });
        }
        Ok(Self(non_empty))
    }
}

impl Borrow<str> for TodoSha {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for TodoSha {
    type Err = TodoError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

/// Exactly `FULL_HEX_SHA_LEN` hexadecimal characters.
#[derive(Clone, Debug, Eq, PartialEq)]
struct FullHexSha(TodoSha);

impl FullHexSha {
    /// Returns the wrapped SHA as a string slice.
    const fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<&str> for FullHexSha {
    type Error = TodoError;

    /// Parses a full-length SHA from a requested token.
    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        let non_empty = match NonEmptyString::try_from(raw.to_owned()) {
            Ok(non_empty) => non_empty,
            Err(_err) => {
                return Err(TodoError::ShaNotPresentInTodo {
                    sha: raw.to_owned(),
                });
            }
        };
        if non_empty.as_str().len() != FULL_HEX_SHA_LEN || !is_hex_token(non_empty.as_str()) {
            return Err(TodoError::ShaNotPresentInTodo {
                sha: raw.to_owned(),
            });
        }
        Ok(Self(TodoSha(non_empty)))
    }
}

/// Result of rewriting todo content.
pub(in crate::git_sequence_editor) struct RewriteTodoResult {
    /// Rewritten todo file contents.
    output: String,
    /// Warning messages emitted during rewrite.
    warnings: Vec<String>,
}

/// Extra lines to insert after a targeted factor commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::git_sequence_editor) struct FactorInsertion {
    /// Command that snapshots the green baseline into factor state.
    begin_command: NonEmptyString,
    /// Preflight command that must pass before beginning the split session.
    preflight_command: NonEmptyString,
}

impl FactorInsertion {
    /// Returns the begin command.
    const fn begin_command(&self) -> &NonEmptyString {
        &self.begin_command
    }

    /// Returns the preflight command.
    const fn preflight_command(&self) -> &NonEmptyString {
        &self.preflight_command
    }
}

impl RewriteTodoResult {
    /// Consumes and returns parts for tests and compatibility call sites.
    #[cfg(test)]
    pub(in crate::git_sequence_editor) fn into_parts(self) -> (String, Vec<String>) {
        (self.output, self.warnings)
    }

    /// Returns rewritten todo content.
    pub(in crate::git_sequence_editor) const fn output(&self) -> &str {
        self.output.as_str()
    }

    /// Returns rewrite warnings.
    pub(in crate::git_sequence_editor) const fn warnings(&self) -> &[String] {
        self.warnings.as_slice()
    }
}

/// Parsed todo action token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TodoActionKind {
    /// `break` / `b` action.
    Break,
    /// `drop` / `d` action.
    Drop,
    /// `edit` / `e` action.
    Edit,
    /// `exec` / `x` action.
    Exec,
    /// `fixup` / `f` action.
    Fixup,
    /// `label` / `l` action.
    Label,
    /// `merge` / `m` action.
    Merge,
    /// `noop` action.
    Noop,
    /// `pick` / `p` action.
    Pick,
    /// `reset` / `t` action.
    Reset,
    /// `reword` / `r` action.
    Reword,
    /// `squash` / `s` action.
    Squash,
    /// `update-ref` / `u` action.
    UpdateRef,
}

impl TodoActionKind {
    /// Parses long-form and short-form todo action tokens.
    fn parse(action: &str) -> Option<Self> {
        match action {
            "break" | "b" => Some(Self::Break),
            "drop" | "d" => Some(Self::Drop),
            "edit" | "e" => Some(Self::Edit),
            "exec" | "x" => Some(Self::Exec),
            "fixup" | "f" => Some(Self::Fixup),
            "label" | "l" => Some(Self::Label),
            "merge" | "m" => Some(Self::Merge),
            "noop" => Some(Self::Noop),
            "pick" | "p" => Some(Self::Pick),
            "reset" | "t" => Some(Self::Reset),
            "reword" | "r" => Some(Self::Reword),
            "squash" | "s" => Some(Self::Squash),
            "update-ref" | "u" => Some(Self::UpdateRef),
            _ => None,
        }
    }
}

/// Returns true if this parsed todo action carries a commit SHA token.
const fn is_commit_action_kind(action: TodoActionKind) -> bool {
    matches!(
        action,
        TodoActionKind::Drop
            | TodoActionKind::Edit
            | TodoActionKind::Fixup
            | TodoActionKind::Pick
            | TodoActionKind::Reword
            | TodoActionKind::Squash
    )
}

/// Returns true if `candidate` is a non-empty hexadecimal token.
fn is_hex_token(candidate: &str) -> bool {
    candidate
        .as_bytes()
        .iter()
        .copied()
        .all(|byte| byte.is_ascii_hexdigit())
}

/// Returns true if the string is exactly `FULL_HEX_SHA_LEN` hex characters (case-insensitive).
#[cfg(test)]
pub(in crate::git_sequence_editor) fn is_hex40(candidate: &str) -> bool {
    let bytes = candidate.as_bytes();
    bytes.len() == FULL_HEX_SHA_LEN && is_hex_token(candidate)
}

/// Returns the canonical long-form action for supported long/short actions.
#[cfg(test)]
fn canonical_action(action: &str) -> Option<&'static str> {
    let parsed = match TodoActionKind::parse(action) {
        Some(parsed) => parsed,
        None => return None,
    };
    Some(match parsed {
        TodoActionKind::Break => "break",
        TodoActionKind::Drop => "drop",
        TodoActionKind::Edit => "edit",
        TodoActionKind::Exec => "exec",
        TodoActionKind::Fixup => "fixup",
        TodoActionKind::Label => "label",
        TodoActionKind::Merge => "merge",
        TodoActionKind::Noop => "noop",
        TodoActionKind::Pick => "pick",
        TodoActionKind::Reset => "reset",
        TodoActionKind::Reword => "reword",
        TodoActionKind::Squash => "squash",
        TodoActionKind::UpdateRef => "update-ref",
    })
}

/// Returns true if the action denotes a commit line containing a SHA token.
#[cfg(test)]
fn is_commit_action(action: &str) -> bool {
    TodoActionKind::parse(action).is_some_and(is_commit_action_kind)
}

/// Returns true if the action is supported by this parser.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "isolated todo validator keeps parser behavior explicit"
    )
)]
pub(in crate::git_sequence_editor) fn validate_todo_format(content: &str) -> Result<(), TodoError> {
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let action_token = trimmed.split_whitespace().next().unwrap_or_default();
        let action = match TodoActionKind::parse(action_token) {
            Some(action) => action,
            None => {
                return Err(TodoError::UnsupportedTodoAction {
                    action: action_token.to_owned(),
                });
            }
        };
        if is_commit_action_kind(action)
            && let Some(sha) = trimmed.split_whitespace().nth(1)
            && !is_hex_token(sha)
        {
            return Err(TodoError::InvalidShaToken {
                token: sha.to_owned(),
            });
        }
    }

    Ok(())
}

/// Returns the commit-ish token from a todo line when present.
pub(in crate::git_sequence_editor) fn parse_todo_sha(line: &str) -> Option<&str> {
    let action = match parse_todo_action(line).and_then(TodoActionKind::parse) {
        Some(action) => action,
        None => return None,
    };
    if is_commit_action_kind(action) {
        line.split_whitespace().nth(1)
    } else {
        None
    }
}

/// Returns the action keyword token from a todo line when present.
pub(in crate::git_sequence_editor) fn parse_todo_action(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    trimmed.split_whitespace().next()
}

/// Errors if `shas` contains duplicates.
fn validate_no_duplicates(label: RequestedActionKind, shas: &[TodoSha]) -> Result<(), TodoError> {
    let mut set = BTreeSet::new();
    for sha in shas {
        if !set.insert(sha.as_str()) {
            return Err(TodoError::DuplicateRequestedActionSha {
                label,
                sha: sha.as_str().to_owned(),
            });
        }
    }
    Ok(())
}

// Test override hook for the `git` binary used by `resolve_requested_sha`.
// Defaults to `None` (use `"git"`). Unit tests set this thread-local override
// to exercise spawn-error branches deterministically.
thread_local! {
    static TEST_GIT_BIN_OVERRIDE: Cell<Option<&'static str>> = const { Cell::new(None) };
}

/// Resolves a requested SHA to the exact token used in the todo file.
pub(in crate::git_sequence_editor) fn resolve_requested_sha(
    requested: &str,
    todo_shas: &BTreeSet<TodoSha>,
) -> Result<TodoSha, TodoError> {
    let raw = requested;

    if let Some(matched) = todo_shas.get(raw) {
        return Ok(matched.clone());
    }

    let full_requested = match FullHexSha::try_from(requested) {
        Ok(full_requested) => full_requested,
        Err(err) => return Err(err),
    };

    let commit_ref = format!("{}^{{commit}}", full_requested.as_str());
    let git_bin = TEST_GIT_BIN_OVERRIDE.with(|override_bin| override_bin.get().unwrap_or("git"));
    let output_result = process::Command::new(git_bin)
        .args(["rev-parse", "--verify", "--quiet", &commit_ref])
        .output()
        .map_err(|err| TodoError::GitRevParseSpawn {
            message: err.to_string(),
        });
    let output = match output_result {
        Ok(command_output) => command_output,
        Err(err) => return Err(err),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(TodoError::GitRevParseFailed {
            code: output.status.code().unwrap_or(1),
            stderr: stderr.trim().to_owned(),
        });
    }

    let full = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if let Ok(full_sha) = FullHexSha::try_from(full.as_str())
        && let Some(matched) = todo_shas.get(full_sha.as_str())
    {
        return Ok(matched.clone());
    }

    // Find a unique todo SHA that is a prefix of the resolved full SHA.
    let mut candidates = Vec::<TodoSha>::new();
    for short in todo_shas {
        if full.starts_with(short.as_str()) {
            candidates.push(short.clone());
        }
    }

    match candidates.len() {
        0 => Err(TodoError::ShaNotPresentInTodo { sha: full }),
        1 => Ok(candidates.swap_remove(0)),
        _ => Err(TodoError::AmbiguousShaInTodo { sha: full }),
    }
}

/// Builds the requested action map, rejecting duplicates and contradictions.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "single helper centralizes duplicate/action conflict validation"
    )
)]
pub(in crate::git_sequence_editor) fn build_requested_actions(
    cli: &Cli,
    todo_shas: &BTreeSet<TodoSha>,
) -> Result<BTreeMap<TodoSha, Action>, TodoError> {
    if let Err(err) = validate_no_duplicates(RequestedActionKind::Pick, cli.pick()) {
        return Err(err);
    }
    if let Err(err) = validate_no_duplicates(RequestedActionKind::Edit, cli.edit()) {
        return Err(err);
    }
    if let Err(err) = validate_no_duplicates(RequestedActionKind::Drop, cli.drop()) {
        return Err(err);
    }

    let mut out = BTreeMap::<TodoSha, Action>::new();
    macro_rules! insert_requested {
        ($shas:expr, $action:expr) => {
            for sha in $shas {
                let resolved = match resolve_requested_sha(sha.as_str(), todo_shas) {
                    Ok(resolved) => resolved,
                    Err(err) => return Err(err),
                };
                if out.insert(resolved.clone(), $action).is_some() {
                    return Err(TodoError::DuplicateRequestedSha {
                        sha: resolved.as_str().to_owned(),
                    });
                }
            }
        };
    }
    insert_requested!(cli.pick(), Action::Pick);
    insert_requested!(cli.edit(), Action::Edit);
    insert_requested!(cli.drop(), Action::Drop);

    Ok(out)
}

/// Builds the optional factor-session insertion for one targeted todo SHA.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "factor insertion validation stays centralized in one rewrite pre-pass"
    )
)]
pub(in crate::git_sequence_editor) fn build_factor_insertion(
    cli: &Cli,
    todo_shas: &BTreeSet<TodoSha>,
) -> Result<Option<(TodoSha, FactorInsertion)>, TodoError> {
    let (target, preflight, begin) = match (
        cli.factor_target(),
        cli.factor_preflight(),
        cli.factor_begin(),
    ) {
        (None, None, None) => return Ok(None),
        (Some(target), Some(preflight), Some(begin)) => (target, preflight, begin),
        _ => {
            return Err(TodoError::InvalidFactorArguments {
                message: "factor-target, factor-preflight, and factor-begin must all be provided"
                    .to_owned(),
            });
        }
    };
    let resolved = match resolve_requested_sha(target.as_str(), todo_shas) {
        Ok(resolved) => resolved,
        Err(err) => return Err(err),
    };

    Ok(Some((
        resolved,
        FactorInsertion {
            begin_command: begin.clone(),
            preflight_command: preflight.clone(),
        },
    )))
}

/// Rewrites todo content and returns structured rewrite output.
#[cfg(test)]
pub(in crate::git_sequence_editor) fn rewrite_todo(
    content: &str,
    requested: &BTreeMap<TodoSha, Action>,
) -> RewriteTodoResult {
    rewrite_todo_with_factor(content, requested, None)
}

/// Rewrites todo content and returns structured rewrite output.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "single rewrite pass keeps todo transformation deterministic"
    )
)]
pub(in crate::git_sequence_editor) fn rewrite_todo_with_factor(
    content: &str,
    requested: &BTreeMap<TodoSha, Action>,
    factor_insertion: Option<&(TodoSha, FactorInsertion)>,
) -> RewriteTodoResult {
    let mut warnings = Vec::<String>::new();
    let mut output = String::with_capacity(content.len());

    for line in content.lines() {
        let Some(sha) = parse_todo_sha(line) else {
            output.push_str(line);
            output.push('\n');
            continue;
        };

        let requested_action = requested.get(sha);
        let factor_insertion_for_sha = factor_insertion.and_then(|pair| {
            let target_sha = &pair.0;
            let insertion = &pair.1;
            (target_sha.as_str() == sha).then_some(insertion)
        });

        let Some(requested_action_for_sha) = requested_action else {
            output.push_str(line);
            output.push('\n');
            append_factor_insertion(&mut output, factor_insertion_for_sha);
            continue;
        };

        let target_action = requested_action_for_sha.as_str();
        let target_kind = match *requested_action_for_sha {
            Action::Drop => TodoActionKind::Drop,
            Action::Edit => TodoActionKind::Edit,
            Action::Pick => TodoActionKind::Pick,
        };

        if parse_todo_action(line).and_then(TodoActionKind::parse) == Some(target_kind) {
            warnings.push(format!(
                "WARN: {sha}: requested '{target_action}', but todo already had '{target_action}'"
            ));
            output.push_str(line);
            output.push('\n');
            continue;
        }

        // Replace only the first action token and preserve the rest of the line as-is.
        let trimmed = line.trim_start();
        let old_action = trimmed.split_whitespace().next().unwrap_or_default();
        let rewritten = line.replacen(old_action, target_action, 1);
        output.push_str(&rewritten);
        output.push('\n');
        append_factor_insertion(&mut output, factor_insertion_for_sha);
    }

    RewriteTodoResult { output, warnings }
}

/// Appends the hidden factor rebase exec sequence for one targeted commit.
fn append_factor_insertion(output: &mut String, factor_insertion: Option<&FactorInsertion>) {
    if let Some(insertion) = factor_insertion {
        output.push_str("exec ");
        output.push_str(insertion.preflight_command().as_str());
        output.push('\n');
        output.push_str("exec ");
        output.push_str(insertion.begin_command().as_str());
        output.push('\n');
        output.push_str("break\n");
    }
}

/// Returns the set of commit tokens present in the todo file.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "single extractor keeps SHA token collection behavior centralized"
    )
)]
pub(in crate::git_sequence_editor) fn todo_shas_in(content: &str) -> BTreeSet<TodoSha> {
    let mut set = BTreeSet::<TodoSha>::new();
    for sha_tok in content
        .lines()
        .filter_map(parse_todo_sha)
        .filter_map(|todo_sha| TodoSha::new(todo_sha).ok())
    {
        let _inserted: bool = set.insert(sha_tok);
    }
    set
}

#[cfg(test)]
mod coverage_extra_tests {
    use alloc::collections::BTreeSet;
    use std::path::Path;

    use super::*;

    fn todo_sha(value: &str) -> TodoSha {
        TodoSha::new(value).or_abort("")
    }

    fn head_commit_sha() -> String {
        let output = process::Command::new("git")
            .args(["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])
            .output()
            .or_abort("");
        assert!(output.status.success(), "git rev-parse should succeed");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn uppercase_variant(input: &str) -> String {
        let mut changed = false;
        let mapped = input
            .chars()
            .map(|ch| {
                if !changed && ch.is_ascii_lowercase() {
                    changed = true;
                    ch.to_ascii_uppercase()
                } else {
                    ch
                }
            })
            .collect::<String>();
        assert!(
            changed,
            "expected at least one alphabetic hex character in commit SHA"
        );
        mapped
    }

    #[test]
    fn action_as_str_supports_drop() {
        assert_eq!(Action::Drop.as_str(), "drop");
    }

    #[test]
    fn uppercase_variant_preserves_non_lowercase_tail_chars() {
        assert_eq!(uppercase_variant("a1"), "A1");
    }

    #[test]
    fn uppercase_variant_supports_non_lowercase_prefix() {
        assert_eq!(uppercase_variant("1a"), "1A");
    }

    #[test]
    fn is_hex40_accepts_lowercase_hex() {
        let sha = "a".repeat(FULL_HEX_SHA_LEN);
        assert!(is_hex40(sha.as_str()));
    }

    #[test]
    fn resolve_requested_sha_returns_exact_short_sha_when_present() {
        let requested = "abc1234";
        let todo_shas = BTreeSet::from([TodoSha::new("abc1234").or_abort("")]);

        let resolved = resolve_requested_sha(requested, &todo_shas).or_abort("");

        assert_eq!(resolved.as_str(), "abc1234");
    }

    #[test]
    fn rewrite_todo_rewrites_action_when_requested_action_differs() {
        let todo = "  pick abc1234 first commit\n";
        let requested = BTreeMap::from([(TodoSha::new("abc1234").or_abort(""), Action::Edit)]);

        let (rewritten, warnings) = rewrite_todo(todo, &requested).into_parts();

        assert_eq!(rewritten, "  edit abc1234 first commit\n");
        assert!(warnings.is_empty(), "warnings were: {warnings:?}");
    }

    #[test]
    fn resolve_requested_sha_accepts_uppercase_full_sha_when_todo_has_lowercase_full_sha() {
        let full = head_commit_sha();
        let requested = uppercase_variant(full.as_str());
        let todo_shas = BTreeSet::from([TodoSha::new(full.as_str()).or_abort("")]);

        let resolved = resolve_requested_sha(requested.as_str(), &todo_shas).or_abort("");

        assert_eq!(resolved.as_str(), full);
    }

    #[test]
    fn resolve_requested_sha_matches_unique_short_prefix_from_resolved_full_sha() {
        let full = head_commit_sha();
        let short = full.get(..12).or_abort("");
        let requested = full.as_str();
        let todo_shas = BTreeSet::from([TodoSha::new(short).or_abort("")]);

        let resolved = resolve_requested_sha(requested, &todo_shas).or_abort("");

        assert_eq!(resolved.as_str(), short);
    }

    #[test]
    fn resolve_requested_sha_errors_when_no_todo_sha_prefix_matches_resolved_full_sha() {
        let full = head_commit_sha();
        let requested = full.as_str();
        let first = full.chars().next().unwrap_or('0');
        let replacement = char::from(b'0' + u8::from(first.eq_ignore_ascii_case(&'0')));
        let other = format!("{replacement}0000000");
        let todo_shas = BTreeSet::from([TodoSha::new(other.as_str()).or_abort("")]);

        let err = resolve_requested_sha(requested, &todo_shas).err_or_abort("");

        assert_eq!(err.to_string(), format!("sha not present in todo: {full}"));
    }

    #[test]
    fn git_rev_parse_verify_full_reports_spawn_error_when_binary_is_missing() {
        let requested = "a".repeat(FULL_HEX_SHA_LEN);
        let todo_shas = BTreeSet::new();
        TEST_GIT_BIN_OVERRIDE.with(|override_bin| override_bin.set(Some("git-does-not-exist")));
        let err = resolve_requested_sha(requested.as_str(), &todo_shas).err_or_abort("");
        TEST_GIT_BIN_OVERRIDE.with(|override_bin| override_bin.set(None));

        assert!(err.to_string().starts_with("failed to run git rev-parse:"));
    }

    #[test]
    fn resolve_requested_sha_errors_when_git_returns_non_sha_stdout() {
        let requested = "a".repeat(FULL_HEX_SHA_LEN);
        let todo_shas = BTreeSet::new();
        TEST_GIT_BIN_OVERRIDE.with(|override_bin| override_bin.set(Some("true")));
        let err = resolve_requested_sha(requested.as_str(), &todo_shas).err_or_abort("");
        TEST_GIT_BIN_OVERRIDE.with(|override_bin| override_bin.set(None));

        assert_eq!(err.to_string(), "sha not present in todo: ");
    }

    #[test]
    fn resolve_requested_sha_errors_when_multiple_todo_prefixes_match_resolved_full_sha() {
        let full = head_commit_sha();
        let requested = full.as_str();
        let short_a = full.get(..7).or_abort("");
        let short_b = full.get(..8).or_abort("");
        let todo_shas = BTreeSet::from([
            TodoSha::new(short_a).or_abort(""),
            TodoSha::new(short_b).or_abort(""),
        ]);

        let err = resolve_requested_sha(requested, &todo_shas).err_or_abort("");

        assert_eq!(err.to_string(), format!("sha is ambiguous in todo: {full}"));
    }

    #[test]
    fn build_requested_actions_supports_pick_edit_and_drop_without_conflicts() {
        let content = "\
pick aaaaaaa first\n\
pick bbbbbbb second\n\
pick ccccccc third\n\
";
        let todo_shas = todo_shas_in(content);
        let cli = Cli::for_tests(
            vec![todo_sha("ccccccc")],
            vec![todo_sha("bbbbbbb")],
            Path::new("todo").to_path_buf(),
            vec![todo_sha("aaaaaaa")],
        );

        let requested = build_requested_actions(&cli, &todo_shas).or_abort("");

        assert_eq!(requested.get("aaaaaaa"), Some(&Action::Pick));
        assert_eq!(requested.get("bbbbbbb"), Some(&Action::Edit));
        assert_eq!(requested.get("ccccccc"), Some(&Action::Drop));
    }

    #[test]
    fn build_requested_actions_rejects_pick_values_that_resolve_to_the_same_todo_sha() {
        let full = head_commit_sha();
        let short = full.get(..12).or_abort("");
        let todo_shas = BTreeSet::from([TodoSha::new(short).or_abort("")]);
        let cli = Cli::for_tests(
            vec![],
            vec![],
            Path::new("todo").to_path_buf(),
            vec![todo_sha(full.as_str()), todo_sha(short)],
        );

        let err = build_requested_actions(&cli, &todo_shas).err_or_abort("");

        assert_eq!(
            err.to_string(),
            format!("sha specified multiple times: {short}")
        );
    }

    #[test]
    fn build_requested_actions_rejects_edit_values_that_resolve_to_the_same_todo_sha() {
        let full = head_commit_sha();
        let short = full.get(..12).or_abort("");
        let todo_shas = BTreeSet::from([TodoSha::new(short).or_abort("")]);
        let cli = Cli::for_tests(
            vec![],
            vec![todo_sha(full.as_str()), todo_sha(short)],
            Path::new("todo").to_path_buf(),
            vec![],
        );

        let err = build_requested_actions(&cli, &todo_shas).err_or_abort("");

        assert_eq!(
            err.to_string(),
            format!("sha specified multiple times: {short}")
        );
    }

    #[test]
    fn build_requested_actions_rejects_duplicate_pick_arguments() {
        let todo_shas = BTreeSet::from([TodoSha::new("aaaaaaa").or_abort("")]);
        let cli = Cli::for_tests(
            vec![],
            vec![],
            Path::new("todo").to_path_buf(),
            vec![todo_sha("aaaaaaa"), todo_sha("aaaaaaa")],
        );

        let err = build_requested_actions(&cli, &todo_shas).err_or_abort("");

        assert_eq!(err.to_string(), "duplicate pick sha: aaaaaaa");
    }

    #[test]
    fn build_requested_actions_rejects_duplicate_edit_arguments() {
        let todo_shas = BTreeSet::from([TodoSha::new("bbbbbbb").or_abort("")]);
        let cli = Cli::for_tests(
            vec![],
            vec![todo_sha("bbbbbbb"), todo_sha("bbbbbbb")],
            Path::new("todo").to_path_buf(),
            vec![],
        );

        let err = build_requested_actions(&cli, &todo_shas).err_or_abort("");

        assert_eq!(err.to_string(), "duplicate edit sha: bbbbbbb");
    }

    #[test]
    fn build_requested_actions_propagates_pick_resolution_error() {
        let todo_shas = BTreeSet::from([TodoSha::new("aaaaaaa").or_abort("")]);
        let cli = Cli::for_tests(
            vec![],
            vec![],
            Path::new("todo").to_path_buf(),
            vec![todo_sha("deadbeef")],
        );

        let err = build_requested_actions(&cli, &todo_shas).err_or_abort("");

        assert_eq!(err.to_string(), "sha not present in todo: deadbeef");
    }

    #[test]
    fn build_requested_actions_propagates_edit_resolution_error() {
        let todo_shas = BTreeSet::from([TodoSha::new("aaaaaaa").or_abort("")]);
        let cli = Cli::for_tests(
            vec![],
            vec![todo_sha("deadbeef")],
            Path::new("todo").to_path_buf(),
            vec![],
        );

        let err = build_requested_actions(&cli, &todo_shas).err_or_abort("");

        assert_eq!(err.to_string(), "sha not present in todo: deadbeef");
    }

    #[test]
    fn build_requested_actions_propagates_drop_resolution_error() {
        let todo_shas = BTreeSet::from([TodoSha::new("aaaaaaa").or_abort("")]);
        let cli = Cli::for_tests(
            vec![todo_sha("deadbeef")],
            vec![],
            Path::new("todo").to_path_buf(),
            vec![],
        );

        let err = build_requested_actions(&cli, &todo_shas).err_or_abort("");

        assert_eq!(err.to_string(), "sha not present in todo: deadbeef");
    }

    #[test]
    fn build_factor_insertion_rejects_incomplete_factor_args() {
        let todo_shas = BTreeSet::from([TodoSha::new("aaaaaaa").or_abort("")]);
        let cli = Cli::for_tests_with_factor(
            Some(NonEmptyString::try_from("echo begin".to_owned()).or_abort("")),
            None,
            Some(todo_sha("aaaaaaa")),
            vec![],
            vec![],
            Path::new("todo").to_path_buf(),
            vec![],
        );

        let err = build_factor_insertion(&cli, &todo_shas).err_or_abort("");

        assert_eq!(
            err.to_string(),
            "invalid factor arguments: factor-target, factor-preflight, and factor-begin must all be provided"
        );
    }

    #[test]
    fn build_factor_insertion_resolves_full_target_sha() {
        let full = head_commit_sha();
        let short = full.get(..12).or_abort("");
        let todo_shas = BTreeSet::from([TodoSha::new(short).or_abort("")]);
        let cli = Cli::for_tests_with_factor(
            Some(NonEmptyString::try_from("echo begin".to_owned()).or_abort("")),
            Some(NonEmptyString::try_from("echo preflight".to_owned()).or_abort("")),
            Some(todo_sha(full.as_str())),
            vec![],
            vec![],
            Path::new("todo").to_path_buf(),
            vec![],
        );

        let (resolved, insertion) = build_factor_insertion(&cli, &todo_shas)
            .or_abort("")
            .or_abort("");

        assert_eq!(resolved.as_str(), short);
        assert_eq!(insertion.begin_command().as_str(), "echo begin");
        assert_eq!(insertion.preflight_command().as_str(), "echo preflight");
    }

    #[test]
    fn proptest_run_coverage_extra_suite() {
        action_as_str_supports_drop();
        build_factor_insertion_rejects_incomplete_factor_args();
        build_factor_insertion_resolves_full_target_sha();
        build_requested_actions_propagates_drop_resolution_error();
        build_requested_actions_propagates_edit_resolution_error();
        build_requested_actions_propagates_pick_resolution_error();
        build_requested_actions_rejects_duplicate_edit_arguments();
        build_requested_actions_rejects_duplicate_pick_arguments();
        build_requested_actions_rejects_edit_values_that_resolve_to_the_same_todo_sha();
        build_requested_actions_rejects_pick_values_that_resolve_to_the_same_todo_sha();
        build_requested_actions_supports_pick_edit_and_drop_without_conflicts();
        git_rev_parse_verify_full_reports_spawn_error_when_binary_is_missing();
        resolve_requested_sha_errors_when_git_returns_non_sha_stdout();
        is_hex40_accepts_lowercase_hex();
        resolve_requested_sha_accepts_uppercase_full_sha_when_todo_has_lowercase_full_sha();
        resolve_requested_sha_errors_when_multiple_todo_prefixes_match_resolved_full_sha();
        resolve_requested_sha_errors_when_no_todo_sha_prefix_matches_resolved_full_sha();
        resolve_requested_sha_matches_unique_short_prefix_from_resolved_full_sha();
        resolve_requested_sha_returns_exact_short_sha_when_present();
        rewrite_todo_rewrites_action_when_requested_action_differs();
        uppercase_variant_preserves_non_lowercase_tail_chars();
        uppercase_variant_supports_non_lowercase_prefix();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        OrAbort as _, ResultOrAbort as _, resolve_requested_sha, todo_shas_in, validate_todo_format,
    };
    use alloc::collections::BTreeSet;

    #[test]
    fn resolve_requested_sha_runs_git_rev_parse_for_full_sha() {
        let requested = "0".repeat(super::FULL_HEX_SHA_LEN);

        let err = resolve_requested_sha(requested.as_str(), &BTreeSet::new()).err_or_abort("");

        assert!(
            err.to_string().contains("git rev-parse failed"),
            "expected git rev-parse failure message, got: {err}"
        );
    }

    #[test]
    fn resolve_requested_sha_rejects_len_40_non_hex_without_rev_parse() {
        let requested = format!("{}g", "0".repeat(super::FULL_HEX_SHA_LEN - 1));

        let err = resolve_requested_sha(requested.as_str(), &BTreeSet::new()).err_or_abort("");

        assert_eq!(
            err.to_string(),
            format!("sha not present in todo: {requested}")
        );
    }

    #[test]
    fn resolve_requested_sha_rejects_non_40_len_sha_without_rev_parse() {
        let requested = "abc1234";

        let err = resolve_requested_sha(requested, &BTreeSet::new()).err_or_abort("");

        assert_eq!(
            err.to_string(),
            format!("sha not present in todo: {requested}")
        );
    }

    #[test]
    fn resolve_requested_sha_rejects_empty_sha_without_rev_parse() {
        let requested = "";

        let err = resolve_requested_sha(requested, &BTreeSet::new()).err_or_abort("");

        assert_eq!(
            err.to_string(),
            format!("sha not present in todo: {requested}")
        );
    }

    #[test]
    fn validate_todo_format_accepts_indented_comment_lines() {
        let content = "\n   # comment line\npick deadbeef message\n";
        assert_eq!(validate_todo_format(content), Ok(()));
    }

    #[test]
    fn proptest_validate_todo_format_accepts_supported_non_commit_action() {
        let content = "exec echo hello";
        assert_eq!(validate_todo_format(content), Ok(()));
    }

    #[test]
    fn proptest_validate_todo_format_accepts_commit_action_without_sha_token() {
        let content = "pick\n";
        assert_eq!(validate_todo_format(content), Ok(()));
    }

    #[test]
    fn todo_shas_in_deduplicates_repeated_tokens() {
        let sha = "0".repeat(super::FULL_HEX_SHA_LEN);
        let content = format!(
            "\
pick {sha} first
pick {sha} duplicate
"
        );

        let shas = todo_shas_in(content.as_str());
        assert_eq!(shas.len(), 1);
        assert!(
            shas.contains(sha.as_str()),
            "expected valid SHA to be present"
        );
    }

    #[test]
    fn option_or_abort_returns_inner_value() {
        assert_eq!(Some("value").or_abort(""), "value");
    }

    #[test]
    fn result_or_abort_returns_inner_value() {
        let value: Result<&str, &str> = Ok("value");
        assert_eq!(value.or_abort(""), "value");
    }

    #[test]
    fn result_err_or_abort_returns_inner_error() {
        let value: Result<&str, &str> = Err("error");
        assert_eq!(value.err_or_abort(""), "error");
    }

    #[test]
    fn proptest_run_unit_suite() {
        option_or_abort_returns_inner_value();
        result_err_or_abort_returns_inner_error();
        result_or_abort_returns_inner_value();
        resolve_requested_sha_rejects_len_40_non_hex_without_rev_parse();
        resolve_requested_sha_rejects_empty_sha_without_rev_parse();
        resolve_requested_sha_rejects_non_40_len_sha_without_rev_parse();
        resolve_requested_sha_runs_git_rev_parse_for_full_sha();
        todo_shas_in_deduplicates_repeated_tokens();
        validate_todo_format_accepts_indented_comment_lines();
    }
}

#[cfg(test)]
mod proptests {
    use alloc::collections::{BTreeMap, BTreeSet};

    use proptest::collection::vec;
    use proptest::prelude::*;
    use proptest::string::string_regex;

    use super::*;

    fn supported_action_broad() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("pick".to_owned()),
            Just("p".to_owned()),
            Just("reword".to_owned()),
            Just("r".to_owned()),
            Just("edit".to_owned()),
            Just("e".to_owned()),
            Just("squash".to_owned()),
            Just("s".to_owned()),
            Just("fixup".to_owned()),
            Just("f".to_owned()),
            Just("drop".to_owned()),
            Just("d".to_owned()),
            Just("exec".to_owned()),
            Just("x".to_owned()),
            Just("break".to_owned()),
            Just("b".to_owned()),
            Just("label".to_owned()),
            Just("l".to_owned()),
            Just("reset".to_owned()),
            Just("t".to_owned()),
            Just("merge".to_owned()),
            Just("m".to_owned()),
            Just("noop".to_owned()),
            Just("update-ref".to_owned()),
            Just("u".to_owned()),
        ]
    }

    fn supported_action_biased() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("pick".to_owned()),
            Just("edit".to_owned()),
            Just("drop".to_owned()),
            Just("reword".to_owned()),
        ]
    }

    fn supported_action() -> impl Strategy<Value = String> {
        prop_oneof![1 => supported_action_broad(), 4 => supported_action_biased()]
    }

    fn commit_action() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("pick".to_owned()),
            Just("p".to_owned()),
            Just("reword".to_owned()),
            Just("r".to_owned()),
            Just("edit".to_owned()),
            Just("e".to_owned()),
            Just("squash".to_owned()),
            Just("s".to_owned()),
            Just("fixup".to_owned()),
            Just("f".to_owned()),
            Just("drop".to_owned()),
            Just("d".to_owned()),
        ]
    }

    fn non_commit_action() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("exec".to_owned()),
            Just("x".to_owned()),
            Just("break".to_owned()),
            Just("b".to_owned()),
            Just("label".to_owned()),
            Just("l".to_owned()),
            Just("reset".to_owned()),
            Just("t".to_owned()),
            Just("merge".to_owned()),
            Just("m".to_owned()),
            Just("noop".to_owned()),
            Just("update-ref".to_owned()),
            Just("u".to_owned()),
        ]
    }

    fn non_whitespace_token() -> impl Strategy<Value = String> {
        string_regex("[!-~]{1,24}").or_abort("")
    }

    fn action_token() -> impl Strategy<Value = String> {
        string_regex("[A-Za-z][A-Za-z0-9._/-]{0,23}").or_abort("")
    }

    fn optional_tail() -> impl Strategy<Value = String> {
        string_regex("[A-Za-z0-9._/-]{0,20}").or_abort("")
    }

    fn indent() -> impl Strategy<Value = String> {
        string_regex("[ ]{0,4}").or_abort("")
    }

    fn hex40_valid_broad() -> impl Strategy<Value = String> {
        let pattern = format!("[0-9A-Fa-f]{{{FULL_HEX_SHA_LEN}}}");
        string_regex(pattern.as_str()).or_abort("")
    }

    fn hex40_valid_biased() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("0".repeat(FULL_HEX_SHA_LEN)),
            Just("f".repeat(FULL_HEX_SHA_LEN)),
            Just("A".repeat(FULL_HEX_SHA_LEN)),
        ]
    }

    fn hex40_valid() -> impl Strategy<Value = String> {
        prop_oneof![1 => hex40_valid_broad(), 4 => hex40_valid_biased()]
    }

    fn hex40_invalid_broad() -> impl Strategy<Value = String> {
        let short_pattern = format!("[0-9A-Fa-f]{{0,{}}}", FULL_HEX_SHA_LEN - 1);
        let long_pattern = format!(
            "[0-9A-Fa-f]{{{},{}}}",
            FULL_HEX_SHA_LEN + 1,
            FULL_HEX_SHA_LEN * 2
        );
        let wrong_length = prop_oneof![
            string_regex(short_pattern.as_str()).or_abort(""),
            string_regex(long_pattern.as_str()).or_abort(""),
        ];
        let non_hex_pattern = format!("[ -~]{{{FULL_HEX_SHA_LEN}}}");
        let non_hex = string_regex(non_hex_pattern.as_str())
            .or_abort("")
            .prop_filter("must contain a non-hex byte", |candidate| {
                candidate.bytes().any(|byte| !byte.is_ascii_hexdigit())
            });

        prop_oneof![wrong_length, non_hex]
    }

    fn hex40_invalid_biased() -> impl Strategy<Value = String> {
        prop_oneof![
            Just(String::new()),
            Just("0".repeat(FULL_HEX_SHA_LEN - 1)),
            Just("0".repeat(FULL_HEX_SHA_LEN + 1)),
            Just(format!("{}g", "0".repeat(FULL_HEX_SHA_LEN - 1))),
        ]
    }

    fn hex40_invalid() -> impl Strategy<Value = String> {
        prop_oneof![1 => hex40_invalid_broad(), 4 => hex40_invalid_biased()]
    }

    fn rewrite_change_case() -> impl Strategy<Value = (&'static str, Action)> {
        prop_oneof![
            Just(("pick", Action::Edit)),
            Just(("pick", Action::Drop)),
            Just(("edit", Action::Pick)),
            Just(("edit", Action::Drop)),
            Just(("drop", Action::Pick)),
            Just(("drop", Action::Edit)),
        ]
    }

    fn rewrite_same_case() -> impl Strategy<Value = (&'static str, Action)> {
        prop_oneof![
            Just(("pick", Action::Pick)),
            Just(("p", Action::Pick)),
            Just(("edit", Action::Edit)),
            Just(("e", Action::Edit)),
            Just(("drop", Action::Drop)),
            Just(("d", Action::Drop)),
        ]
    }

    proptest! {
        #[test]
        fn proptest_todo_sha_new_accepts_hex_tokens(token in string_regex(format!("[0-9A-Fa-f]{{1,{FULL_HEX_SHA_LEN}}}").as_str()).or_abort("")) {
            let sha = TodoSha::new(token.as_str()).or_abort("");
            prop_assert_eq!(sha.as_str(), token);
        }

        #[test]
        fn proptest_todo_sha_new_rejects_non_hex_tokens(token in non_whitespace_token()) {
            prop_assume!(!is_hex_token(token.as_str()));

            let err = TodoSha::new(token.as_str()).err_or_abort("");

            prop_assert_eq!(err.to_string(), format!("invalid todo sha token: {token}"));
        }

        #[test]
        fn proptest_is_hex40_accepts_valid_hex(value in hex40_valid()) {
            prop_assert!(is_hex40(value.as_str()));
        }

        #[test]
        fn proptest_is_hex40_rejects_invalid_hex(value in hex40_invalid()) {
            prop_assert!(!is_hex40(value.as_str()));
        }

        #[test]
        fn proptest_parse_todo_action_returns_first_token(indent in indent(), action in action_token(), tail in optional_tail()) {
            let line = if tail.is_empty() {
                format!("{indent}{action}")
            } else {
                format!("{indent}{action} {tail}")
            };

            prop_assert_eq!(parse_todo_action(line.as_str()), Some(action.as_str()));
        }

        #[test]
        fn proptest_parse_todo_action_ignores_blank_and_comment_lines(indent in indent(), comment in optional_tail()) {
            let blank = indent.clone();
            let comment_line = format!("{indent}#{comment}");

            prop_assert_eq!(parse_todo_action(blank.as_str()), None);
            prop_assert_eq!(parse_todo_action(comment_line.as_str()), None);
        }

        #[test]
        fn proptest_parse_todo_sha_extracts_sha_for_commit_actions(action in commit_action(), sha in non_whitespace_token(), tail in optional_tail()) {
            let line = if tail.is_empty() {
                format!("{action} {sha}")
            } else {
                format!("{action} {sha} {tail}")
            };

            prop_assert_eq!(parse_todo_sha(line.as_str()), Some(sha.as_str()));
        }

        #[test]
        fn proptest_parse_todo_sha_rejects_non_commit_actions(action in non_commit_action(), sha in non_whitespace_token(), tail in optional_tail()) {
            let line = if tail.is_empty() {
                format!("{action} {sha}")
            } else {
                format!("{action} {sha} {tail}")
            };

            prop_assert_eq!(parse_todo_sha(line.as_str()), None);
        }

        #[test]
        fn proptest_validate_todo_format_accepts_supported_actions(lines in vec((supported_action(), non_whitespace_token(), optional_tail()), 1..16)) {
            let mut content = String::new();
            for (action, sha, tail) in lines {
                if is_commit_action(action.as_str()) {
                    let commit_sha = if is_hex_token(sha.as_str()) {
                        sha
                    } else {
                        "deadbeef".to_owned()
                    };
                    if tail.is_empty() {
                        content.push_str(format!("{action} {commit_sha}\n").as_str());
                    } else {
                        content.push_str(format!("{action} {commit_sha} {tail}\n").as_str());
                    }
                } else if tail.is_empty() {
                    content.push_str(format!("{action}\n").as_str());
                } else {
                    content.push_str(format!("{action} {tail}\n").as_str());
                }
            }

            prop_assert!(validate_todo_format(content.as_str()).is_ok());
        }

        #[test]
        fn proptest_validate_todo_format_rejects_unsupported_actions(suffix in string_regex("[a-z]{0,12}").or_abort("")) {
            let action = format!("zz{suffix}");
            let line = format!("{action} abcdef0 message");
            let err = validate_todo_format(line.as_str()).err_or_abort("");

            prop_assert_eq!(err.to_string(), format!("unsupported todo action: {action}"));
        }

        #[test]
        fn proptest_validate_todo_format_rejects_non_hex_commit_sha(action in commit_action(), tail in optional_tail()) {
            let invalid_sha = "not_hex_sha";
            let line = if tail.is_empty() {
                format!("{action} {invalid_sha}")
            } else {
                format!("{action} {invalid_sha} {tail}")
            };
            let err = validate_todo_format(line.as_str()).err_or_abort("");

            prop_assert_eq!(err.to_string(), format!("invalid todo sha token: {invalid_sha}"));
        }

        #[test]
        fn proptest_rewrite_todo_rewrites_only_the_action_token((current, target) in rewrite_change_case(), indent in indent(), sha in string_regex("[0-9a-f]{7,12}").or_abort(""), tail in optional_tail()) {
            let line = if tail.is_empty() {
                format!("{indent}{current} {sha}")
            } else {
                format!("{indent}{current} {sha} {tail}")
            };
            let todo_sha = TodoSha::new(sha.as_str()).or_abort("");
            let requested = BTreeMap::from([(todo_sha, target)]);

            let (rewritten, warnings) = rewrite_todo(line.as_str(), &requested).into_parts();

            let trimmed = line.trim_start();
            let indent_len = line
                .len()
                .checked_sub(trimmed.len())
                .or_abort("");
            let (indent_prefix, rest) = line.split_at(indent_len);
            let rest_without_action = rest
                .strip_prefix(current)
                .or_abort("");
            let expected = format!("{indent_prefix}{}{rest_without_action}\n", target.as_str());

            prop_assert!(warnings.is_empty());
            prop_assert_eq!(rewritten, expected);
        }

        #[test]
        fn proptest_rewrite_todo_warns_when_action_is_already_requested((current, target) in rewrite_same_case(), indent in indent(), sha in string_regex("[0-9a-f]{7,12}").or_abort(""), tail in optional_tail()) {
            let line = if tail.is_empty() {
                format!("{indent}{current} {sha}")
            } else {
                format!("{indent}{current} {sha} {tail}")
            };
            let todo_sha = TodoSha::new(sha.as_str()).or_abort("");
            let requested = BTreeMap::from([(todo_sha, target)]);
            let target_action = target.as_str();

            let (rewritten, warnings) = rewrite_todo(line.as_str(), &requested).into_parts();

            prop_assert_eq!(rewritten, format!("{line}\n"));
            prop_assert_eq!(
                warnings,
                vec![format!(
                    "WARN: {sha}: requested '{target_action}', but todo already had '{target_action}'"
                )]
            );
        }

        #[test]
        fn proptest_todo_shas_in_collects_unique_commit_tokens(lines in vec((commit_action(), string_regex("[0-9a-f]{7,12}").or_abort(""), optional_tail()), 1..24)) {
            let mut content = String::new();
            let mut expected = BTreeSet::<String>::new();
            for (action, sha, tail) in lines {
                let _inserted = expected.insert(sha.clone());
                if tail.is_empty() {
                    content.push_str(format!("{action} {sha}\n").as_str());
                } else {
                    content.push_str(format!("{action} {sha} {tail}\n").as_str());
                }
            }

            let actual = todo_shas_in(content.as_str())
                .into_iter()
                .map(|todo_sha| todo_sha.as_str().to_owned())
                .collect::<BTreeSet<_>>();

            prop_assert_eq!(actual, expected);
        }
    }

    proptest! {
        #[test]
        fn proptest_supported_action_broad_yields_supported(action in supported_action_broad()) {
            prop_assert!(canonical_action(action.as_str()).is_some());
        }

        #[test]
        fn proptest_supported_action_biased_yields_supported(action in supported_action_biased()) {
            prop_assert!(canonical_action(action.as_str()).is_some());
        }

        #[test]
        fn proptest_supported_action_generator_yields_supported(action in supported_action()) {
            prop_assert!(canonical_action(action.as_str()).is_some());
        }

        #[test]
        fn proptest_non_commit_action_generator_yields_non_commit_actions(action in non_commit_action()) {
            prop_assert!(!is_commit_action(action.as_str()));
        }

        #[test]
        fn proptest_action_token_generator_yields_non_empty_tokens(token in action_token()) {
            prop_assert!(!token.is_empty());
        }

        #[test]
        fn proptest_hex40_valid_generators_yield_hex(value in hex40_valid()) {
            prop_assert!(is_hex40(value.as_str()));
        }

        #[test]
        fn proptest_hex40_valid_broad_generator_yields_hex(value in hex40_valid_broad()) {
            prop_assert!(is_hex40(value.as_str()));
        }

        #[test]
        fn proptest_hex40_valid_biased_generator_yields_hex(value in hex40_valid_biased()) {
            prop_assert!(is_hex40(value.as_str()));
        }

        #[test]
        fn proptest_hex40_invalid_generators_yield_non_hex(value in hex40_invalid()) {
            prop_assert!(!is_hex40(value.as_str()));
        }

        #[test]
        fn proptest_hex40_invalid_broad_generator_yields_non_hex(value in hex40_invalid_broad()) {
            prop_assert!(!is_hex40(value.as_str()));
        }

        #[test]
        fn proptest_hex40_invalid_biased_generator_yields_non_hex(value in hex40_invalid_biased()) {
            prop_assert!(!is_hex40(value.as_str()));
        }

        #[test]
        fn proptest_rewrite_change_case_generator_produces_different_actions((current, target) in rewrite_change_case()) {
            let target_action = target.as_str();
            prop_assert!(target_action == "pick" || target_action == "edit" || target_action == "drop");
            prop_assert_ne!(current, target_action);
        }

        #[test]
        fn proptest_rewrite_same_case_generator_produces_matching_actions((current, target) in rewrite_same_case()) {
            let target_action = target.as_str();
            prop_assert!(
                (target_action == "pick" && (current == "pick" || current == "p"))
                    || (target_action == "edit" && (current == "edit" || current == "e"))
                    || (target_action == "drop" && (current == "drop" || current == "d"))
            );
        }
    }

    #[test]
    fn proptest_canonical_action_rejects_unknown_action() {
        assert_eq!(canonical_action("zz"), None);
    }

    #[test]
    fn proptest_canonical_action_marks_commit_actions_correctly() {
        let commit_actions = [
            "pick", "p", "reword", "r", "edit", "e", "squash", "s", "fixup", "f", "drop", "d",
        ];
        let non_commit_actions = [
            "exec",
            "x",
            "break",
            "b",
            "label",
            "l",
            "reset",
            "t",
            "merge",
            "m",
            "noop",
            "update-ref",
            "u",
        ];

        for action in commit_actions {
            assert!(
                canonical_action(action).is_some(),
                "action should be supported: {action}"
            );
            assert!(
                is_commit_action(action),
                "action should be commit action: {action}"
            );
        }
        for action in non_commit_actions {
            assert!(
                canonical_action(action).is_some(),
                "action should be supported: {action}"
            );
            assert!(
                !is_commit_action(action),
                "action should be non-commit action: {action}"
            );
        }
    }
}
