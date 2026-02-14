//! Integration tests for the temporary `#[expect_fail]` proc-macro shim.

#![forbid(unsafe_code)]
#![allow(
    clippy::all,
    clippy::pedantic,
    clippy::restriction,
    clippy::nursery,
    unfulfilled_lint_expectations,
    reason = "Temporary baseline for pre-existing lint debt; tighten in follow-up commits"
)]

use expect_fail_macro::expect_fail;

#[cfg(test)]
mod tests {
    use super::expect_fail;

    #[expect_fail(message = "intentional xfail panic")]
    #[expect(
        clippy::panic,
        reason = "Deliberately panics to verify exact-message xfail shim behavior"
    )]
    #[test]
    fn accepts_exact_message() {
        panic!("intentional xfail panic");
    }

    #[expect_fail(messager = "intentional alias panic")]
    #[expect(
        clippy::panic,
        reason = "Deliberately panics to verify exact-message xfail shim alias"
    )]
    #[test]
    fn accepts_messager_alias() {
        panic!("intentional alias panic");
    }

    #[expect_fail(message = "intentional owned string panic")]
    #[expect(
        clippy::panic,
        reason = "Deliberately panics with String payload to exercise xfail shim downcast path"
    )]
    #[test]
    fn accepts_string_payload() {
        panic!("{}", String::from("intentional owned string panic"));
    }

    #[expect_fail(message = "<non-string panic payload>")]
    #[test]
    fn accepts_non_string_payload() {
        std::panic::panic_any(123_u32);
    }
}
