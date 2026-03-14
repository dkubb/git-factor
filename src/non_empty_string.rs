use core::borrow::Borrow;
use core::fmt;
use core::ops::Deref;
use core::str::FromStr;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, thiserror::Error)]
pub enum EmptyStringError {
    #[error("value must not be empty")]
    Empty,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NonEmptyString(String);

impl NonEmptyString {
    #[must_use]
    pub const fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn new(value: String) -> Result<Self, EmptyStringError> {
        if value.is_empty() {
            return Err(EmptyStringError::Empty);
        }

        Ok(Self(value))
    }

    pub fn push_str(&mut self, value: &str) {
        self.0.push_str(value);
    }
}

impl AsRef<str> for NonEmptyString {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for NonEmptyString {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl Deref for NonEmptyString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for NonEmptyString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for NonEmptyString {
    type Err = EmptyStringError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value.to_owned())
    }
}

impl TryFrom<&str> for NonEmptyString {
    type Error = EmptyStringError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value.to_owned())
    }
}

impl TryFrom<String> for NonEmptyString {
    type Error = EmptyStringError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
