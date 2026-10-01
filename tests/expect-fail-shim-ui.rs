//! UI tests for `#[expect_fail]` macro compile-time diagnostics.

#![forbid(unsafe_code)]

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "preserve the established inline test layout"
)]
mod tests {
    #[test]
    #[cfg_attr(
        any(coverage, coverage_nightly),
        ignore = "trybuild nested builds cause duplicate coverage objects/profile mismatches"
    )]
    fn ui_compile_fail_cases_report_expected_errors() {
        let test_cases = trybuild::TestCases::new();
        test_cases.compile_fail("tests/ui/expect-fail-*.rs");
    }
}
