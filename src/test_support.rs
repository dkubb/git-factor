use core::fmt::Debug;

/// Unwraps `Ok(T)` or panics with a test-invariant message.
pub trait OrAbort<T> {
    /// Unwraps the value or panics with a test-invariant message.
    fn or_abort<M>(self, _message: M) -> T;
}

impl<T, E: Debug> OrAbort<T> for Result<T, E> {
    #[expect(
        clippy::implicit_return,
        reason = "test-only helper is intentionally a single expression"
    )]
    #[expect(
        clippy::expect_used,
        reason = "test-only helper enforces setup invariants with explicit failure context"
    )]
    #[inline]
    fn or_abort<M>(self, _message: M) -> T {
        self.expect("test invariant violated: expected Ok(..)")
    }
}

impl<T> OrAbort<T> for Option<T> {
    #[expect(
        clippy::implicit_return,
        reason = "test-only helper is intentionally a single expression"
    )]
    #[expect(
        clippy::expect_used,
        reason = "test-only helper enforces setup invariants with explicit failure context"
    )]
    #[inline]
    fn or_abort<M>(self, _message: M) -> T {
        self.expect("test invariant violated: expected Some(..)")
    }
}

/// Unwraps `Err(E)` or panics with a test-invariant message.
pub trait ResultOrAbort<T, E> {
    /// Unwraps the error or panics with a test-invariant message.
    fn err_or_abort<M>(self, _message: M) -> E;
}

impl<T: Debug, E> ResultOrAbort<T, E> for Result<T, E> {
    #[expect(
        clippy::implicit_return,
        reason = "test-only helper is intentionally a single expression"
    )]
    #[expect(
        clippy::expect_used,
        reason = "test-only helper enforces setup invariants with explicit failure context"
    )]
    #[inline]
    fn err_or_abort<M>(self, _message: M) -> E {
        self.expect_err("test invariant violated: expected Err(..)")
    }
}
