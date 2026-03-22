use std::{
    any::Any,
    borrow::{Borrow, Cow},
    collections::{BTreeMap, HashMap},
    fmt::{Debug, Display},
    hash::BuildHasher,
    iter::FusedIterator,
    ops::{Deref, Range},
    rc::Rc,
    str::Split,
    sync::Arc,
};

use ref_cast::{RefCastCustom, ref_cast_custom};

#[cfg(feature = "derive")]
pub use ploidy_pointer_derive::JsonPointee;

/// A JSON Pointer.
#[derive(Debug, Eq, Hash, Ord, PartialEq, PartialOrd, RefCastCustom)]
#[repr(transparent)]
pub struct JsonPointer(str);

impl JsonPointer {
    #[ref_cast_custom]
    fn new(raw: &str) -> &Self;

    /// Parses a pointer from an RFC 6901 string.
    ///
    /// The empty string is the valid root pointer.
    /// All other strings must start with `/`.
    #[inline]
    pub fn parse(s: &str) -> Result<&Self, BadJsonPointerSyntax> {
        if s.is_empty() || s.starts_with('/') {
            Ok(Self::new(s))
        } else {
            Err(BadJsonPointerSyntax::MissingLeadingSlash)
        }
    }

    /// Returns the empty root pointer.
    #[inline]
    pub fn empty() -> &'static Self {
        JsonPointer::new("")
    }

    /// Returns `true` if this is the empty root pointer.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the first segment, or `None` for the root pointer.
    #[inline]
    pub fn head(&self) -> Option<&JsonPointerSegment> {
        let rest = self.0.strip_prefix('/')?;
        let raw = rest.find('/').map(|index| &rest[..index]).unwrap_or(rest);
        Some(JsonPointerSegment::new(raw))
    }

    /// Returns the pointer without its first segment.
    ///
    /// For the root pointer, returns the root pointer.
    #[inline]
    pub fn tail(&self) -> &JsonPointer {
        self.0
            .strip_prefix('/')
            .and_then(|rest| rest.find('/').map(|index| &rest[index..]))
            .map(JsonPointer::new)
            .unwrap_or_else(|| JsonPointer::empty())
    }

    /// Returns a borrowing iterator over the segments.
    #[inline]
    pub fn segments(&self) -> JsonPointerSegments<'_> {
        JsonPointerSegments(self.0.strip_prefix('/').map(|raw| raw.split('/')))
    }
}

impl Display for JsonPointer {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'a> From<&'a JsonPointer> for Cow<'a, JsonPointer> {
    #[inline]
    fn from(value: &'a JsonPointer) -> Self {
        Cow::Borrowed(value)
    }
}

impl ToOwned for JsonPointer {
    type Owned = JsonPointerBuf;

    #[inline]
    fn to_owned(&self) -> Self::Owned {
        JsonPointerBuf(self.0.to_owned())
    }
}

/// An owned JSON Pointer.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JsonPointerBuf(String);

impl JsonPointerBuf {
    /// Parses an owned pointer from an RFC 6901 string.
    ///
    /// The empty string is the valid root pointer.
    /// All other strings must start with `/`.
    #[inline]
    pub fn parse(s: String) -> Result<Self, BadJsonPointerSyntax> {
        if s.is_empty() || s.starts_with('/') {
            Ok(Self(s))
        } else {
            Err(BadJsonPointerSyntax::MissingLeadingSlash)
        }
    }
}

impl AsRef<JsonPointer> for JsonPointerBuf {
    #[inline]
    fn as_ref(&self) -> &JsonPointer {
        self
    }
}

impl Borrow<JsonPointer> for JsonPointerBuf {
    #[inline]
    fn borrow(&self) -> &JsonPointer {
        self
    }
}

impl Deref for JsonPointerBuf {
    type Target = JsonPointer;

    #[inline]
    fn deref(&self) -> &Self::Target {
        JsonPointer::new(&self.0)
    }
}

impl Display for JsonPointerBuf {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <JsonPointer as Display>::fmt(self, f)
    }
}

impl From<JsonPointerBuf> for Cow<'_, JsonPointer> {
    #[inline]
    fn from(value: JsonPointerBuf) -> Self {
        Cow::Owned(value)
    }
}

impl From<&JsonPointer> for JsonPointerBuf {
    #[inline]
    fn from(value: &JsonPointer) -> Self {
        value.to_owned()
    }
}

/// A value that a [`JsonPointer`] points to.
pub trait JsonPointee: Any {
    /// Returns this value as a `&dyn Any` for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Resolves a [`JsonPointer`] against this value.
    fn resolve(&self, pointer: &JsonPointer) -> Result<&dyn JsonPointee, BadJsonPointer>;
}

impl dyn JsonPointee {
    /// Returns a reference to the pointed-to value if it's of type `T`,
    /// or `None` if it isn't.
    #[inline]
    pub fn downcast_ref<T: JsonPointee>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    /// Returns `true` if the pointed-to value is of type `T`.
    #[inline]
    pub fn is<T: JsonPointee>(&self) -> bool {
        self.as_any().is::<T>()
    }
}

/// Convenience methods for [`JsonPointee`] types.
pub trait JsonPointeeExt: JsonPointee {
    /// Parses a JSON pointer string, resolves it against this value,
    /// and downcasts the result to `T`.
    #[inline]
    fn pointer<T: JsonPointee>(&self, path: &str) -> Result<&T, JsonPointerError> {
        let pointer = JsonPointer::parse(path)?;
        let resolved = self.resolve(pointer)?;
        resolved
            .downcast_ref()
            .ok_or_else(|| JsonPointerError::Type {
                pointer: pointer.to_owned(),
                expected: std::any::type_name::<T>(),
                actual: std::any::type_name_of_val(resolved),
            })
    }
}

impl<P: JsonPointee + ?Sized> JsonPointeeExt for P {}

/// A single segment of a [`JsonPointer`].
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, RefCastCustom)]
#[repr(transparent)]
pub struct JsonPointerSegment(str);

impl JsonPointerSegment {
    #[ref_cast_custom]
    fn new(raw: &str) -> &Self;

    /// Returns the value of this segment as a string.
    #[inline]
    pub fn to_str(&self) -> Cow<'_, str> {
        if self.0.contains('~') {
            self.0.replace("~1", "/").replace("~0", "~").into()
        } else {
            Cow::Borrowed(&self.0)
        }
    }

    /// Returns the value of this segment as an array index,
    /// or `None` if this segment can't be used as an index.
    #[inline]
    pub fn to_index(&self) -> Option<usize> {
        match self.0.as_bytes() {
            [b'0'] => Some(0),
            [b'1'..=b'9', rest @ ..] if rest.iter().all(u8::is_ascii_digit) => {
                // `usize::from_str` allows a leading `+`, and
                // ignores leading zeros; RFC 6901 forbids both.
                self.0.parse().ok()
            }
            _ => None,
        }
    }
}

impl PartialEq<str> for JsonPointerSegment {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.to_str() == other
    }
}

impl PartialEq<JsonPointerSegment> for str {
    #[inline]
    fn eq(&self, other: &JsonPointerSegment) -> bool {
        other == self
    }
}

impl Display for JsonPointerSegment {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_str())
    }
}

/// A borrowing iterator over the segments of a [`JsonPointer`].
#[derive(Clone, Debug)]
pub struct JsonPointerSegments<'a>(Option<Split<'a, char>>);

impl<'a> Iterator for JsonPointerSegments<'a> {
    type Item = &'a JsonPointerSegment;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.0
            .as_mut()
            .and_then(|iter| iter.next())
            .map(JsonPointerSegment::new)
    }
}

impl<'a> DoubleEndedIterator for JsonPointerSegments<'a> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0
            .as_mut()
            .and_then(|iter| iter.next_back())
            .map(JsonPointerSegment::new)
    }
}

impl FusedIterator for JsonPointerSegments<'_> {}

macro_rules! impl_pointee_for {
    () => {};
    (#[$($attrs:tt)+] $ty:ty $(, $($rest:tt)*)?) => {
        #[$($attrs)*]
        impl_pointee_for!($ty);
        $(impl_pointee_for!($($rest)*);)?
    };
    ($ty:ty $(, $($rest:tt)*)?) => {
        impl JsonPointee for $ty {
            #[inline]
            fn as_any(&self) -> &dyn Any { self }

            fn resolve(&self, pointer: &JsonPointer) -> Result<&dyn JsonPointee, BadJsonPointer> {
                if pointer.is_empty() {
                    Ok(self)
                } else {
                    Err({
                        #[cfg(feature = "did-you-mean")]
                        let err = BadJsonPointerTy::with_ty(
                            pointer,
                            JsonPointeeTy::Named(stringify!($ty)),
                        );
                        #[cfg(not(feature = "did-you-mean"))]
                        let err = BadJsonPointerTy::new(pointer);
                        err
                    })?
                }
            }
        }
        $(impl_pointee_for!($($rest)*);)?
    };
}

impl_pointee_for!(
    i8, u8, i16, u16, i32, u32, i64, u64, i128, u128, isize, usize, f32, f64, bool, String, &'static str,
    #[cfg(feature = "chrono")] chrono::DateTime<chrono::Utc>,
    #[cfg(feature = "chrono")] chrono::NaiveDate,
    #[cfg(feature = "url")] url::Url,
);

impl<T: JsonPointee> JsonPointee for Option<T> {
    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn resolve(&self, pointer: &JsonPointer) -> Result<&dyn JsonPointee, BadJsonPointer> {
        if let Some(value) = self {
            value.resolve(pointer)
        } else {
            let Some(key) = pointer.head() else {
                return Ok(&None::<T>);
            };
            Err({
                #[cfg(feature = "did-you-mean")]
                let err = BadJsonPointerKey::with_ty(key, JsonPointeeTy::name_of(self));
                #[cfg(not(feature = "did-you-mean"))]
                let err = BadJsonPointerKey::new(key);
                err
            })?
        }
    }
}

impl<T: JsonPointee> JsonPointee for Box<T> {
    #[inline]
    fn as_any(&self) -> &dyn Any {
        &**self
    }

    fn resolve(&self, pointer: &JsonPointer) -> Result<&dyn JsonPointee, BadJsonPointer> {
        (**self).resolve(pointer)
    }
}

impl<T: JsonPointee> JsonPointee for Arc<T> {
    #[inline]
    fn as_any(&self) -> &dyn Any {
        &**self
    }

    fn resolve(&self, pointer: &JsonPointer) -> Result<&dyn JsonPointee, BadJsonPointer> {
        (**self).resolve(pointer)
    }
}

impl<T: JsonPointee> JsonPointee for Rc<T> {
    #[inline]
    fn as_any(&self) -> &dyn Any {
        &**self
    }

    fn resolve(&self, pointer: &JsonPointer) -> Result<&dyn JsonPointee, BadJsonPointer> {
        (**self).resolve(pointer)
    }
}

impl<T: JsonPointee> JsonPointee for Vec<T> {
    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn resolve(&self, pointer: &JsonPointer) -> Result<&dyn JsonPointee, BadJsonPointer> {
        let Some(key) = pointer.head() else {
            return Ok(self);
        };
        if let Some(index) = key.to_index() {
            if let Some(item) = self.get(index) {
                item.resolve(pointer.tail())
            } else {
                Err(BadJsonPointer::Index(index, 0..self.len()))
            }
        } else {
            Err({
                #[cfg(feature = "did-you-mean")]
                let err = BadJsonPointerTy::with_ty(pointer, JsonPointeeTy::name_of(self));
                #[cfg(not(feature = "did-you-mean"))]
                let err = BadJsonPointerTy::new(pointer);
                err
            })?
        }
    }
}

impl<T, H> JsonPointee for HashMap<String, T, H>
where
    T: JsonPointee,
    H: BuildHasher + 'static,
{
    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn resolve(&self, pointer: &JsonPointer) -> Result<&dyn JsonPointee, BadJsonPointer> {
        let Some(key) = pointer.head() else {
            return Ok(self);
        };
        if let Some(value) = self.get(&*key.to_str()) {
            value.resolve(pointer.tail())
        } else {
            Err({
                #[cfg(feature = "did-you-mean")]
                let err = BadJsonPointerKey::with_suggestions(
                    key,
                    JsonPointeeTy::name_of(self),
                    self.keys().map(|key| key.as_str()),
                );
                #[cfg(not(feature = "did-you-mean"))]
                let err = BadJsonPointerKey::new(key);
                err
            })?
        }
    }
}

impl<T: JsonPointee> JsonPointee for BTreeMap<String, T> {
    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn resolve(&self, pointer: &JsonPointer) -> Result<&dyn JsonPointee, BadJsonPointer> {
        let Some(key) = pointer.head() else {
            return Ok(self);
        };
        if let Some(value) = self.get(&*key.to_str()) {
            value.resolve(pointer.tail())
        } else {
            Err({
                #[cfg(feature = "did-you-mean")]
                let err = BadJsonPointerKey::with_suggestions(
                    key,
                    JsonPointeeTy::name_of(self),
                    self.keys().map(|key| key.as_str()),
                );
                #[cfg(not(feature = "did-you-mean"))]
                let err = BadJsonPointerKey::new(key);
                err
            })?
        }
    }
}

#[cfg(feature = "indexmap")]
impl<T, H> JsonPointee for indexmap::IndexMap<String, T, H>
where
    T: JsonPointee,
    H: BuildHasher + 'static,
{
    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn resolve(&self, pointer: &JsonPointer) -> Result<&dyn JsonPointee, BadJsonPointer> {
        let Some(key) = pointer.head() else {
            return Ok(self);
        };
        if let Some(value) = self.get(&*key.to_str()) {
            value.resolve(pointer.tail())
        } else {
            Err({
                #[cfg(feature = "did-you-mean")]
                let err = BadJsonPointerKey::with_suggestions(
                    key,
                    JsonPointeeTy::name_of(self),
                    self.keys().map(|key| key.as_str()),
                );
                #[cfg(not(feature = "did-you-mean"))]
                let err = BadJsonPointerKey::new(key);
                err
            })?
        }
    }
}

#[cfg(feature = "serde_json")]
impl JsonPointee for serde_json::Value {
    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn resolve(&self, pointer: &JsonPointer) -> Result<&dyn JsonPointee, BadJsonPointer> {
        let Some(key) = pointer.head() else {
            return Ok(self);
        };
        match self {
            serde_json::Value::Object(map) => {
                if let Some(value) = map.get(&*key.to_str()) {
                    value.resolve(pointer.tail())
                } else {
                    Err({
                        #[cfg(feature = "did-you-mean")]
                        let err = BadJsonPointerKey::with_suggestions(
                            key,
                            JsonPointeeTy::name_of(map),
                            map.keys().map(|key| key.as_str()),
                        );
                        #[cfg(not(feature = "did-you-mean"))]
                        let err = BadJsonPointerKey::new(key);
                        err
                    })?
                }
            }
            serde_json::Value::Array(array) => {
                let Some(index) = key.to_index() else {
                    return Err({
                        #[cfg(feature = "did-you-mean")]
                        let err = BadJsonPointerTy::with_ty(pointer, JsonPointeeTy::name_of(array));
                        #[cfg(not(feature = "did-you-mean"))]
                        let err = BadJsonPointerTy::new(pointer);
                        err
                    })?;
                };
                if let Some(item) = array.get(index) {
                    item.resolve(pointer.tail())
                } else {
                    Err(BadJsonPointer::Index(index, 0..array.len()))
                }
            }
            serde_json::Value::Null => Err({
                #[cfg(feature = "did-you-mean")]
                let err = BadJsonPointerKey::with_ty(key, JsonPointeeTy::name_of(self));
                #[cfg(not(feature = "did-you-mean"))]
                let err = BadJsonPointerKey::new(key);
                err
            })?,
            _ => Err({
                #[cfg(feature = "did-you-mean")]
                let err = BadJsonPointerTy::with_ty(pointer, JsonPointeeTy::name_of(self));
                #[cfg(not(feature = "did-you-mean"))]
                let err = BadJsonPointerTy::new(pointer);
                err
            })?,
        }
    }
}

/// An error that occurs during traversal.
#[derive(Debug, thiserror::Error)]
pub enum JsonPointerError {
    #[error(transparent)]
    Syntax(#[from] BadJsonPointerSyntax),
    #[error(transparent)]
    Resolve(#[from] BadJsonPointer),
    #[error("expected `{pointer}` to be {expected}; got {actual}")]
    Type {
        pointer: JsonPointerBuf,
        expected: &'static str,
        actual: &'static str,
    },
}

/// An error that occurs during parsing.
#[derive(Debug, thiserror::Error)]
pub enum BadJsonPointerSyntax {
    #[error("JSON Pointer must start with `/`")]
    MissingLeadingSlash,
}

/// An error that occurs during traversal.
#[derive(Debug, thiserror::Error)]
pub enum BadJsonPointer {
    #[error(transparent)]
    Key(#[from] BadJsonPointerKey),
    #[error("index {} out of range {}..{}", .0, .1.start, .1.end)]
    Index(usize, Range<usize>),
    #[error(transparent)]
    Ty(#[from] BadJsonPointerTy),
}

/// An error that occurs when a pointed-to value doesn't have a key
/// that the pointer references, with an optional suggestion
/// for the correct key.
#[derive(Debug)]
pub struct BadJsonPointerKey {
    pub key: String,
    pub context: Option<BadJsonPointerKeyContext>,
}

impl BadJsonPointerKey {
    #[cold]
    pub fn new(key: &JsonPointerSegment) -> Self {
        Self {
            key: key.to_str().into_owned(),
            context: None,
        }
    }

    #[cfg(feature = "did-you-mean")]
    #[cold]
    pub fn with_ty(key: &JsonPointerSegment, ty: JsonPointeeTy) -> Self {
        Self {
            key: key.to_str().into_owned(),
            context: Some(BadJsonPointerKeyContext {
                ty,
                suggestion: None,
            }),
        }
    }

    #[cfg(feature = "did-you-mean")]
    #[cold]
    pub fn with_suggestions<'a>(
        key: &JsonPointerSegment,
        ty: JsonPointeeTy,
        suggestions: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let key = key.to_str();
        let suggestion = suggestions
            .into_iter()
            .map(|suggestion| (suggestion, strsim::jaro_winkler(&key, suggestion)))
            .max_by(|&(_, a), &(_, b)| {
                // `strsim::jaro_winkler` returns the Jaro-Winkler _similarity_,
                // not distance; so higher values mean the strings are closer.
                a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(suggestion, _)| suggestion.to_owned());
        Self {
            key: key.into_owned(),
            context: Some(BadJsonPointerKeyContext { ty, suggestion }),
        }
    }
}

impl std::error::Error for BadJsonPointerKey {}

impl Display for BadJsonPointerKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.context {
            Some(BadJsonPointerKeyContext {
                ty,
                suggestion: Some(suggestion),
            }) => write!(
                f,
                "unknown key {:?} for value of {ty}; did you mean {suggestion:?}?",
                self.key
            ),
            Some(BadJsonPointerKeyContext {
                ty,
                suggestion: None,
            }) => write!(f, "unknown key {:?} for value of {ty}", self.key),
            None => write!(f, "unknown key {:?}", self.key),
        }
    }
}

#[derive(Debug)]
pub struct BadJsonPointerKeyContext {
    pub ty: JsonPointeeTy,
    pub suggestion: Option<String>,
}

/// An error that occurs when a pointer can't be resolved
/// against a value of the given type.
#[derive(Debug)]
pub struct BadJsonPointerTy {
    pub pointer: String,
    pub ty: Option<JsonPointeeTy>,
}

impl BadJsonPointerTy {
    pub fn new(pointer: &JsonPointer) -> Self {
        Self {
            pointer: pointer.to_string(),
            ty: None,
        }
    }

    #[cfg(feature = "did-you-mean")]
    #[cold]
    pub fn with_ty(pointer: &JsonPointer, ty: JsonPointeeTy) -> Self {
        Self {
            pointer: pointer.to_string(),
            ty: Some(ty),
        }
    }
}

impl std::error::Error for BadJsonPointerTy {}

impl Display for BadJsonPointerTy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.ty {
            Some(ty) => write!(f, "can't resolve {:?} against value of {ty}", self.pointer),
            None => write!(f, "can't resolve {:?}", self.pointer),
        }
    }
}

/// The name of a pointed-to type, for reporting traversal errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JsonPointeeTy {
    Struct(JsonPointeeStructTy),
    Variant(&'static str, JsonPointeeStructTy),
    Named(&'static str),
}

impl JsonPointeeTy {
    #[inline]
    pub fn struct_named(ty: &'static str) -> Self {
        Self::Struct(JsonPointeeStructTy::Named(ty))
    }

    #[inline]
    pub fn tuple_struct_named(ty: &'static str) -> Self {
        Self::Struct(JsonPointeeStructTy::Tuple(ty))
    }

    #[inline]
    pub fn unit_struct_named(ty: &'static str) -> Self {
        Self::Struct(JsonPointeeStructTy::Unit(ty))
    }

    #[inline]
    pub fn struct_variant_named(ty: &'static str, variant: &'static str) -> Self {
        Self::Variant(ty, JsonPointeeStructTy::Named(variant))
    }

    #[inline]
    pub fn tuple_variant_named(ty: &'static str, variant: &'static str) -> Self {
        Self::Variant(ty, JsonPointeeStructTy::Tuple(variant))
    }

    #[inline]
    pub fn unit_variant_named(ty: &'static str, variant: &'static str) -> Self {
        Self::Variant(ty, JsonPointeeStructTy::Unit(variant))
    }

    #[inline]
    pub fn named<T: ?Sized>() -> Self {
        Self::Named(std::any::type_name::<T>())
    }

    #[inline]
    pub fn name_of<T: ?Sized>(value: &T) -> Self {
        Self::Named(std::any::type_name_of_val(value))
    }
}

impl Display for JsonPointeeTy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Struct(JsonPointeeStructTy::Named(ty)) => write!(f, "struct `{ty}`"),
            Self::Struct(JsonPointeeStructTy::Tuple(ty)) => write!(f, "tuple struct `{ty}`"),
            Self::Struct(JsonPointeeStructTy::Unit(ty)) => write!(f, "unit struct `{ty}`"),
            Self::Variant(ty, JsonPointeeStructTy::Named(variant)) => {
                write!(f, "variant `{variant}` of `{ty}`")
            }
            Self::Variant(ty, JsonPointeeStructTy::Tuple(variant)) => {
                write!(f, "tuple variant `{variant}` of `{ty}`")
            }
            Self::Variant(ty, JsonPointeeStructTy::Unit(variant)) => {
                write!(f, "unit variant `{variant}` of `{ty}`")
            }
            Self::Named(ty) => write!(f, "type `{ty}`"),
        }
    }
}

/// The name of a pointed-to struct type or enum variant,
/// for reporting traversal errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JsonPointeeStructTy {
    Named(&'static str),
    Tuple(&'static str),
    Unit(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segments() {
        let pointer = JsonPointer::parse("/foo/bar/0").unwrap();
        let mut segments = pointer.segments();
        assert_eq!(segments.next().unwrap(), "foo");
        assert_eq!(segments.next().unwrap(), "bar");
        // `"0"` is parsed as a string segment, but implementations for `Vec`
        // and tuple structs will parse it as an index.
        assert_eq!(segments.next().unwrap(), "0");
        assert_eq!(segments.next(), None);
    }

    #[test]
    fn test_escaped_segments() {
        let pointer = JsonPointer::parse("/foo~1bar/baz~0qux").unwrap();
        let mut segments = pointer.segments();
        // `~1` unescapes to `/`, `~0` unescapes to `~`.
        assert_eq!(segments.next().unwrap(), "foo/bar");
        assert_eq!(segments.next().unwrap(), "baz~qux");
        assert_eq!(segments.next(), None);
    }

    #[test]
    fn test_segment_display() {
        let pointer = JsonPointer::parse("/foo~1bar").unwrap();
        let segment = pointer.head().unwrap();
        assert_eq!(segment.to_string(), "foo/bar");
    }

    #[test]
    fn test_pointer_display() {
        let input = "/foo/bar~1baz/0";
        let pointer = JsonPointer::parse(input).unwrap();
        assert_eq!(pointer.to_string(), input);
    }

    #[test]
    fn test_pointer_buf() {
        let pointer: Cow<'_, JsonPointer> = JsonPointer::parse("/foo/bar~0baz").unwrap().into();
        let owned = pointer.into_owned();
        let mut segments = owned.segments();
        assert_eq!(segments.next().unwrap(), "foo");
        assert_eq!(segments.next().unwrap(), "bar~baz");
        assert_eq!(owned.to_string(), "/foo/bar~0baz");
    }

    #[test]
    fn test_head_tail_single_segment() {
        let pointer = JsonPointer::parse("/foo").unwrap();
        assert_eq!(pointer.head().unwrap(), "foo");
        assert!(pointer.tail().is_empty());
    }

    #[test]
    fn test_tail_root_idempotent() {
        let root = JsonPointer::empty();
        assert!(root.tail().is_empty());
        assert!(root.tail().tail().is_empty());
    }

    #[test]
    fn test_trailing_slash_produces_empty_segment() {
        let pointer = JsonPointer::parse("/foo/").unwrap();
        let mut segments = pointer.segments();
        assert_eq!(segments.next().unwrap(), "foo");
        assert_eq!(segments.next().unwrap(), "");
        assert_eq!(segments.next(), None);

        // `head()` returns the first segment; `tail()` preserves the
        // trailing slash as a pointer with one empty segment.
        assert_eq!(pointer.head().unwrap(), "foo");
        let tail = pointer.tail();
        assert_eq!(tail.head().unwrap(), "");
        assert!(tail.tail().is_empty());
    }

    #[test]
    fn test_consecutive_slashes() {
        let pointer = JsonPointer::parse("//").unwrap();
        let mut segments = pointer.segments();
        assert_eq!(segments.next().unwrap(), "");
        assert_eq!(segments.next().unwrap(), "");
        assert_eq!(segments.next(), None);

        assert_eq!(pointer.head().unwrap(), "");
        let tail = pointer.tail();
        assert_eq!(tail.head().unwrap(), "");
        assert!(tail.tail().is_empty());
    }

    #[test]
    fn test_parse_missing_leading_slash() {
        assert!(JsonPointer::parse("foo").is_err());
        assert!(JsonPointerBuf::parse("foo".to_owned()).is_err());
    }

    #[test]
    fn test_resolve_vec() {
        let data = vec![1, 2, 3];
        let pointer = JsonPointer::parse("/1").unwrap();
        let result = data.resolve(pointer).unwrap();
        assert_eq!(result.downcast_ref::<i32>(), Some(&2));
    }

    #[test]
    fn test_resolve_hashmap() {
        let mut data = HashMap::new();
        data.insert("foo".to_string(), 42);

        let pointer = JsonPointer::parse("/foo").unwrap();
        let result = data.resolve(pointer).unwrap();
        assert_eq!(result.downcast_ref::<i32>(), Some(&42));
    }

    #[test]
    fn test_resolve_option() {
        let data = Some(42);
        let pointer = JsonPointer::parse("").unwrap();
        let result = data.resolve(pointer).unwrap();
        assert_eq!(result.downcast_ref::<i32>(), Some(&42));
    }

    #[test]
    fn test_primitive_empty_path() {
        let data = 42;
        let pointer = JsonPointer::parse("").unwrap();
        let result = data.resolve(pointer).unwrap();
        assert_eq!(result.downcast_ref::<i32>(), Some(&42));
    }

    #[test]
    fn test_primitive_non_empty_path() {
        let data = 42;
        let pointer = JsonPointer::parse("/foo").unwrap();
        assert!(data.resolve(pointer).is_err());
    }

    #[test]
    fn test_pointer_vec_element() {
        let data = vec![10, 20, 30];
        let result: &i32 = data.pointer("/1").unwrap();
        assert_eq!(result, &20);
    }

    #[test]
    fn test_pointer_hashmap_value() {
        let mut data = HashMap::new();
        data.insert("foo".to_owned(), 42);
        let result: &i32 = data.pointer("/foo").unwrap();
        assert_eq!(result, &42);
    }

    #[test]
    fn test_pointer_root() {
        let data = 42;
        let result: &i32 = data.pointer("").unwrap();
        assert_eq!(result, &42);
    }

    #[test]
    fn test_pointer_syntax_error() {
        let data = 42;
        assert!(matches!(
            data.pointer::<i32>("no-slash"),
            Err(JsonPointerError::Syntax(_))
        ));
    }

    #[test]
    fn test_pointer_resolve_error() {
        let data = 42;
        assert!(matches!(
            data.pointer::<i32>("/foo"),
            Err(JsonPointerError::Resolve(_))
        ));
    }

    #[test]
    fn test_pointer_cast_error() {
        let data = vec![42];
        let err = data.pointer::<String>("/0").unwrap_err();
        assert!(matches!(err, JsonPointerError::Type { .. }));
    }
}
