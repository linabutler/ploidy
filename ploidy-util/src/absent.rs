use std::{marker::PhantomData, ops::Deref};

#[cfg(feature = "did-you-mean")]
use ploidy_pointer::JsonPointeeType;
use ploidy_pointer::{JsonPointee, JsonPointeeError, JsonPointer, JsonPointerTypeError};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An [`Option`]-like type that distinguishes between
/// "value not present" and "value present but `null`".
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AbsentOr<T> {
    #[default]
    Absent,
    Null,
    Present(T),
}

/// Converts optional values into [`AbsentOr`].
pub trait AbsentOrExt<T> {
    /// Converts this value into an [`AbsentOr`], mapping absence to
    /// [`AbsentOr::Absent`].
    fn or_absent(self) -> AbsentOr<T>;

    /// Converts this value into an [`AbsentOr`], mapping absence to
    /// [`AbsentOr::Null`].
    fn or_null(self) -> AbsentOr<T>;
}

impl<T> AbsentOrExt<T> for Option<T> {
    #[inline]
    fn or_absent(self) -> AbsentOr<T> {
        match self {
            Some(value) => AbsentOr::Present(value),
            None => AbsentOr::Absent,
        }
    }

    #[inline]
    fn or_null(self) -> AbsentOr<T> {
        match self {
            Some(value) => AbsentOr::Present(value),
            None => AbsentOr::Null,
        }
    }
}

impl<T> AbsentOr<T> {
    /// Returns `true` if the value is [`Absent`](Self::Absent).
    #[inline]
    pub fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }

    /// Returns `true` if the value is [`Null`](Self::Null).
    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns `true` if the value is [`Present`](Self::Present).
    #[inline]
    pub fn is_present(&self) -> bool {
        matches!(self, Self::Present(_))
    }

    /// Converts this [`AbsentOr`] into a [`Result`], mapping
    /// [`Present`] to [`Ok`], and both [`Absent`] and
    /// [`Null`] to [`AbsentError`].
    ///
    /// [`Present`]: Self::Present
    /// [`Absent`]: Self::Absent
    /// [`Null`]: Self::Null
    #[inline]
    pub fn ok(self) -> Result<T, AbsentError> {
        match self {
            Self::Absent => Err(AbsentError::Absent),
            Self::Null => Err(AbsentError::Null),
            Self::Present(value) => Ok(value),
        }
    }

    /// Converts from `&AbsentOr<T>` to `AbsentOr<&T>`.
    #[inline]
    pub fn as_ref(&self) -> AbsentOr<&T> {
        match self {
            Self::Absent => AbsentOr::Absent,
            Self::Null => AbsentOr::Null,
            Self::Present(value) => AbsentOr::Present(value),
        }
    }

    /// Applies `f` to the contained value if [`Present`],
    /// leaving [`Absent`] and [`Null`] untouched.
    ///
    /// [`Present`]: Self::Present
    /// [`Absent`]: Self::Absent
    /// [`Null`]: Self::Null
    #[inline]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> AbsentOr<U> {
        match self {
            Self::Absent => AbsentOr::Absent,
            Self::Null => AbsentOr::Null,
            Self::Present(value) => AbsentOr::Present(f(value)),
        }
    }

    /// Applies `f` to the contained value if [`Present`](Self::Present),
    /// or returns `default` otherwise.
    #[inline]
    pub fn map_or<U>(self, default: U, f: impl FnOnce(T) -> U) -> U {
        match self {
            Self::Absent | Self::Null => default,
            Self::Present(value) => f(value),
        }
    }

    /// Applies `f` to the contained value if [`Present`](Self::Present),
    /// or computes a `default` otherwise.
    #[inline]
    pub fn map_or_else<U>(self, default: impl FnOnce() -> U, f: impl FnOnce(T) -> U) -> U {
        match self {
            Self::Absent | Self::Null => default(),
            Self::Present(value) => f(value),
        }
    }

    /// Returns `other` if `self` is [`Present`], or propagates
    /// [`Absent`] and [`Null`].
    ///
    /// [`Present`]: Self::Present
    /// [`Absent`]: Self::Absent
    /// [`Null`]: Self::Null
    #[inline]
    pub fn and<U>(self, other: AbsentOr<U>) -> AbsentOr<U> {
        match self {
            Self::Absent => AbsentOr::Absent,
            Self::Null => AbsentOr::Null,
            Self::Present(_) => other,
        }
    }

    /// Returns the result of applying `f` to the contained value
    /// if [`Present`], or propagates [`Absent`] and [`Null`].
    ///
    /// [`Present`]: Self::Present
    /// [`Absent`]: Self::Absent
    /// [`Null`]: Self::Null
    #[inline]
    pub fn and_then<U>(self, f: impl FnOnce(T) -> AbsentOr<U>) -> AbsentOr<U> {
        match self {
            Self::Absent => AbsentOr::Absent,
            Self::Null => AbsentOr::Null,
            Self::Present(value) => f(value),
        }
    }

    /// Returns `self` if [`Present`](Self::Present), or `other`
    /// otherwise.
    #[inline]
    pub fn or(self, other: AbsentOr<T>) -> AbsentOr<T> {
        match self {
            Self::Present(_) => self,
            Self::Absent | Self::Null => other,
        }
    }

    /// Returns `self` if [`Present`](Self::Present), or computes
    /// a fallback from `f` otherwise.
    #[inline]
    pub fn or_else(self, f: impl FnOnce() -> AbsentOr<T>) -> AbsentOr<T> {
        match self {
            Self::Present(_) => self,
            Self::Absent | Self::Null => f(),
        }
    }

    /// Returns the contained value if [`Present`](Self::Present),
    /// or the provided `default` otherwise.
    #[inline]
    pub fn unwrap_or(self, default: T) -> T {
        match self {
            Self::Absent | Self::Null => default,
            Self::Present(value) => value,
        }
    }

    /// Returns the contained value if [`Present`](Self::Present),
    /// or computes a default from `f` otherwise.
    #[inline]
    pub fn unwrap_or_else(self, f: impl FnOnce() -> T) -> T {
        match self {
            Self::Absent | Self::Null => f(),
            Self::Present(value) => value,
        }
    }

    /// Converts this [`AbsentOr`] into an [`Option`],
    /// collapsing [`Absent`] and [`Null`] into [`None`].
    ///
    /// [`Absent`]: Self::Absent
    /// [`Null`]: Self::Null
    #[inline]
    pub fn into_option(self) -> Option<T> {
        match self {
            Self::Absent | Self::Null => None,
            Self::Present(value) => Some(value),
        }
    }
}

impl<T: Deref> AbsentOr<T> {
    /// Converts from `AbsentOr<T>` to `AbsentOr<&T::Target>`.
    #[inline]
    pub fn as_deref(&self) -> AbsentOr<&T::Target> {
        match self {
            Self::Absent => AbsentOr::Absent,
            Self::Null => AbsentOr::Null,
            Self::Present(value) => AbsentOr::Present(value),
        }
    }
}

impl<T: Default> AbsentOr<T> {
    /// Returns the contained value if [`Present`](Self::Present),
    /// or the default value of `T` otherwise.
    #[inline]
    pub fn unwrap_or_default(self) -> T {
        match self {
            Self::Absent | Self::Null => T::default(),
            Self::Present(value) => value,
        }
    }
}

impl<T> From<T> for AbsentOr<T> {
    #[inline]
    fn from(value: T) -> Self {
        Self::Present(value)
    }
}

/// Transparently resolves a [`JsonPointer`] against the contained value
/// if [`Present`], or returns an error if [`Absent`] or [`Null`].
///
/// [`Present`]: Self::Present
/// [`Absent`]: Self::Absent
/// [`Null`]: Self::Null
impl<T: JsonPointee> JsonPointee for AbsentOr<T> {
    fn resolve(&self, pointer: &JsonPointer) -> Result<&dyn JsonPointee, JsonPointeeError> {
        match self {
            Self::Present(value) => value.resolve(pointer),
            _ => Err({
                #[cfg(feature = "did-you-mean")]
                let err = JsonPointerTypeError::with_ty(pointer, JsonPointeeType::name_of(self));
                #[cfg(not(feature = "did-you-mean"))]
                let err = JsonPointerTypeError::new(pointer);
                err
            })?,
        }
    }
}

impl<T: Serialize> Serialize for AbsentOr<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Absent | Self::Null => serializer.serialize_none(),
            Self::Present(value) => serializer.serialize_some(value),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for AbsentOr<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<T> {
            type Value = AbsentOr<T>;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("`null` or value")
            }

            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(AbsentOr::Null)
            }

            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(AbsentOr::Null)
            }

            fn visit_some<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                T::deserialize(deserializer).map(AbsentOr::Present)
            }
        }
        deserializer.deserialize_option(Visitor(PhantomData))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AbsentError {
    #[error("value not present")]
    Absent,
    #[error("value is `null`")]
    Null,
}

impl AbsentError {
    /// Attaches a field name to this [`AbsentError`], producing a
    /// [`FieldAbsentError`] suitable for user-facing diagnostics
    /// when a specific field isn't [`Present`](AbsentOr::Present).
    #[inline]
    pub fn field(self, name: &'static str) -> FieldAbsentError {
        match self {
            Self::Absent => FieldAbsentError::Absent(name),
            Self::Null => FieldAbsentError::Null(name),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FieldAbsentError {
    #[error("field `{0}` not present")]
    Absent(&'static str),
    #[error("field `{0}` is `null`")]
    Null(&'static str),
}

#[cfg(test)]
mod tests {
    use ploidy_pointer::{JsonPointee, JsonPointeeExt, JsonPointerTarget};

    use super::*;

    #[derive(JsonPointee, JsonPointerTarget)]
    #[ploidy(pointer(untagged))]
    enum Response {
        One(ResponseOne),
        Two(ResponseTwo),
    }

    #[derive(JsonPointee, JsonPointerTarget)]
    struct ResponseOne {
        data: AbsentOr<String>,
        error: AbsentOr<ResponseError>,
    }

    #[derive(JsonPointee, JsonPointerTarget)]
    struct ResponseTwo {
        data: AbsentOr<i32>,
        error: AbsentOr<ResponseError>,
    }

    #[derive(JsonPointee, JsonPointerTarget)]
    struct ResponseError {
        message: String,
    }

    #[test]
    fn test_absent_or_present_pointer_succeeds() {
        let response = Response::One(ResponseOne {
            error: AbsentOr::Present(ResponseError {
                message: "oops".to_owned(),
            }),
            data: AbsentOr::Null,
        });

        // `error` is present, so the pointer should resolve.
        let err = response.pointer::<&ResponseError>("/error").unwrap();
        assert_eq!(err.message, "oops");
    }

    #[test]
    fn test_absent_or_null_errors() {
        let response = Response::Two(ResponseTwo {
            data: AbsentOr::Present(2),
            error: AbsentOr::Null,
        });

        // The `AbsentOr` wrapper is transparent, and `Null` has no value,
        // so resolving the pointer always errors.
        let result = response.pointer::<&ResponseError>("/error");
        assert!(result.is_err());
    }

    #[test]
    fn test_absent_or_absent_errors() {
        let response = Response::Two(ResponseTwo {
            data: AbsentOr::Present(2),
            error: AbsentOr::Absent,
        });

        // `AbsentOr::Absent` behaves the same as `Null`.
        let result = response.pointer::<&ResponseError>("/error");
        assert!(result.is_err());
    }

    #[test]
    fn test_absent_or_ext_option_some_is_present() {
        let value = Some("value").or_absent();
        assert_eq!(value, AbsentOr::Present("value"));

        let value = Some("value").or_null();
        assert_eq!(value, AbsentOr::Present("value"));
    }

    #[test]
    fn test_absent_or_ext_option_none_or_absent_is_absent() {
        let value: AbsentOr<&str> = None.or_absent();
        assert_eq!(value, AbsentOr::Absent);
    }

    #[test]
    fn test_absent_or_ext_option_none_or_null_is_null() {
        let value: AbsentOr<&str> = None.or_null();
        assert_eq!(value, AbsentOr::Null);
    }
}
