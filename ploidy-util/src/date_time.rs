use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UnixMicroseconds(DateTime<Utc>);

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
        Ok(Self(DateTime::from_timestamp_micros(value).ok_or_else(
            || TryFromTimestampError::Range(value.into()),
        )?))
    }
}

impl Serialize for UnixMicroseconds {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(self.0.timestamp_micros())
    }
}

impl<'de> Deserialize<'de> for UnixMicroseconds {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use de::Error;
        let timestamp = NumericTimestamp::deserialize(deserializer)?;
        let micros = timestamp.try_into().map_err(D::Error::custom)?;
        DateTime::from_timestamp_micros(micros)
            .map(Self)
            .ok_or_else(|| D::Error::custom(TryFromTimestampError::Range(micros.into())))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UnixMilliseconds(DateTime<Utc>);

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
        Ok(Self(DateTime::from_timestamp_millis(value).ok_or_else(
            || TryFromTimestampError::Range(value.into()),
        )?))
    }
}

impl Serialize for UnixMilliseconds {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(self.0.timestamp_millis())
    }
}

impl<'de> Deserialize<'de> for UnixMilliseconds {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use de::Error;
        let timestamp = NumericTimestamp::deserialize(deserializer)?;
        let millis = timestamp.try_into().map_err(D::Error::custom)?;
        DateTime::from_timestamp_millis(millis)
            .map(Self)
            .ok_or_else(|| D::Error::custom(TryFromTimestampError::Range(millis.into())))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UnixNanoseconds(DateTime<Utc>);

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

impl Serialize for UnixNanoseconds {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(self.0.timestamp_nanos_opt().unwrap_or(i64::MAX))
    }
}

impl<'de> Deserialize<'de> for UnixNanoseconds {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use de::Error;
        let timestamp = NumericTimestamp::deserialize(deserializer)?;
        let nanos = timestamp.try_into().map_err(D::Error::custom)?;
        Ok(Self(DateTime::from_timestamp_nanos(nanos)))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UnixSeconds(DateTime<Utc>);

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
        Ok(Self(DateTime::from_timestamp_secs(value).ok_or_else(
            || TryFromTimestampError::Range(value.into()),
        )?))
    }
}

impl Serialize for UnixSeconds {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(self.0.timestamp())
    }
}

impl<'de> Deserialize<'de> for UnixSeconds {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use de::Error;
        let timestamp = NumericTimestamp::deserialize(deserializer)?;
        let secs = timestamp.try_into().map_err(D::Error::custom)?;
        DateTime::from_timestamp_secs(secs)
            .map(Self)
            .ok_or_else(|| D::Error::custom(TryFromTimestampError::Range(secs.into())))
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum NumericTimestamp<'a> {
    I64(i64),
    U64(u64),
    Str(&'a str),
}

impl TryFrom<NumericTimestamp<'_>> for i64 {
    type Error = TryFromTimestampError;

    fn try_from(value: NumericTimestamp<'_>) -> Result<Self, Self::Error> {
        Ok(match value {
            NumericTimestamp::I64(n) => n,
            NumericTimestamp::U64(n) => {
                Self::try_from(n).map_err(|_| TryFromTimestampError::Range(n.into()))?
            }
            NumericTimestamp::Str(s) => s
                .parse()
                .map_err(|_| TryFromTimestampError::Str(s.to_owned()))?,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TryFromTimestampError {
    #[error("timestamp `{0}` out of range for `DateTime<Utc>`")]
    Range(i128),
    #[error("can't convert `{0}` to timestamp")]
    Str(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    // MARK: Unix seconds

    #[test]
    fn test_unix_seconds_deserialize_from_number() {
        let json = "1609459200";
        let result: UnixSeconds = serde_json::from_str(json).unwrap();
        let dt: DateTime<Utc> = result.into();
        assert_eq!(dt.timestamp(), 1609459200);
    }

    #[test]
    fn test_unix_seconds_deserialize_from_string() {
        let json = r#""1609459200""#;
        let result: UnixSeconds = serde_json::from_str(json).unwrap();
        let dt: DateTime<Utc> = result.into();
        assert_eq!(dt.timestamp(), 1609459200);
    }

    #[test]
    fn test_unix_seconds_serialize() {
        let dt = DateTime::from_timestamp_secs(1609459200).unwrap();
        let ts = UnixSeconds::from(dt);
        let json = serde_json::to_string(&ts).unwrap();
        assert_eq!(json, "1609459200");
    }

    #[test]
    fn test_unix_seconds_invalid_string() {
        let json = r#""not-a-number""#;
        let result: Result<UnixSeconds, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    // MARK: Unix milliseconds

    #[test]
    fn test_unix_milliseconds_deserialize_from_number() {
        let json = "1609459200123";
        let result: UnixMilliseconds = serde_json::from_str(json).unwrap();
        let dt: DateTime<Utc> = result.into();
        assert_eq!(dt.timestamp_millis(), 1609459200123);
    }

    #[test]
    fn test_unix_milliseconds_deserialize_from_string() {
        let json = r#""1609459200123""#;
        let result: UnixMilliseconds = serde_json::from_str(json).unwrap();
        let dt: DateTime<Utc> = result.into();
        assert_eq!(dt.timestamp_millis(), 1609459200123);
    }

    #[test]
    fn test_unix_milliseconds_serialize() {
        let dt = DateTime::from_timestamp_millis(1609459200123).unwrap();
        let ts = UnixMilliseconds::from(dt);
        let json = serde_json::to_string(&ts).unwrap();
        assert_eq!(json, "1609459200123");
    }

    #[test]
    fn test_unix_milliseconds_invalid_string() {
        let json = r#""not-a-number""#;
        let result: Result<UnixMilliseconds, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    // MARK: Unix microseconds

    #[test]
    fn test_unix_microseconds_deserialize_from_number() {
        let json = "1609459200123456";
        let result: UnixMicroseconds = serde_json::from_str(json).unwrap();
        let dt: DateTime<Utc> = result.into();
        assert_eq!(dt.timestamp_micros(), 1609459200123456);
    }

    #[test]
    fn test_unix_microseconds_deserialize_from_string() {
        let json = r#""1609459200123456""#;
        let result: UnixMicroseconds = serde_json::from_str(json).unwrap();
        let dt: DateTime<Utc> = result.into();
        assert_eq!(dt.timestamp_micros(), 1609459200123456);
    }

    #[test]
    fn test_unix_microseconds_serialize() {
        let dt = DateTime::from_timestamp_micros(1609459200123456).unwrap();
        let ts = UnixMicroseconds::from(dt);
        let json = serde_json::to_string(&ts).unwrap();
        assert_eq!(json, "1609459200123456");
    }

    #[test]
    fn test_unix_microseconds_invalid_string() {
        let json = r#""not-a-number""#;
        let result: Result<UnixMicroseconds, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    // MARK: Unix nanoseconds

    #[test]
    fn test_unix_nanoseconds_deserialize_from_number() {
        let json = "1609459200123456789";
        let result: UnixNanoseconds = serde_json::from_str(json).unwrap();
        let dt: DateTime<Utc> = result.into();
        assert_eq!(dt.timestamp_nanos_opt(), Some(1609459200123456789));
    }

    #[test]
    fn test_unix_nanoseconds_deserialize_from_string() {
        let json = r#""1609459200123456789""#;
        let result: UnixNanoseconds = serde_json::from_str(json).unwrap();
        let dt: DateTime<Utc> = result.into();
        assert_eq!(dt.timestamp_nanos_opt(), Some(1609459200123456789));
    }

    #[test]
    fn test_unix_nanoseconds_serialize() {
        let dt = DateTime::from_timestamp_nanos(1609459200123456789);
        let ts = UnixNanoseconds::from(dt);
        let json = serde_json::to_string(&ts).unwrap();
        assert_eq!(json, "1609459200123456789");
    }

    #[test]
    fn test_unix_nanoseconds_invalid_string() {
        let json = r#""not-a-number""#;
        let result: Result<UnixNanoseconds, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}
