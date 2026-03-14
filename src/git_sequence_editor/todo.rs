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
