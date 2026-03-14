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
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "support modules land before the full factor engine is wired into them"
    )
)]
/// Core domain types for `git-factor`.
#[path = "git_factor/types.rs"]
mod types;

use crate::exit_codes::EXIT_OK;

/// Runs the `git-factor` CLI entrypoint.
///
/// The full factor workflow is added in later commits. This placeholder keeps
/// the binary wiring intact while the first domain types land.
#[must_use]
#[inline]
pub const fn main_entry() -> i32 {
    EXIT_OK
}
