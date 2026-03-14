//! Proc-macro test shim for exact-message expected-failure tests.

#![forbid(unsafe_code)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![expect(
    clippy::single_call_fn,
    reason = "proc-macro stages are kept as small, named steps for diagnostics and tests"
)]
#![expect(
    clippy::question_mark_used,
    reason = "fallible parse/expand flows are clearer with `?` in proc-macro code"
)]
#![expect(
    clippy::implicit_return,
    reason = "proc-macro helpers use expression tails for concise transformations"
)]

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::Parser as _;
use syn::punctuated::Punctuated;
use syn::{Expr, ItemFn, Lit, LitStr, MetaNameValue, Token, parse_macro_input};

/// Marks a test as expected-to-fail with an exact panic message.
///
/// Usage:
/// `#[expect_fail(message = "exact panic message")]`.
///
/// The test passes only when it panics with exactly the provided message.
/// It fails on unexpected pass (no panic) or panic message mismatch.
#[proc_macro_attribute]
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn expect_fail(args: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    render_expand_result(expand_expect_fail(args.into(), input_fn)).into()
}

/// Rewrites the annotated test function to enforce exact-message xfail semantics.
#[cfg_attr(coverage_nightly, coverage(off))]
fn expand_expect_fail(
    args: proc_macro2::TokenStream,
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

/// Renders expansion results into the token stream consumed by the proc-macro API.
#[cfg_attr(coverage_nightly, coverage(off))]
fn render_expand_result(result: syn::Result<proc_macro2::TokenStream>) -> proc_macro2::TokenStream {
    match result {
        Ok(tokens) => tokens,
        Err(err) => err.to_compile_error(),
    }
}

/// Parses attribute arguments and returns the expected panic message literal.
#[cfg_attr(coverage_nightly, coverage(off))]
fn parse_expected_message(args: proc_macro2::TokenStream) -> syn::Result<LitStr> {
    let mut expected: Option<LitStr> = None;

    let parser = Punctuated::<MetaNameValue, Token![,]>::parse_terminated;
    let entries = parser.parse2(args)?;
    for entry in entries {
        if !(entry.path.is_ident("message") || entry.path.is_ident("messager")) {
            return Err(syn::Error::new_spanned(
                entry.path,
                "expected `message = \"...\"`",
            ));
        }
        if expected.is_some() {
            return Err(syn::Error::new_spanned(
                entry.path,
                "duplicate message argument",
            ));
        }

        let Expr::Lit(expr_lit) = entry.value else {
            return Err(syn::Error::new_spanned(
                entry,
                "expected `message = \"...\"`",
            ));
        };
        let Lit::Str(value) = expr_lit.lit else {
            return Err(syn::Error::new_spanned(
                expr_lit,
                "expected `message = \"...\"`",
            ));
        };
        expected = Some(value);
    }

    expected.map_or_else(
        || {
            Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "missing `message = \"...\"`",
            ))
        },
        Ok,
    )
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn parse_expected_message_accepts_message_and_alias() {
        let direct = parse_expected_message(quote!(message = "boom")).expect("message");
        assert_eq!(direct.value(), "boom");

        let alias = parse_expected_message(quote!(messager = "bang")).expect("alias");
        assert_eq!(alias.value(), "bang");
    }

    #[test]
    fn parse_expected_message_rejects_invalid_forms() {
        let missing = parse_expected_message(proc_macro2::TokenStream::new())
            .err()
            .expect("missing should fail");
        assert!(missing.to_string().contains("missing `message = \"...\"`"));

        let duplicate = parse_expected_message(quote!(message = "a", message = "b"))
            .err()
            .expect("duplicate should fail");
        assert!(duplicate.to_string().contains("duplicate message argument"));

        let unknown = parse_expected_message(quote!(unexpected = "x"))
            .err()
            .expect("unknown should fail");
        assert!(unknown.to_string().contains("expected `message = \"...\"`"));

        let malformed = parse_expected_message(quote!(message =))
            .err()
            .expect("malformed should fail");
        assert!(!malformed.to_string().is_empty());

        let parse_error = parse_expected_message(quote!(message))
            .err()
            .expect("parse error should fail");
        assert!(!parse_error.to_string().is_empty());

        let malformed_tokens = parse_expected_message(quote!(= "x"))
            .err()
            .expect("malformed tokens should fail");
        assert!(!malformed_tokens.to_string().is_empty());

        let non_literal_expr = parse_expected_message(quote!(message = value + 1))
            .err()
            .expect("non-literal expression should fail");
        assert!(
            non_literal_expr
                .to_string()
                .contains("expected `message = \"...\"`")
        );

        let non_string_lit = parse_expected_message(quote!(message = 123))
            .err()
            .expect("non-string literal should fail");
        assert!(
            non_string_lit
                .to_string()
                .contains("expected `message = \"...\"`")
        );
    }

    #[test]
    fn expand_expect_fail_rejects_async_and_argument_functions() {
        let async_fn: ItemFn = syn::parse_quote!(
            async fn sample() {}
        );
        let async_err =
            expand_expect_fail(quote!(message = "x"), async_fn).expect_err("async should fail");
        assert!(
            async_err
                .to_string()
                .contains("expect_fail does not support async tests")
        );

        let arg_fn: ItemFn = syn::parse_quote!(
            fn sample(value: i32) {
                let _ = value;
            }
        );
        let arg_err =
            expand_expect_fail(quote!(message = "x"), arg_fn).expect_err("args should fail");
        assert!(
            arg_err
                .to_string()
                .contains("expect_fail test functions must not take arguments")
        );
    }

    #[test]
    fn expand_expect_fail_rewrites_sync_noarg_function() {
        let input_fn: ItemFn = syn::parse_quote!(
            fn sample() {
                panic!("boom");
            }
        );
        let tokens = expand_expect_fail(quote!(message = "boom"), input_fn)
            .expect("sync no-arg function should expand");
        let rendered = tokens.to_string();

        assert!(rendered.contains("catch_unwind"));
        assert!(rendered.contains("XFAIL unexpectedly passed"));
        assert!(rendered.contains("XFAIL panic message mismatch"));
    }

    #[test]
    fn expand_expect_fail_rejects_invalid_args_via_parser() {
        let input_fn: ItemFn = syn::parse_quote!(
            fn sample() {}
        );
        let err = expand_expect_fail(quote!(unexpected = "x"), input_fn)
            .expect_err("invalid attribute args should fail");
        assert!(err.to_string().contains("expected `message = \"...\"`"));
    }

    #[test]
    fn render_expand_result_passes_through_success_and_maps_errors() {
        let tokens = quote!(
            fn sample() {}
        );
        assert_eq!(
            render_expand_result(Ok(tokens.clone())).to_string(),
            tokens.to_string()
        );

        let err = syn::Error::new(proc_macro2::Span::call_site(), "boom");
        let rendered = render_expand_result(Err(err)).to_string();
        assert!(rendered.contains("compile_error"), "rendered: {rendered}");
        assert!(rendered.contains("boom"), "rendered: {rendered}");
    }
}
