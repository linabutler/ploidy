use std::{
    any::Any,
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    fmt::{Debug, Display},
    ops::{Deref, Range},
    rc::Rc,
    sync::Arc,
};

use itertools::Itertools;

#[cfg(feature = "derive")]
pub use ploidy_pointer_derive::JsonPointee;

/// A parsed JSON Pointer.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JsonPointer<'a>(Cow<'a, [JsonPointerSegment<'a>]>);

impl JsonPointer<'static> {
    /// Constructs a pointer from an RFC 6901 string,
    /// with segments that own their contents.
    pub fn parse_owned(s: &str) -> Result<Self, BadJsonPointerSyntax> {
        if s.is_empty() {
            return Ok(Self::empty());
        }
        let Some(s) = s.strip_prefix('/') else {
            return Err(BadJsonPointerSyntax::MissingLeadingSlash);
        };
        let segments = s
            .split('/')
            .map(str::to_owned)
            .map(JsonPointerSegment::from_str)
            .collect_vec();
        Ok(Self(segments.into()))
    }
}

impl<'a> JsonPointer<'a> {
    /// Constructs an empty pointer that resolves to the current value.
    pub fn empty() -> Self {
        Self(Cow::Borrowed(&[]))
    }

    /// Constructs a pointer from an RFC 6901 string,
    /// with segments that borrow from the string.
    pub fn parse(s: &'a str) -> Result<Self, BadJsonPointerSyntax> {
        if s.is_empty() {
            return Ok(Self::empty());
        }
        let Some(s) = s.strip_prefix('/') else {
            return Err(BadJsonPointerSyntax::MissingLeadingSlash);
        };
        let segments = s.split('/').map(JsonPointerSegment::from_str).collect_vec();
        Ok(Self(segments.into()))
    }

    /// Returns `true` if this is an empty pointer.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the first segment of this pointer, or `None`
    /// if this is an empty pointer.
    pub fn head(&self) -> Option<&JsonPointerSegment<'a>> {
        self.0.first()
    }

    /// Returns a new pointer without the first segment of this pointer.
    /// If this pointer has only one segment, or is an empty pointer,
    /// returns an empty pointer.
    pub fn tail(&self) -> JsonPointer<'_> {
        self.0
            .get(1..)
            .map(|tail| JsonPointer(tail.into()))
            .unwrap_or_else(JsonPointer::empty)
    }

    /// Returns a borrowing iterator over this pointer's segments.
    pub fn segments(&self) -> JsonPointerSegments<'_> {
        JsonPointerSegments(self.0.iter())
    }

    /// Returns a consuming iterator over this pointer's segments.
    pub fn into_segments(self) -> IntoJsonPointerSegments<'a> {
        IntoJsonPointerSegments(self.0.into_owned().into_iter())
    }
}

impl Display for JsonPointer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &*self.0 {
            [] => Ok(()),
            segments => write!(f, "/{}", segments.iter().format("/")),
        }
    }
}

/// A value that a [`JsonPointer`] points to.
pub trait JsonPointee: Any {
    /// Resolves a [`JsonPointer`] against this value.
    fn resolve(&self, pointer: JsonPointer<'_>) -> Result<&dyn JsonPointee, BadJsonPointer>;
}

impl dyn JsonPointee {
    /// Returns a reference to the pointed-to value if it's of type `T`,
    /// or `None` if it isn't.
    #[inline]
    pub fn downcast_ref<T: JsonPointee>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref::<T>()
    }

    /// Returns `true` if the pointed-to value is of type `T`.
    #[inline]
    pub fn is<T: JsonPointee>(&self) -> bool {
        (self as &dyn Any).is::<T>()
    }
}

/// A borrowing iterator over the segments of a [`JsonPointer`].
#[derive(Clone, Debug)]
pub struct JsonPointerSegments<'a>(std::slice::Iter<'a, JsonPointerSegment<'a>>);

impl<'a> Iterator for JsonPointerSegments<'a> {
    type Item = &'a JsonPointerSegment<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }

    #[inline]
    fn count(self) -> usize {
        self.0.count()
    }

    #[inline]
    fn last(mut self) -> Option<Self::Item> {
        self.next_back()
    }
}

impl ExactSizeIterator for JsonPointerSegments<'_> {}

impl DoubleEndedIterator for JsonPointerSegments<'_> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back()
    }
}

/// A consuming iterator over the segments of a [`JsonPointer`].
#[derive(Debug)]
pub struct IntoJsonPointerSegments<'a>(std::vec::IntoIter<JsonPointerSegment<'a>>);

impl<'a> Iterator for IntoJsonPointerSegments<'a> {
    type Item = JsonPointerSegment<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }

    #[inline]
    fn count(self) -> usize {
        self.0.count()
    }

    #[inline]
    fn last(mut self) -> Option<Self::Item> {
        self.next_back()
    }
}

impl ExactSizeIterator for IntoJsonPointerSegments<'_> {}

impl DoubleEndedIterator for IntoJsonPointerSegments<'_> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back()
    }
}

/// A single segment of a [`JsonPointer`].
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JsonPointerSegment<'a>(Cow<'a, str>);

impl<'a> JsonPointerSegment<'a> {
    #[inline]
    fn from_str(s: impl Into<Cow<'a, str>>) -> Self {
        let s = s.into();
        if s.contains('~') {
            Self(s.replace("~1", "/").replace("~0", "~").into())
        } else {
            Self(s)
        }
    }

    /// Returns the string value of this segment.
    #[inline]
    pub fn as_str(&self) -> &str {
        self
    }

    /// Returns the value of this segment as an array index,
    /// or `None` if this segment can't be used as an index.
    #[inline]
    pub fn to_index(&self) -> Option<usize> {
        match self.as_bytes() {
            [b'0'] => Some(0),
            [b'1'..=b'9', rest @ ..] if rest.iter().all(|b: &u8| b.is_ascii_digit()) => {
                // `usize::from_str` allows a leading `+`, and
                // ignores leading zeros; RFC 6901 forbids both.
                self.parse().ok()
            }
            _ => None,
        }
    }
}

impl Deref for JsonPointerSegment<'_> {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for JsonPointerSegment<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.replace("~", "~0").replace("/", "~1"))
    }
}

macro_rules! impl_pointee_for {
    () => {};
    (#[$($attrs:tt)+] $ty:ty $(, $($rest:tt)*)?) => {
        #[$($attrs)*]
        impl_pointee_for!($ty);
        $(impl_pointee_for!($($rest)*);)?
    };
    ($ty:ty $(, $($rest:tt)*)?) => {
        impl JsonPointee for $ty {
            fn resolve(&self, pointer: JsonPointer<'_>) -> Result<&dyn JsonPointee, BadJsonPointer> {
                if pointer.is_empty() {
                    Ok(self)
                } else {
                    Err({
                        #[cfg(feature = "did-you-mean")]
                        let err = BadJsonPointerTy::with_ty(
                            &pointer,
                            JsonPointeeTy::Named(stringify!($ty)),
                        );
                        #[cfg(not(feature = "did-you-mean"))]
                        let err = BadJsonPointerTy::new(&pointer);
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
    #[cfg(feature = "url")] url::Url,
);

impl<T: JsonPointee> JsonPointee for Option<T> {
    fn resolve(&self, pointer: JsonPointer<'_>) -> Result<&dyn JsonPointee, BadJsonPointer> {
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
    fn resolve(&self, pointer: JsonPointer<'_>) -> Result<&dyn JsonPointee, BadJsonPointer> {
        (**self).resolve(pointer)
    }
}

impl<T: JsonPointee> JsonPointee for Arc<T> {
    fn resolve(&self, pointer: JsonPointer<'_>) -> Result<&dyn JsonPointee, BadJsonPointer> {
        (**self).resolve(pointer)
    }
}

impl<T: JsonPointee> JsonPointee for Rc<T> {
    fn resolve(&self, pointer: JsonPointer<'_>) -> Result<&dyn JsonPointee, BadJsonPointer> {
        (**self).resolve(pointer)
    }
}

impl<T: JsonPointee> JsonPointee for Vec<T> {
    fn resolve(&self, pointer: JsonPointer<'_>) -> Result<&dyn JsonPointee, BadJsonPointer> {
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
                let err =
                    BadJsonPointerTy::with_ty(&pointer, JsonPointeeTy::Named(stringify!($ty)));
                #[cfg(not(feature = "did-you-mean"))]
                let err = BadJsonPointerTy::new(&pointer);
                err
            })?
        }
    }
}

impl<T: JsonPointee> JsonPointee for HashMap<String, T> {
    fn resolve(&self, pointer: JsonPointer<'_>) -> Result<&dyn JsonPointee, BadJsonPointer> {
        let Some(key) = pointer.head() else {
            return Ok(self);
        };
        if let Some(value) = self.get(key.as_str()) {
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
    fn resolve(&self, pointer: JsonPointer<'_>) -> Result<&dyn JsonPointee, BadJsonPointer> {
        let Some(key) = pointer.head() else {
            return Ok(self);
        };
        if let Some(value) = self.get(key.as_str()) {
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
impl<T: JsonPointee> JsonPointee for indexmap::IndexMap<String, T> {
    fn resolve(&self, pointer: JsonPointer<'_>) -> Result<&dyn JsonPointee, BadJsonPointer> {
        let Some(key) = pointer.head() else {
            return Ok(self);
        };
        if let Some(value) = self.get(key.as_str()) {
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
    fn resolve(&self, pointer: JsonPointer<'_>) -> Result<&dyn JsonPointee, BadJsonPointer> {
        let Some(key) = pointer.head() else {
            return Ok(self);
        };
        match self {
            serde_json::Value::Object(map) => {
                if let Some(value) = map.get(key.as_str()) {
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
                        let err =
                            BadJsonPointerTy::with_ty(&pointer, JsonPointeeTy::name_of(array));
                        #[cfg(not(feature = "did-you-mean"))]
                        let err = BadJsonPointerTy::new(&pointer);
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
                let err = BadJsonPointerTy::with_ty(&pointer, JsonPointeeTy::name_of(self));
                #[cfg(not(feature = "did-you-mean"))]
                let err = BadJsonPointerTy::new(&pointer);
                err
            })?,
        }
    }
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
    pub fn new(key: &JsonPointerSegment<'_>) -> Self {
        Self {
            key: key.to_string(),
            context: None,
        }
    }

    #[cfg(feature = "did-you-mean")]
    #[cold]
    pub fn with_ty(key: &JsonPointerSegment<'_>, ty: JsonPointeeTy) -> Self {
        Self {
            key: key.to_string(),
            context: Some(BadJsonPointerKeyContext {
                ty,
                suggestion: None,
            }),
        }
    }

    #[cfg(feature = "did-you-mean")]
    #[cold]
    pub fn with_suggestions<'a>(
        key: &'a JsonPointerSegment<'_>,
        ty: JsonPointeeTy,
        suggestions: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let suggestion = suggestions
            .into_iter()
            .map(|suggestion| (suggestion, strsim::jaro_winkler(key.as_str(), suggestion)))
            .max_by(|&(_, a), &(_, b)| {
                // `strsim::jaro_winkler` returns the Jaro-Winkler _similarity_,
                // not distance; so higher values mean the strings are closer.
                a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(suggestion, _)| suggestion.to_owned());
        Self {
            key: key.to_string(),
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
    pub fn new(pointer: &JsonPointer<'_>) -> Self {
        Self {
            pointer: pointer.to_string(),
            ty: None,
        }
    }

    #[cfg(feature = "did-you-mean")]
    #[cold]
    pub fn with_ty(pointer: &JsonPointer<'_>, ty: JsonPointeeTy) -> Self {
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
    fn test_parse_pointer() {
        let pointer = JsonPointer::parse("/foo/bar/0").unwrap();
        let mut segments = pointer.into_segments();
        assert_eq!(segments.len(), 3);
        assert_eq!(segments.next(), Some(JsonPointerSegment::from_str("foo")));
        assert_eq!(segments.next(), Some(JsonPointerSegment::from_str("bar")));
        // `"0"` is parsed as a string segment, but implementations for `Vec`
        // and tuple structs will parse it as an index.
        assert_eq!(segments.next(), Some(JsonPointerSegment::from_str("0")));
        assert_eq!(segments.next(), None);
    }

    #[test]
    fn test_parse_pointer_escaping() {
        let pointer = JsonPointer::parse("/foo~1bar/baz~0qux").unwrap();
        let mut segments = pointer.into_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(
            segments.next(),
            Some(JsonPointerSegment::from_str("foo~1bar"))
        );
        assert_eq!(
            segments.next(),
            Some(JsonPointerSegment::from_str("baz~0qux"))
        );
        assert_eq!(segments.next(), None);
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
    fn test_segments() {
        let pointer = JsonPointer::parse("/foo/bar/baz").unwrap();

        // Can iterate multiple times with borrowing iterator.
        let segments: Vec<_> = pointer.segments().map(|s| s.as_str()).collect();
        assert_eq!(segments, vec!["foo", "bar", "baz"]);

        // Pointer is still usable after borrowing iteration.
        let segments_again: Vec<_> = pointer.segments().map(|s| s.as_str()).collect();
        assert_eq!(segments_again, vec!["foo", "bar", "baz"]);

        // Verify iterator traits.
        assert_eq!(pointer.segments().len(), 3);
        assert_eq!(pointer.segments().last().map(|s| s.as_str()), Some("baz"));
        assert_eq!(
            pointer.segments().next_back().map(|s| s.as_str()),
            Some("baz")
        );
    }
}
