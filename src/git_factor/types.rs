use core::fmt;
use std::collections::BTreeSet;

use super::FactorError;

/// A validated 40-character hexadecimal commit SHA.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct CommitSha(String);

impl CommitSha {
    /// Returns the SHA as a string slice.
    pub(super) const fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Creates a new `CommitSha` from a string, validating it is exactly
    /// 40 hexadecimal characters.
    pub(super) fn new(sha: String) -> Result<Self, FactorError> {
        if sha.len() != 40 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(FactorError::InvalidCommit(sha));
        }
        Ok(Self(sha))
    }
}

impl fmt::Display for CommitSha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A non-empty set of unique commit SHAs.
#[derive(Debug)]
pub(super) struct Commits(BTreeSet<CommitSha>);

impl Commits {
    /// Returns an iterator over the commit SHAs in lexicographic order.
    pub(super) fn iter(&self) -> impl Iterator<Item = &CommitSha> {
        self.0.iter()
    }

    /// Creates a `Commits` from a non-empty `BTreeSet`.
    #[expect(
        clippy::single_call_fn,
        reason = "Smart constructor validates non-empty commits set"
    )]
    pub(super) fn new(set: BTreeSet<CommitSha>) -> Result<Self, FactorError> {
        if set.is_empty() {
            return Err(FactorError::GitCommand(
                "no commits resolved from the given refs".to_owned(),
            ));
        }
        Ok(Self(set))
    }
}
