//! UI tests for `#[expect_fail]` macro compile-time diagnostics.

#![forbid(unsafe_code)]
#![allow(
    clippy::all,
    clippy::pedantic,
    clippy::restriction,
    clippy::nursery,
    unfulfilled_lint_expectations,
    reason = "Temporary baseline for pre-existing lint debt; tighten in follow-up commits"
)]

#[test]
#[cfg_attr(
    any(coverage, coverage_nightly),
    ignore = "trybuild nested builds cause duplicate coverage objects/profile mismatches"
)]
fn ui_compile_fail_cases_report_expected_errors() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/expect-fail-*.rs");
}
