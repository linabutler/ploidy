use base64::Engine;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// A wrapper around a [`Vec<u8>`] that serializes and deserializes
/// OpenAPI `byte` strings, which encode binary data as Base64.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Base64(Vec<u8>);

impl Base64 {
    #[inline]
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

impl AsRef<[u8]> for Base64 {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl AsMut<[u8]> for Base64 {
    #[inline]
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

impl From<Vec<u8>> for Base64 {
    #[inline]
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl From<&[u8]> for Base64 {
    #[inline]
    fn from(value: &[u8]) -> Self {
        Self(value.to_vec())
    }
}

impl Serialize for Base64 {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(&self.0);
        serializer.serialize_str(&encoded)
    }
}

impl<'de> Deserialize<'de> for Base64 {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use de::Error;
        let value: &'de str = Deserialize::deserialize(deserializer)?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(value)
            .map_err(|err| D::Error::custom(Base64Error(err)))?;
        Ok(Base64(decoded))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("byte string contains invalid Base64: {0}")]
pub struct Base64Error(#[from] base64::DecodeError);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_empty() {
        let byte = Base64::default();
        let json = serde_json::to_string(&byte).unwrap();
        assert_eq!(json, r#""""#);
    }

    #[test]
    fn test_serialize_text_data() {
        let byte = Base64::from(b"Hello, World!".as_slice());
        let json = serde_json::to_string(&byte).unwrap();
        assert_eq!(json, r#""SGVsbG8sIFdvcmxkIQ==""#);
    }

    #[test]
    fn test_serialize_binary_data() {
        let byte = Base64::from(vec![0x00, 0x01, 0x02, 0xff, 0xfe, 0xfd]);
        let json = serde_json::to_string(&byte).unwrap();
        assert_eq!(json, r#""AAEC//79""#);
    }

    #[test]
    fn test_deserialize_empty() {
        let byte: Base64 = serde_json::from_str(r#""""#).unwrap();
        assert_eq!(byte.as_ref(), b"");
    }

    #[test]
    fn test_deserialize_text_data() {
        let byte: Base64 = serde_json::from_str(r#""SGVsbG8sIFdvcmxkIQ==""#).unwrap();
        assert_eq!(byte.as_ref(), b"Hello, World!");
    }

    #[test]
    fn test_deserialize_binary_data() {
        let byte: Base64 = serde_json::from_str(r#""AAEC//79""#).unwrap();
        assert_eq!(byte.as_ref(), &[0x00, 0x01, 0x02, 0xff, 0xfe, 0xfd]);
    }

    #[test]
    fn test_deserialize_invalid_base64() {
        let result: Result<Base64, _> = serde_json::from_str(r#""not valid base64!!!""#);
        assert!(result.is_err());
    }

    #[test]
    fn test_roundtrip() {
        let original = Base64::from(vec![0x00, 0x7f, 0x80, 0xff, 0x42]);
        let json = serde_json::to_string(&original).unwrap();
        let restored: Base64 = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
    }
}
