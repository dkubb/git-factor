use core::fmt;

use crate::non_empty_string::NonEmptyString;

/// Required hexadecimal character length for full SHA-1 hashes.
pub(in crate::git_factor) const SHA_HEX_LEN: usize = 40;

/// A validated 40-character lowercase-hex SHA-1 hash.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(in crate::git_factor) struct Sha(NonEmptyString);

impl Sha {
    /// Parses and validates a 40-char hex SHA-1 hash.
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

impl CommitSha {}

impl fmt::Display for CommitSha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.0.as_str())
    }
}

/// A validated full-length hexadecimal tree hash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::git_factor) struct TreeHash(Sha);

impl TreeHash {}

impl fmt::Display for TreeHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.0.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{CommitSha, SHA_HEX_LEN, Sha, TreeHash};

    #[test]
    fn sha_parse_accepts_hex_input() {
        let raw = "1234567890abcdef1234567890abcdef12345678".to_owned();

        let actual = Sha::parse(raw.clone());

        assert_eq!(actual.as_ref().map(|sha| sha.0.as_str()), Ok(raw.as_str()));
    }

    #[test]
    fn sha_parse_rejects_non_hex_input() {
        let raw = "g234567890abcdef1234567890abcdef12345678".to_owned();

        let actual = Sha::parse(raw.clone());

        assert_eq!(actual, Err(raw));
    }

    #[test]
    fn commit_sha_formats_as_string() {
        let raw = "abcdef1234567890abcdef1234567890abcdef12".to_owned();
        let sha = Sha::parse(raw.clone());

        let actual = sha.map(CommitSha).map(|commit_sha| commit_sha.to_string());

        assert_eq!(actual, Ok(raw));
    }

    #[test]
    fn tree_hash_rejects_wrong_length() {
        let raw = "f".repeat(SHA_HEX_LEN - 1);

        let actual = Sha::parse(raw.clone()).map(TreeHash);

        assert_eq!(actual, Err(raw));
    }
}
