use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Deserialize, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct UnixMicroseconds(#[serde(with = "chrono::serde::ts_microseconds")] DateTime<Utc>);

impl From<DateTime<Utc>> for UnixMicroseconds {
    #[inline]
    fn from(value: DateTime<Utc>) -> Self {
        Self(value)
    }
}

impl From<UnixMicroseconds> for DateTime<Utc> {
    #[inline]
    fn from(value: UnixMicroseconds) -> Self {
        value.0
    }
}

impl TryFrom<i64> for UnixMicroseconds {
    type Error = TryFromTimestampError;

    #[inline]
    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Ok(Self(
            DateTime::from_timestamp_micros(value).ok_or(TryFromTimestampError)?,
        ))
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct UnixMilliseconds(#[serde(with = "chrono::serde::ts_milliseconds")] DateTime<Utc>);

impl From<DateTime<Utc>> for UnixMilliseconds {
    #[inline]
    fn from(value: DateTime<Utc>) -> Self {
        Self(value)
    }
}

impl From<UnixMilliseconds> for DateTime<Utc> {
    #[inline]
    fn from(value: UnixMilliseconds) -> Self {
        value.0
    }
}

impl TryFrom<i64> for UnixMilliseconds {
    type Error = TryFromTimestampError;

    #[inline]
    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Ok(Self(
            DateTime::from_timestamp_millis(value).ok_or(TryFromTimestampError)?,
        ))
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct UnixNanoseconds(#[serde(with = "chrono::serde::ts_nanoseconds")] DateTime<Utc>);

impl From<DateTime<Utc>> for UnixNanoseconds {
    #[inline]
    fn from(value: DateTime<Utc>) -> Self {
        Self(value)
    }
}

impl From<UnixNanoseconds> for DateTime<Utc> {
    #[inline]
    fn from(value: UnixNanoseconds) -> Self {
        value.0
    }
}

impl From<i64> for UnixNanoseconds {
    #[inline]
    fn from(value: i64) -> Self {
        Self(DateTime::from_timestamp_nanos(value))
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct UnixSeconds(#[serde(with = "chrono::serde::ts_seconds")] DateTime<Utc>);

impl From<DateTime<Utc>> for UnixSeconds {
    #[inline]
    fn from(value: DateTime<Utc>) -> Self {
        Self(value)
    }
}

impl From<UnixSeconds> for DateTime<Utc> {
    #[inline]
    fn from(value: UnixSeconds) -> Self {
        value.0
    }
}

impl TryFrom<i64> for UnixSeconds {
    type Error = TryFromTimestampError;

    #[inline]
    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Ok(Self(
            DateTime::from_timestamp_secs(value).ok_or(TryFromTimestampError)?,
        ))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("timestamp out of range for `DateTime<Utc>`")]
pub struct TryFromTimestampError;
