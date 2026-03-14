use core::fmt::Debug;

pub trait OrAbort<T> {
    fn or_abort<M>(self, _message: M) -> T;
}

impl<T, E: Debug> OrAbort<T> for Result<T, E> {
    fn or_abort<M>(self, _message: M) -> T {
        self.expect("test invariant violated: expected Ok(..)")
    }
}

impl<T> OrAbort<T> for Option<T> {
    fn or_abort<M>(self, _message: M) -> T {
        self.expect("test invariant violated: expected Some(..)")
    }
}

pub trait ResultOrAbort<T, E> {
    fn err_or_abort<M>(self, _message: M) -> E;
}

impl<T: Debug, E> ResultOrAbort<T, E> for Result<T, E> {
    fn err_or_abort<M>(self, _message: M) -> E {
        self.expect_err("test invariant violated: expected Err(..)")
    }
}
