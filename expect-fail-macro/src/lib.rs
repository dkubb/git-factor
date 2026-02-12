//! Proc-macro test shim for exact-message expected-failure tests.

#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use quote::quote;
use syn::meta::parser;
use syn::parse::Parser as _;
use syn::{ItemFn, LitStr, parse_macro_input};

/// Marks a test as expected-to-fail with an exact panic message.
///
/// Usage:
/// `#[expect_fail(message = "exact panic message")]`.
///
/// The test passes only when it panics with exactly the provided message.
/// It fails on unexpected pass (no panic) or panic message mismatch.
#[proc_macro_attribute]
pub fn expect_fail(args: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    match expand_expect_fail(args, input_fn) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Rewrites the annotated test function to enforce exact-message xfail semantics.
fn expand_expect_fail(
    args: TokenStream,
    mut input_fn: ItemFn,
) -> syn::Result<proc_macro2::TokenStream> {
    if input_fn.sig.asyncness.is_some() {
        return Err(syn::Error::new_spanned(
            input_fn.sig.fn_token,
            "expect_fail does not support async tests",
        ));
    }

    if !input_fn.sig.inputs.is_empty() {
        return Err(syn::Error::new_spanned(
            &input_fn.sig.ident,
            "expect_fail test functions must not take arguments",
        ));
    }

    let expected = parse_expected_message(args)?;
    let original_block = input_fn.block;

    input_fn.block = Box::new(syn::parse_quote!({
        let __expect_fail_result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| #original_block));

        match __expect_fail_result {
            Ok(()) => {
                panic!(
                    "XFAIL unexpectedly passed. Remove #[expect_fail]. Expected panic message:\n{}",
                    #expected
                );
            }
            Err(__payload) => {
                let __actual = if let Some(__s) = __payload.downcast_ref::<String>() {
                    __s.as_str()
                } else if let Some(__s) = __payload.downcast_ref::<&'static str>() {
                    __s
                } else {
                    "<non-string panic payload>"
                };

                assert_eq!(
                    __actual,
                    #expected,
                    "XFAIL panic message mismatch (exact match required)"
                );
            }
        }
    }));

    Ok(quote!(#input_fn))
}

/// Parses attribute arguments and returns the expected panic message literal.
fn parse_expected_message(args: TokenStream) -> syn::Result<LitStr> {
    let mut expected: Option<LitStr> = None;

    let parser = parser(|meta| {
        if meta.path.is_ident("message") || meta.path.is_ident("messager") {
            if expected.is_some() {
                return Err(meta.error("duplicate message argument"));
            }
            let value: LitStr = meta.value()?.parse()?;
            expected = Some(value);
            Ok(())
        } else {
            Err(meta.error("expected `message = \"...\"`"))
        }
    });

    parser.parse(args)?;

    expected.ok_or_else(|| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            "missing `message = \"...\"`",
        )
    })
}
