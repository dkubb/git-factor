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
