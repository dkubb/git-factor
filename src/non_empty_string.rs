//! Crate-local non-empty UTF-8 string type.

#![expect(
    clippy::implicit_return,
    reason = "conflicts with `clippy::needless_return` from `clippy::all`"
)]

use core::borrow::Borrow;
use core::fmt;
use core::ops::Deref;
use core::str::FromStr;
use serde::Deserializer;
use serde::de::Error as _;

/// Construction error for `NonEmptyString`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, thiserror::Error)]
#[non_exhaustive]
pub enum EmptyStringError {
    /// Provided value was empty.
    #[error("value must not be empty")]
    Empty,
}

/// UTF-8 string guaranteed to be non-empty.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub struct NonEmptyString(String);

impl NonEmptyString {
    /// Returns the wrapped string slice.
    #[must_use]
    #[inline]
    pub const fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Constructs a non-empty string.
    ///
    /// # Errors
    ///
    /// Returns `Err(EmptyStringError::Empty)` when `value` is empty.
    #[inline]
    pub fn new(value: String) -> Result<Self, EmptyStringError> {
        if value.is_empty() {
            return Err(EmptyStringError::Empty);
        }

        Ok(Self(value))
    }

    /// Appends `value` to this string.
    #[inline]
    pub fn push_str(&mut self, value: &str) {
        self.0.push_str(value);
    }
}

impl AsRef<str> for NonEmptyString {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for NonEmptyString {
    #[inline]
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl Deref for NonEmptyString {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for NonEmptyString {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for NonEmptyString {
    type Err = EmptyStringError;

    /// # Errors
    ///
    /// Returns `Err(EmptyStringError::Empty)` when `value` is empty.
    #[inline]
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value.to_owned())
    }
}

impl TryFrom<&str> for NonEmptyString {
    type Error = EmptyStringError;

    /// # Errors
    ///
    /// Returns `Err(EmptyStringError::Empty)` when `value` is empty.
    #[inline]
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value.to_owned())
    }
}

impl TryFrom<String> for NonEmptyString {
    type Error = EmptyStringError;

    /// # Errors
    ///
    /// Returns `Err(EmptyStringError::Empty)` when `value` is empty.
    #[inline]
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl serde::Serialize for NonEmptyString {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}
impl<'de> serde::Deserialize<'de> for NonEmptyString {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match String::deserialize(deserializer) {
            Ok(value) => Self::new(value).map_err(D::Error::custom),
            Err(error) => Err(error),
        }
    }

    #[inline]
    fn deserialize_in_place<D>(deserializer: D, place: &mut Self) -> Result<(), D::Error>
    where
        D: Deserializer<'de>,
    {
        match Self::deserialize(deserializer) {
            Ok(value) => {
                *place = value;
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "preserve the established inline test layout"
)]
mod tests {
    mod non_empty_string {
        mod serde {
            mod serialize {
                #[test]
                fn preserves_escapable_content_as_a_json_string() {
                    use crate::test_support::OrAbort as _;
                    let input = "quoted \"value\"\n";
                    let value = super::super::super::super::NonEmptyString::new(input.to_owned())
                        .or_abort("nonempty");
                    let actual = serde_json::to_value(&value).or_abort("serialize");
                    assert_eq!(actual, serde_json::Value::String(input.to_owned()));
                }
            }
        }
    }
    use core::borrow::Borrow;

    use super::{EmptyStringError, NonEmptyString};

    #[test]
    fn as_ref_returns_inner_str() {
        let value = NonEmptyString("alpha".to_owned());
        assert_eq!(<NonEmptyString as AsRef<str>>::as_ref(&value), "alpha");
    }

    #[test]
    fn borrow_returns_inner_str() {
        let value = NonEmptyString("beta".to_owned());
        let borrowed: &str = Borrow::borrow(&value);
        assert_eq!(borrowed, "beta");
    }

    #[test]
    fn deref_returns_inner_str() {
        let value = NonEmptyString("gamma".to_owned());
        let deref_value: &str = &value;
        assert_eq!(deref_value, "gamma");
    }

    #[test]
    fn try_from_str_constructs_non_empty() {
        let result = NonEmptyString::try_from("delta");
        assert_eq!(result.as_ref().map(NonEmptyString::as_str), Ok("delta"));
    }

    #[test]
    fn try_from_string_constructs_non_empty() {
        let result = NonEmptyString::try_from("epsilon".to_owned());
        assert_eq!(result.as_ref().map(NonEmptyString::as_str), Ok("epsilon"));
    }

    #[test]
    fn from_str_constructs_non_empty() {
        let result = "zeta".parse::<NonEmptyString>();
        assert_eq!(result.as_ref().map(NonEmptyString::as_str), Ok("zeta"));
    }

    #[test]
    fn new_rejects_empty_string() {
        assert_eq!(
            NonEmptyString::new(String::new()),
            Err(EmptyStringError::Empty)
        );
    }

    #[test]
    fn push_str_appends_text() {
        let mut value = NonEmptyString("eta".to_owned());
        value.push_str("-theta");
        assert_eq!(value.as_str(), "eta-theta");
    }

    #[test]
    fn display_writes_inner_value() {
        let value = NonEmptyString("iota".to_owned());
        assert_eq!(value.to_string(), "iota");
    }
}

#[cfg(test)]
#[path = "non_empty_string_properties.rs"]
mod proptests;
