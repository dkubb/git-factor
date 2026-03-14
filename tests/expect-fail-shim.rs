//! Integration tests for the temporary `#[expect_fail]` proc-macro shim.

#![forbid(unsafe_code)]

use expect_fail_macro::expect_fail;

#[cfg(test)]
mod tests {
    use super::expect_fail;
    use std::panic::resume_unwind;

    #[expect_fail(message = "intentional xfail panic")]
    #[test]
    fn accepts_exact_message() {
        resume_unwind(Box::new("intentional xfail panic"));
    }

    #[expect_fail(message = "intentional owned string panic")]
    #[test]
    fn accepts_string_payload() {
        resume_unwind(Box::new(String::from("intentional owned string panic")));
    }

    #[expect_fail(message = "<non-string panic payload>")]
    #[test]
    fn accepts_non_string_payload() {
        let payload: i32 = 123;
        resume_unwind(Box::new(payload));
    }
}
