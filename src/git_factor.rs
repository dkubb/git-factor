//! `git-factor` is a `git` subcommand for splitting a large commit into smaller,
//! atomic commits using interactive rebase.
//!
//! After installing (for example with `cargo install --path .`), run it as:
//! `git factor ...` (git will resolve this to the `git-factor` binary).

#![forbid(unsafe_code)]
#![expect(
    clippy::implicit_return,
    reason = "conflicts with `clippy::needless_return` from `clippy::all`"
)]
#![expect(
    dead_code,
    reason = "support modules land before the full factor engine is wired into them"
)]

/// CLI argument model for `git-factor`.
#[path = "git_factor/cli.rs"]
mod cli;
/// Error model for `git-factor`.
#[path = "git_factor/error.rs"]
mod error;
/// Core domain types for `git-factor`.
#[path = "git_factor/types.rs"]
mod types;

use crate::exit_codes::EXIT_OK;

use self::error::{FactorError, non_empty_msg};

/// Runs the `git-factor` CLI entrypoint.
///
/// The full factor workflow is added in later commits. This placeholder keeps
/// the binary wiring intact while the core CLI and domain support land first.
#[must_use]
#[inline]
pub const fn main_entry() -> i32 {
    EXIT_OK
}
