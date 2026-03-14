//! `git-factor` command implementation.

#![forbid(unsafe_code)]
#![expect(
    clippy::implicit_return,
    reason = "conflicts with `clippy::needless_return` from `clippy::all`"
)]

use crate::exit_codes::EXIT_OK;

/// Runs the `git-factor` CLI entrypoint.
///
/// The full factor workflow is added in later commits. This placeholder keeps
/// the binary wiring intact while the stricter workspace and test gates land
/// first.
#[must_use]
#[inline]
pub const fn main_entry() -> i32 {
    EXIT_OK
}
