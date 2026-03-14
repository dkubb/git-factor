//! Proc-macro test shim for exact-message expected-failure tests.

#![forbid(unsafe_code)]

use proc_macro::TokenStream;

/// Leaves the annotated item unchanged.
#[proc_macro_attribute]
pub fn expect_fail(_args: TokenStream, item: TokenStream) -> TokenStream {
    item
}
