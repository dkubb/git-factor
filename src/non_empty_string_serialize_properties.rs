use proptest::prelude::*;
proptest! {
    #[test]
    fn preserves_generated_nonempty_unicode_content(input in ".{1,64}") {
        use crate::test_support::OrAbort as _;
        let value = super::super::super::super::NonEmptyString::new(input.clone()).or_abort("nonempty");
        let actual = serde_json::to_value(&value).or_abort("serialize");
        prop_assert_eq!(actual, serde_json::Value::String(input));
    }
}
