use crate::git_factor::ShortSha;
use crate::test_support::ResultOrAbort as _;

#[test]
fn short_sha_try_from_rejects_invalid_values() {
    let too_long = "a".repeat(ShortSha::MAX_LEN + 1);
    let too_long_err = ShortSha::try_from(too_long).err_or_abort("too-long short sha");
    assert_eq!(
        too_long_err.to_string(),
        format!(
            "git command failed: short SHA exceeds {} chars ({} chars)",
            ShortSha::MAX_LEN,
            ShortSha::MAX_LEN + 1
        )
    );

    let non_hex_err = ShortSha::try_from("xyz".to_owned()).err_or_abort("non-hex short sha");
    assert_eq!(
        non_hex_err.to_string(),
        "git command failed: short SHA contains non-hex characters: xyz"
    );

    let empty_err = ShortSha::try_from(String::new()).err_or_abort("empty short sha");
    assert_eq!(empty_err.to_string(), "git command failed: empty short SHA");
}
