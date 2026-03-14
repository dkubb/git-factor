use alloc::collections::BTreeSet;
use core::fmt;
use std::path::{Path, PathBuf};

use crate::non_empty_string::NonEmptyString;

use super::{FactorError, non_empty_msg};

/// Required hexadecimal character length for full SHA-1 hashes.
pub(in crate::git_factor) const SHA_HEX_LEN: usize = 40;

/// Alias for backwards compatibility in tests.
#[cfg(test)]
pub(in crate::git_factor) const COMMIT_SHA_HEX_LEN: usize = SHA_HEX_LEN;

/// A validated 40-character lowercase-hex SHA-1 hash.
///
/// This is the shared representation for all git object hashes (commits, trees,
/// blobs, tags). Use [`CommitSha`] or [`TreeHash`] for domain-specific wrappers
/// that prevent mixing different object types.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(in crate::git_factor) struct Sha(NonEmptyString);

impl Sha {
    /// Returns the hash as a string slice.
    pub(in crate::git_factor) const fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Parses and validates a 40-char hex SHA-1 hash.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the input is not exactly 40 ASCII hex characters.
    pub(in crate::git_factor) fn parse(raw: String) -> Result<Self, String> {
        let non_empty = match NonEmptyString::try_from(raw.clone()) {
            Ok(non_empty) => non_empty,
            Err(_err) => return Err(raw),
        };
        let value = non_empty.as_str();
        if value.len() != SHA_HEX_LEN || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(raw);
        }
        Ok(Self(non_empty))
    }
}

/// A validated full-length hexadecimal commit SHA.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(in crate::git_factor) struct CommitSha(Sha);

impl CommitSha {
    /// Returns the SHA as a string slice.
    pub(in crate::git_factor) const fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Creates a new `CommitSha` from a string, validating it is exactly
    /// 40 hexadecimal characters.
    pub(in crate::git_factor) fn new(sha: String) -> Result<Self, FactorError> {
        Sha::parse(sha)
            .map(Self)
            .map_err(FactorError::InvalidCommit)
    }
}

impl fmt::Display for CommitSha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A validated full-length hexadecimal tree hash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::git_factor) struct TreeHash(Sha);

impl TreeHash {
    /// Returns the hash as a string slice.
    pub(in crate::git_factor) const fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Creates a new `TreeHash` from a string, validating it is exactly
    /// 40 hexadecimal characters.
    pub(in crate::git_factor) fn new(raw: &str) -> Result<Self, FactorError> {
        let sha = match Sha::parse(raw.to_owned()) {
            Ok(sha) => sha,
            Err(_err) => {
                return Err(FactorError::GitCommand(non_empty_msg(format!(
                    "invalid tree hash: '{raw}'"
                ))));
            }
        };
        Ok(Self(sha))
    }
}

impl fmt::Display for TreeHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A non-empty set of unique commit SHAs.
#[derive(Debug)]
pub(in crate::git_factor) struct Commits(BTreeSet<CommitSha>);

impl Commits {
    /// Returns an iterator over the commit SHAs in lexicographic order.
    pub(in crate::git_factor) fn iter(&self) -> impl Iterator<Item = &CommitSha> {
        self.0.iter()
    }
}

impl TryFrom<BTreeSet<CommitSha>> for Commits {
    type Error = FactorError;

    /// Creates a `Commits` from a non-empty `BTreeSet`.
    fn try_from(set: BTreeSet<CommitSha>) -> Result<Self, Self::Error> {
        if set.is_empty() {
            return Err(FactorError::GitCommand(non_empty_msg(
                "no commits resolved from the given refs".to_owned(),
            )));
        }
        Ok(Self(set))
    }
}

/// Path to the `.git/factor` state directory.
///
/// Wraps `PathBuf` so that state-directory paths cannot be confused with
/// working-directory paths, git-directory paths, or other filesystem locations.
#[derive(Clone, Debug)]
pub(in crate::git_factor) struct StateDir(PathBuf);

impl StateDir {
    /// Returns the inner path as a `&Path` for filesystem operations.
    pub(in crate::git_factor) fn as_path(&self) -> &Path {
        &self.0
    }

    /// Constructs a `StateDir` from a validated path.
    #[cfg_attr(
        not(test),
        expect(
            clippy::single_call_fn,
            reason = "smart constructor keeps the path wrapper explicit at call sites"
        )
    )]
    pub(in crate::git_factor) const fn new(path: PathBuf) -> Self {
        Self(path)
    }
}

#[cfg(test)]
mod tests {
    use super::{COMMIT_SHA_HEX_LEN, CommitSha, Commits, FactorError, Sha, TreeHash};
    use crate::test_support::OrAbort as _;
    use crate::test_support::ResultOrAbort as _;
    use alloc::collections::BTreeSet;
    use proptest::collection::vec;
    use proptest::prelude::*;
    use proptest::sample::select;

    const HEX_ALPHABET: &str = "0123456789abcdefABCDEF";
    const MIN_LONG_SHA_LEN: usize = COMMIT_SHA_HEX_LEN + 1;
    const MAX_LONG_HEX_LEN: usize = COMMIT_SHA_HEX_LEN * 2;
    const SHORT_SHA_LEN: usize = COMMIT_SHA_HEX_LEN - 1;

    fn hex_chars() -> Vec<char> {
        HEX_ALPHABET.chars().collect()
    }

    fn commit_sha_valid_broad() -> impl Strategy<Value = String> {
        let hex = hex_chars();
        vec(select(hex), COMMIT_SHA_HEX_LEN).prop_map(|chars| chars.into_iter().collect())
    }

    fn commit_sha_valid_biased() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("0".repeat(COMMIT_SHA_HEX_LEN)),
            Just("f".repeat(COMMIT_SHA_HEX_LEN)),
            Just("A".repeat(COMMIT_SHA_HEX_LEN)),
            Just("1234567890abcdef1234567890abcdef12345678".to_owned()),
        ]
    }

    fn commit_sha_valid() -> impl Strategy<Value = String> {
        prop_oneof![1 => commit_sha_valid_broad(), 4 => commit_sha_valid_biased()]
    }

    fn commit_sha_invalid_broad() -> impl Strategy<Value = String> {
        let hex = hex_chars();
        let short_hex = vec(select(hex.clone()), 0..COMMIT_SHA_HEX_LEN)
            .prop_map(|chars| chars.into_iter().collect());
        let long_hex = vec(select(hex), MIN_LONG_SHA_LEN..(MAX_LONG_HEX_LEN + 1))
            .prop_map(|chars| chars.into_iter().collect());
        let wrong_length = prop_oneof![short_hex, long_hex,];

        let printable = (b' '..=b'~').map(char::from).collect::<Vec<_>>();
        let has_non_hex = vec(select(printable), COMMIT_SHA_HEX_LEN)
            .prop_map(|chars| chars.into_iter().collect::<String>())
            .prop_filter("must contain at least one non-hex byte", |candidate| {
                candidate.bytes().any(|byte| !byte.is_ascii_hexdigit())
            });

        prop_oneof![wrong_length, has_non_hex]
    }

    fn commit_sha_invalid_biased() -> impl Strategy<Value = String> {
        prop_oneof![
            Just(String::new()),
            Just("0".repeat(SHORT_SHA_LEN)),
            Just("0".repeat(MIN_LONG_SHA_LEN)),
            Just(format!("{}g", "0".repeat(SHORT_SHA_LEN))),
        ]
    }

    fn commit_sha_invalid() -> impl Strategy<Value = String> {
        prop_oneof![1 => commit_sha_invalid_broad(), 4 => commit_sha_invalid_biased()]
    }

    fn commits_valid() -> impl Strategy<Value = BTreeSet<CommitSha>> {
        vec(commit_sha_valid(), 1..8).prop_filter_map(
            "generated SHAs should validate as CommitSha",
            |shas| {
                shas.into_iter()
                    .map(CommitSha::new)
                    .collect::<Result<BTreeSet<_>, _>>()
                    .ok()
            },
        )
    }

    #[test]
    fn proptest_commits_new_rejects_empty_set() {
        let err = Commits::try_from(BTreeSet::new()).err_or_abort("empty set should be rejected");
        assert_eq!(
            err.to_string(),
            "git command failed: no commits resolved from the given refs"
        );
    }

    #[test]
    fn commit_sha_new_rejects_len_40_non_hex_input() {
        let invalid = format!("{}g", "0".repeat(COMMIT_SHA_HEX_LEN - 1));
        let err = CommitSha::new(invalid.clone()).err_or_abort("non-hex sha should be rejected");
        assert_eq!(err.to_string(), format!("invalid commit: {invalid}"));
    }

    #[test]
    fn strategy_helper_functions_construct() {
        let _valid_broad = commit_sha_valid_broad();
        let _valid_biased = commit_sha_valid_biased();
        let _valid = commit_sha_valid();
        let _invalid_broad = commit_sha_invalid_broad();
        let _invalid_biased = commit_sha_invalid_biased();
        let _invalid = commit_sha_invalid();
        let _commits = commits_valid();
    }

    #[test]
    fn proptest_run_unit_suite() {
        proptest_commits_new_rejects_empty_set();
        commit_sha_new_rejects_len_40_non_hex_input();
        proptest_tree_hash_new_preserves_inner_value();
        strategy_helper_functions_construct();
    }

    #[test]
    fn proptest_tree_hash_new_preserves_inner_value() {
        let raw = "a".repeat(COMMIT_SHA_HEX_LEN);
        let tree = TreeHash::new(raw.as_str()).or_abort("valid tree hash");
        assert_eq!(tree.as_str(), raw);
        assert_eq!(format!("{tree}"), raw);
    }

    proptest! {
        #[test]
        fn proptest_commit_sha_new_accepts_valid_input(sha in commit_sha_valid()) {
            let result = CommitSha::new(sha.clone());
            prop_assert!(result.is_ok());
            if let Ok(commit_sha) = result {
                prop_assert_eq!(commit_sha.as_str(), sha);
            }
        }

        #[test]
        fn proptest_commit_sha_new_rejects_invalid_input(sha in commit_sha_invalid()) {
            let result = CommitSha::new(sha.clone());
            prop_assert!(result.is_err());
            if let Err(err) = result {
                prop_assert!(matches!(err, FactorError::InvalidCommit(invalid) if invalid == sha));
            }
        }

        #[test]
        fn proptest_commits_new_accepts_non_empty_sets(set in commits_valid()) {
            let expected_len = set.len();
            let result = Commits::try_from(set);
            prop_assert!(result.is_ok());
            if let Ok(commits) = result {
                prop_assert_eq!(commits.iter().count(), expected_len);
            }
        }

        #[test]
        fn proptest_commit_sha_display_matches_inner_value(sha in commit_sha_valid()) {
            let result = CommitSha::new(sha.clone());
            prop_assert!(result.is_ok());
            if let Ok(commit_sha) = result {
                prop_assert_eq!(format!("{commit_sha}"), sha);
            }
        }

        #[test]
        fn proptest_sha_parse_preserves_inner_value(sha in commit_sha_valid()) {
            let result = Sha::parse(sha.clone());
            prop_assert!(result.is_ok());
            if let Ok(value) = result {
                prop_assert_eq!(value.as_str(), sha);
            }
        }
    }
}
