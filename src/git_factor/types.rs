use alloc::collections::BTreeSet;
use core::fmt;
use std::path::{Path, PathBuf};

use crate::non_empty_string::NonEmptyString;

use super::{FactorError, non_empty_msg};

/// Required hexadecimal character length for full SHA-1 hashes.
pub(in crate::git_factor) const SHA_HEX_LEN: usize = 40;

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
    #[cfg_attr(
        test,
        expect(
            clippy::single_call_fn,
            reason = "constructor is introduced before the orchestration callers that use it"
        )
    )]
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
        test,
        expect(
            clippy::single_call_fn,
            reason = "constructor is introduced before the orchestration callers that use it"
        )
    )]
    pub(in crate::git_factor) const fn new(path: PathBuf) -> Self {
        Self(path)
    }
}

#[cfg(test)]
mod tests {
    use super::{CommitSha, Commits, SHA_HEX_LEN, Sha, StateDir, TreeHash};
    use crate::test_support::OrAbort as _;
    use crate::test_support::ResultOrAbort as _;
    use alloc::collections::BTreeSet;
    use std::path::PathBuf;

    #[test]
    fn sha_parse_preserves_inner_value() {
        let raw = "1234567890abcdef1234567890abcdef12345678".to_owned();
        let sha = Sha::parse(raw.clone()).or_abort("valid sha should parse");

        assert_eq!(sha.as_str(), raw);
    }

    #[test]
    fn commit_sha_new_formats_as_string() {
        let raw = "abcdef1234567890abcdef1234567890abcdef12".to_owned();
        let commit_sha = CommitSha::new(raw.clone()).or_abort("valid commit sha");

        assert_eq!(commit_sha.as_str(), raw);
        assert_eq!(commit_sha.to_string(), raw);
    }

    #[test]
    fn tree_hash_new_rejects_invalid_input() {
        let invalid = "f".repeat(SHA_HEX_LEN - 1);
        let err = TreeHash::new(invalid.as_str()).err_or_abort("short tree hash should fail");

        assert_eq!(
            err.to_string(),
            format!("git command failed: invalid tree hash: '{invalid}'")
        );
    }

    #[test]
    fn commits_try_from_rejects_empty_set() {
        let err = Commits::try_from(BTreeSet::new()).err_or_abort("empty set should be rejected");

        assert_eq!(
            err.to_string(),
            "git command failed: no commits resolved from the given refs"
        );
    }

    #[test]
    fn commits_try_from_preserves_entries() {
        let commit_sha = CommitSha::new("1234567890abcdef1234567890abcdef12345678".to_owned())
            .or_abort("valid commit sha");
        let commits = Commits::try_from(BTreeSet::from([commit_sha.clone()]))
            .or_abort("non-empty set should succeed");

        assert_eq!(commits.iter().collect::<Vec<_>>(), vec![&commit_sha]);
    }

    #[test]
    fn state_dir_new_preserves_path() {
        let path = PathBuf::from(".git/factor");
        let state_dir = StateDir::new(path.clone());

        assert_eq!(state_dir.as_path(), path.as_path());
    }
}
