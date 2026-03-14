//! Shared library modules for `git-factor` binaries.
//!
//! This crate exposes command implementations so thin binary entrypoints can
//! call into testable logic without duplicating orchestration code.

#![forbid(unsafe_code)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
extern crate alloc;

pub mod non_empty_string;
/// Shared test helper traits for consistent assertion patterns.
#[cfg(test)]
pub mod test_support;

/// Process exit code constants used by CLI binaries.
pub mod exit_codes;
/// `git-factor` command implementation.
pub mod git_factor;
/// `git-sequence-editor` command implementation.
pub mod git_sequence_editor;
