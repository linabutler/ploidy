use std::marker::PhantomData;

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

impl<T> AbsentOr<T> {
    #[inline]
    pub fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    #[inline]
    pub fn is_present(&self) -> bool {
        matches!(self, Self::Present(_))
    }

    #[inline]
    pub fn ok(self) -> Result<T, AbsentError> {
        match self {
            Self::Absent => Err(AbsentError::Absent),
            Self::Null => Err(AbsentError::Null),
            Self::Present(value) => Ok(value),
        }
    }

    #[inline]
    pub fn as_ref(&self) -> AbsentOr<&T> {
        match self {
            Self::Absent => AbsentOr::Absent,
            Self::Null => AbsentOr::Null,
            Self::Present(value) => AbsentOr::Present(value),
        }
    }

    #[inline]
    pub fn into_option(self) -> Option<T> {
        match self {
            Self::Absent | Self::Null => None,
            Self::Present(value) => Some(value),
        }
    }
}

impl<T> From<T> for AbsentOr<T> {
    #[inline]
    fn from(value: T) -> Self {
        Self::Present(value)
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
