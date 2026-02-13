//! Integration tests for the temporary `#[expect_fail]` proc-macro shim.

#![forbid(unsafe_code)]

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
}
