//! OpenAPI query parameter serialization.
//!
//! This module provides Serde-based serialization for OpenAPI 3.x
//! query parameters, and supports all standard query styles:
//! `form`, `deepObject`, `spaceDelimited`, and `pipeDelimited`.
//!
//! # Examples
//!
//! ```
//! use url::Url;
//! use ploidy_util::query::{QuerySerializer, QueryStyle};
//! # use ploidy_util::query::QueryParamError;
//!
//! # fn main() -> Result<(), QueryParamError> {
//! # use serde::Serialize;
//! #[derive(Serialize)]
//! #[serde(rename_all = "lowercase")]
//! enum Kind {
//!     Dog,
//!     Cat,
//!     Fish,
//!     Bunny,
//! }
//!
//! // Serialize parameters with the default style: `form`, exploded.
//! let mut url = Url::parse("https://api.example.com/pets").unwrap();
//! QuerySerializer::new(&mut url)
//!     .append("kind", &[Kind::Dog, Kind::Cat])?
//!     .append("limit", &10)?;
//! assert_eq!(url.as_str(), "https://api.example.com/pets?kind=dog&kind=cat&limit=10");
//!
//! // ...Or as comma-separated values:
//! let mut url = Url::parse("https://api.example.com/pets").unwrap();
//! QuerySerializer::new(&mut url)
//!     .style(QueryStyle::Form { exploded: false })
//!     .append("kind", &[Kind::Dog, Kind::Cat])?;
//! assert_eq!(url.as_str(), "https://api.example.com/pets?kind=dog,cat");
//!
//! // ...Or use `spaceDelimited` values:
//! let mut url = Url::parse("https://api.example.com/pets").unwrap();
//! QuerySerializer::new(&mut url)
//!     .style(QueryStyle::SpaceDelimited)
//!     .append("kind", &[Kind::Dog, Kind::Cat])?;
//! assert_eq!(url.as_str(), "https://api.example.com/pets?kind=dog%20cat");
//!
//! // ...Or `pipeDelimited` values:
//! let mut url = Url::parse("https://api.example.com/pets").unwrap();
//! QuerySerializer::new(&mut url)
//!     .style(QueryStyle::PipeDelimited)
//!     .append("kind", &[Kind::Dog, Kind::Cat])?;
//! assert_eq!(url.as_str(), "https://api.example.com/pets?kind=dog%7Ccat");
//!
//! // ...Or `deepObject` for nested structures:
//! #[derive(Serialize)]
//! struct Filter {
//!     kind: Vec<Kind>,
//!     term: String,
//!     max_price: u32,
//! }
//!
//! let filter = Filter {
//!     kind: vec![Kind::Dog, Kind::Cat, Kind::Bunny],
//!     term: "chow".to_owned(),
//!     max_price: 30,
//! };
//!
//! let mut url = Url::parse("https://api.example.com/search").unwrap();
//! QuerySerializer::new(&mut url)
//!     .style(QueryStyle::DeepObject)
//!     .append("filter", &filter)?;
//! assert!(url.query_pairs().eq([
//!     ("filter[kind][0]".into(), "dog".into()),
//!     ("filter[kind][1]".into(), "cat".into()),
//!     ("filter[kind][2]".into(), "bunny".into()),
//!     ("filter[term]".into(), "chow".into()),
//!     ("filter[max_price]".into(), "30".into()),
//! ]));
//! # Ok(())
//! # }
//! ```

use std::{borrow::Cow, fmt::Display};

use itertools::Itertools;
use percent_encoding::{AsciiSet, CONTROLS, PercentEncode};
use serde::{
    Serialize,
    ser::{Impossible, SerializeMap, SerializeSeq, SerializeStruct, SerializeTuple},
};
use url::Url;

/// Styles that describe how to format URL query parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryStyle {
    /// Multiple values formatted as repeated parameters (if exploded),
    /// or comma-separated values (if non-exploded).
    ///
    /// The exploded `form` style is the default.
    Form { exploded: bool },

    /// Multiple values separated by spaces.
    SpaceDelimited,

    /// Multiple values separated by pipes.
    PipeDelimited,

    /// Bracket notation for nested structures.
    DeepObject,
}

impl Default for QueryStyle {
    fn default() -> Self {
        Self::Form { exploded: true }
    }
}

/// A serializer that formats and appends URL query parameters
/// according to OpenAPI styles.
pub struct QuerySerializer<'a> {
    url: &'a mut Url,
    style: QueryStyle,
}

impl<'a> QuerySerializer<'a> {
    /// Creates a new serializer.
    pub fn new(url: &'a mut Url) -> Self {
        Self {
            url,
            style: QueryStyle::default(),
        }
    }

    /// Sets the formatting style.
    pub fn style(mut self, style: QueryStyle) -> Self {
        self.style = style;
        self
    }

    /// Serializes and appends a query parameter to the URL.
    pub fn append<T: Serialize>(self, name: &str, value: &T) -> Result<Self, QueryParamError> {
        use ParamSerializerState::*;
        let style = match self.style {
            QueryStyle::DeepObject => DeepObject,
            QueryStyle::Form { exploded: true } => ExplodedForm,
            QueryStyle::Form { exploded: false } => NonExplodedForm(vec![]),
            QueryStyle::PipeDelimited => Delimited("|", vec![]),
            QueryStyle::SpaceDelimited => Delimited(" ", vec![]),
        };
        let mut path = KeyPath::new(name);
        let mut serializer = QueryParamSerializer::new(self.url, &mut path, style);
        value.serialize(&mut serializer)?;
        serializer.flush();
        Ok(self)
    }
}

#[derive(Debug)]
enum ParamSerializerState {
    /// Non-exploded `spaceDelimited` or `pipeDelimited` style.
    Delimited(&'static str, Vec<String>),
    /// Exploded `form` style.
    ExplodedForm,
    /// Non-exploded `form` style.
    NonExplodedForm(Vec<String>),
    /// Exploded `deepObject` style.
    DeepObject,
}

#[derive(Clone, Debug)]
struct KeyPath<'a>(Cow<'a, str>, Vec<Cow<'a, str>>);

impl<'a> KeyPath<'a> {
    fn new(head: impl Into<Cow<'a, str>>) -> Self {
        Self(head.into(), vec![])
    }

    fn len(&self) -> usize {
        self.1.len() + 1
    }

    fn push(&mut self, segment: impl Into<Cow<'a, str>>) {
        self.1.push(segment.into());
    }

    fn pop(&mut self) -> Cow<'a, str> {
        self.1.pop().unwrap_or_else(|| self.0.clone())
    }

    fn first(&self) -> &str {
        &self.0
    }

    fn last(&self) -> &str {
        self.1.last().unwrap_or(&self.0)
    }

    fn split_first(&self) -> (&str, &[Cow<'a, str>]) {
        (&self.0, &self.1)
    }
}

/// The [component percent-encode set][component], as defined by
/// the WHATWG URL Standard.
///
/// This is the [userinfo percent-encode set][userinfo] and
/// U+0024 (`$`) to U+0026 (`&`), inclusive; U+002B (`+`); and U+002C (`,`).
/// It gives identical results to JavaScript's `encodeURIComponent()`
/// function.
///
/// [component]: https://url.spec.whatwg.org/#component-percent-encode-set
/// [userinfo]: https://url.spec.whatwg.org/#userinfo-percent-encode-set
const COMPONENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'^')
    .add(b'{')
    .add(b'}')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'=')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'|')
    .add(b'$')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b',');

#[derive(Clone, Debug)]
enum EncodedOrRaw<'a> {
    Encoded(PercentEncode<'a>),
    Raw(&'a str),
}

impl<'a> EncodedOrRaw<'a> {
    fn encode(input: &'a str) -> Self {
        Self::Encoded(percent_encoding::utf8_percent_encode(input, COMPONENT))
    }
}

impl Display for EncodedOrRaw<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encoded(s) => write!(f, "{s}"),
            Self::Raw(s) => f.write_str(s),
        }
    }
}

#[derive(Debug)]
struct PercentEncodeDelimited<'a, T>(&'a [T], EncodedOrRaw<'a>);

impl<T: AsRef<str>> Display for PercentEncodeDelimited<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            // Use the fully qualified syntax as suggested by
            // rust-lang/rust#79524.
            Itertools::intersperse(
                self.0
                    .iter()
                    .map(|input| input.as_ref())
                    .map(EncodedOrRaw::encode),
                self.1.clone()
            )
            .format("")
        )
    }
}

/// A [`Serializer`][serde::Serializer] for a single query parameter.
#[derive(Debug)]
struct QueryParamSerializer<'a> {
    /// A mutable reference to the URL being constructed.
    url: &'a mut url::Url,
    /// The current key path, starting with the parameter name.
    /// The serializer pushes and pops additional segments for
    /// nested structures.
    path: &'a mut KeyPath<'a>,
    state: ParamSerializerState,
}

impl<'a> QueryParamSerializer<'a> {
    /// Creates a new query parameter serializer.
    fn new(url: &'a mut url::Url, path: &'a mut KeyPath<'a>, state: ParamSerializerState) -> Self {
        Self { url, path, state }
    }

    /// Computes the key for the current value, accounting for nesting.
    fn key(&self) -> Cow<'_, str> {
        use ParamSerializerState::*;
        match &self.state {
            DeepObject => {
                // `deepObject` style uses `base[field1][field2]...`.
                match self.path.split_first() {
                    (head, []) => head.into(),
                    (head, rest) => format!("{head}[{}]", rest.iter().format("][")).into(),
                }
            }
            ExplodedForm => {
                // Exploded `form` style uses the field name directly.
                self.path.last().into()
            }
            NonExplodedForm(_) | Delimited(_, _) => {
                // Non-exploded styles use the base parameter name directly.
                self.path.first().into()
            }
        }
    }

    /// Appends an unencoded value, either to the buffer or directly to the URL.
    fn append<'b>(&mut self, value: impl Into<Cow<'b, str>>) {
        use ParamSerializerState::*;
        let value = value.into();
        match &mut self.state {
            NonExplodedForm(buf) | Delimited(_, buf) => {
                buf.push(value.into_owned());
            }
            DeepObject | ExplodedForm => {
                // For exploded styles, append the key and value directly to the URL.
                // This encodes them using `form-urlencoded` rules, not percent-encoding;
                // OpenAPI allows both here.
                let key = self.key().into_owned();
                self.url.query_pairs_mut().append_pair(&key, &value);
            }
        }
    }

    /// Flushes any buffered values to the URL.
    ///
    /// This is called by compound serializers when they finish collecting values,
    /// and by [`Serializer::append`] to write top-level values.
    fn flush(&mut self) {
        use ParamSerializerState::*;
        let (delimiter, buf) = match &mut self.state {
            NonExplodedForm(buf) => (
                // For the non-exploded `form` style, commas aren't encoded.
                EncodedOrRaw::Raw(","),
                std::mem::take(buf),
            ),
            Delimited(delimiter, buf) => (
                // For `spaceDelimited` and `pipeDelimited`, delimeters are encoded.
                EncodedOrRaw::encode(delimiter),
                std::mem::take(buf),
            ),
            _ => return,
        };
        if buf.is_empty() {
            return;
        }

        let key = self.key();
        let key = EncodedOrRaw::encode(&key);
        let value = PercentEncodeDelimited(&buf, delimiter);

        // Append the percent-encoded key and value to the existing query string.
        // We avoid `query_pairs_mut()` here, because it uses `form-urlencoded` rules,
        // while OpenAPI requires percent-encoding for "non-RFC6570 query string styles".
        let new_query = match self.url.query().map(|q| q.trim_end_matches('&')) {
            Some(query) if !query.is_empty() => format!("{query}&{key}={value}"),
            _ => format!("{key}={value}"),
        };
        self.url.set_query(Some(&new_query));
    }
}

impl<'a, 'b> serde::Serializer for &'a mut QueryParamSerializer<'b> {
    type Ok = ();
    type Error = QueryParamError;

    type SerializeSeq = QuerySeqSerializer<'a, 'b>;
    type SerializeTuple = QuerySeqSerializer<'a, 'b>;
    type SerializeTupleStruct = Impossible<(), QueryParamError>;
    type SerializeTupleVariant = Impossible<(), QueryParamError>;
    type SerializeMap = QueryStructSerializer<'a, 'b>;
    type SerializeStruct = QueryStructSerializer<'a, 'b>;
    type SerializeStructVariant = Impossible<(), QueryParamError>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        self.append(if v { "true" } else { "false" });
        Ok(())
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.append(v.to_string());
        Ok(())
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        self.append(v.to_string());
        Ok(())
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.append(v.to_string());
        Ok(())
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        self.append(v.to_string());
        Ok(())
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.append(v.to_string());
        Ok(())
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        self.append(v.to_string());
        Ok(())
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        self.append(v.to_string());
        Ok(())
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        self.append(v.to_string());
        Ok(())
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.append(v.to_string());
        Ok(())
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        self.append(v.to_string());
        Ok(())
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        self.append(v.to_string());
        Ok(())
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        self.append(v);
        Ok(())
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(UnsupportedTypeError::Bytes)?
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        // Don't emit query parameters for `None`.
        Ok(())
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(UnsupportedTypeError::Unit)?
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok, Self::Error> {
        Err(UnsupportedTypeError::UnitStruct(name))?
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.append(variant);
        Ok(())
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(UnsupportedTypeError::NewtypeVariant(name, variant))?
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(QuerySeqSerializer {
            serializer: self,
            index: 0,
        })
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(QuerySeqSerializer {
            serializer: self,
            index: 0,
        })
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(UnsupportedTypeError::TupleStruct(name))?
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(UnsupportedTypeError::TupleVariant(name, variant))?
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(QueryStructSerializer { serializer: self })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(QueryStructSerializer { serializer: self })
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(UnsupportedTypeError::StructVariant(name, variant))?
    }
}

/// A serializer for sequences (arrays) and tuples.
pub struct QuerySeqSerializer<'a, 'b> {
    serializer: &'a mut QueryParamSerializer<'b>,
    index: usize,
}

impl<'a, 'b> SerializeSeq for QuerySeqSerializer<'a, 'b> {
    type Ok = ();
    type Error = QueryParamError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        use ParamSerializerState::*;
        match &mut self.serializer.state {
            DeepObject if self.serializer.path.len() == 1 => {
                // OpenAPI doesn't define `deepObject` for top-level arrays; and
                // we know we're at the top level if the key path has just one segment.
                return Err(QueryParamError::UnspecifiedStyleExploded);
            }
            DeepObject => {
                // Otherwise, we're inside a nested structure.
                self.serializer.path.push(self.index.to_string());
                value.serialize(&mut *self.serializer)?;
                self.serializer.path.pop();
            }
            _ => value.serialize(&mut *self.serializer)?,
        }
        self.index += 1;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.serializer.flush();
        Ok(())
    }
}

impl<'a, 'b> SerializeTuple for QuerySeqSerializer<'a, 'b> {
    type Ok = ();
    type Error = QueryParamError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeSeq::end(self)
    }
}

/// A serializer for structs and maps (objects).
pub struct QueryStructSerializer<'a, 'b> {
    serializer: &'a mut QueryParamSerializer<'b>,
}

impl<'a, 'b> SerializeStruct for QueryStructSerializer<'a, 'b> {
    type Ok = ();
    type Error = QueryParamError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        use ParamSerializerState::*;
        if let NonExplodedForm(buf) | Delimited(_, buf) = &mut self.serializer.state {
            // For non-exploded styles, insert the key before the value.
            // This creates alternating key-value pairs.
            buf.push(key.to_owned());
        };

        self.serializer.path.push(key);
        value.serialize(&mut *self.serializer)?;
        self.serializer.path.pop();
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.serializer.flush();
        Ok(())
    }
}

impl<'a, 'b> SerializeMap for QueryStructSerializer<'a, 'b> {
    type Ok = ();
    type Error = QueryParamError;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
        let mut extractor = KeyExtractor { key: String::new() };
        key.serialize(&mut extractor)?;
        self.serializer.path.push(extractor.key);
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        use ParamSerializerState::*;
        if let NonExplodedForm(buf) | Delimited(_, buf) = &mut self.serializer.state {
            // For non-exploded styles, insert the key before the value
            // (`serialize_key()` already added the key to the path).
            buf.push(self.serializer.path.last().to_owned());
        };

        value.serialize(&mut *self.serializer)?;
        self.serializer.path.pop();
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeStruct::end(self)
    }
}

/// A helper [`Serializer`][serde::Serializer] for extracting string keys
/// from maps.
struct KeyExtractor {
    key: String,
}

impl serde::Serializer for &mut KeyExtractor {
    type Ok = ();
    type Error = QueryParamError;

    type SerializeSeq = Impossible<(), QueryParamError>;
    type SerializeTuple = Impossible<(), QueryParamError>;
    type SerializeTupleStruct = Impossible<(), QueryParamError>;
    type SerializeTupleVariant = Impossible<(), QueryParamError>;
    type SerializeMap = Impossible<(), QueryParamError>;
    type SerializeStruct = Impossible<(), QueryParamError>;
    type SerializeStructVariant = Impossible<(), QueryParamError>;

    fn serialize_bool(self, _: bool) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_i8(self, _: i8) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_i16(self, _: i16) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_i32(self, _: i32) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_i64(self, _: i64) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_u8(self, _: u8) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_u16(self, _: u16) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_u32(self, _: u32) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_u64(self, _: u64) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_f32(self, _: f32) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_f64(self, _: f64) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_char(self, _: char) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        self.key = v.to_owned();
        Ok(())
    }

    fn serialize_bytes(self, _: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(QueryParamError::MapKeyNotString)
    }
}

/// An error that occurs during query parameter serialization.
#[derive(Debug, thiserror::Error)]
pub enum QueryParamError {
    #[error("can't serialize {0} as query parameter")]
    UnsupportedType(#[from] UnsupportedTypeError),
    #[error("style-exploded combination not defined by OpenAPI")]
    UnspecifiedStyleExploded,
    #[error("map keys must be strings")]
    MapKeyNotString,
    #[error("{0}")]
    Custom(String),
}

impl serde::ser::Error for QueryParamError {
    fn custom<T: std::fmt::Display>(err: T) -> Self {
        Self::Custom(err.to_string())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UnsupportedTypeError {
    #[error("bytes")]
    Bytes,
    #[error("unit")]
    Unit,
    #[error("unit struct `{0}`")]
    UnitStruct(&'static str),
    #[error("tuple struct `{0}`")]
    TupleStruct(&'static str),
    #[error("newtype variant `{1}` of `{0}`")]
    NewtypeVariant(&'static str, &'static str),
    #[error("tuple variant `{1}` of `{0}`")]
    TupleVariant(&'static str, &'static str),
    #[error("struct variant `{1}` of `{0}`")]
    StructVariant(&'static str, &'static str),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
    use url::Url;

    #[test]
    fn test_integer() {
        let mut url = Url::parse("http://example.com/").unwrap();
        QuerySerializer::new(&mut url).append("limit", &42).unwrap();
        assert_eq!(url.query(), Some("limit=42"));
    }

    #[test]
    fn test_string() {
        let mut url = Url::parse("http://example.com/").unwrap();
        QuerySerializer::new(&mut url)
            .append("name", &"Alice")
            .unwrap();
        assert_eq!(url.query(), Some("name=Alice"));
    }

    #[test]
    fn test_bool() {
        let mut url = Url::parse("http://example.com/").unwrap();
        QuerySerializer::new(&mut url)
            .append("active", &true)
            .unwrap();
        assert_eq!(url.query(), Some("active=true"));
    }

    #[test]
    fn test_option_some() {
        let mut url = Url::parse("http://example.com/").unwrap();
        let value = Some(42);
        QuerySerializer::new(&mut url)
            .append("limit", &value)
            .unwrap();
        assert_eq!(url.query(), Some("limit=42"));
    }

    #[test]
    fn test_option_none() {
        let mut url = Url::parse("http://example.com/").unwrap();
        let value: Option<i32> = None;
        QuerySerializer::new(&mut url)
            .append("limit", &value)
            .unwrap();
        assert_eq!(url.query(), None);
    }

    #[test]
    fn test_array_form_exploded() {
        let mut url = Url::parse("http://example.com/").unwrap();
        let values = vec![1, 2, 3];
        QuerySerializer::new(&mut url)
            .append("ids", &values)
            .unwrap();
        assert_eq!(url.query(), Some("ids=1&ids=2&ids=3"));
    }

    #[test]
    fn test_array_form_non_exploded() {
        let mut url = Url::parse("http://example.com/").unwrap();
        let values = vec![1, 2, 3];
        QuerySerializer::new(&mut url)
            .style(QueryStyle::Form { exploded: false })
            .append("ids", &values)
            .unwrap();
        assert_eq!(url.query(), Some("ids=1,2,3"));
    }

    #[test]
    fn test_array_space_delimited() {
        let mut url = Url::parse("http://example.com/").unwrap();
        let values = vec![1, 2, 3];
        QuerySerializer::new(&mut url)
            .style(QueryStyle::SpaceDelimited)
            .append("ids", &values)
            .unwrap();
        assert_eq!(url.query(), Some("ids=1%202%203"));
    }

    #[test]
    fn test_array_pipe_delimited() {
        let mut url = Url::parse("http://example.com/").unwrap();
        let values = vec![1, 2, 3];
        QuerySerializer::new(&mut url)
            .style(QueryStyle::PipeDelimited)
            .append("ids", &values)
            .unwrap();
        assert_eq!(url.query(), Some("ids=1%7C2%7C3"));
    }

    #[test]
    fn test_empty_array() {
        let mut url = Url::parse("http://example.com/").unwrap();
        let values: Vec<i32> = vec![];
        QuerySerializer::new(&mut url)
            .append("ids", &values)
            .unwrap();
        assert_eq!(url.query(), None);
    }

    #[test]
    fn test_object_form_exploded() {
        #[derive(Serialize)]
        struct Person {
            first_name: String,
            last_name: String,
        }

        let mut url = Url::parse("http://example.com/").unwrap();
        let person = Person {
            first_name: "John".to_owned(),
            last_name: "Doe".to_owned(),
        };
        QuerySerializer::new(&mut url)
            .append("person", &person)
            .unwrap();
        assert_eq!(url.query(), Some("first_name=John&last_name=Doe"));
    }

    #[test]
    fn test_object_form_non_exploded() {
        #[derive(Serialize)]
        struct Person {
            first_name: String,
            last_name: String,
        }

        let mut url = Url::parse("http://example.com/").unwrap();
        let person = Person {
            first_name: "John".to_owned(),
            last_name: "Doe".to_owned(),
        };
        QuerySerializer::new(&mut url)
            .style(QueryStyle::Form { exploded: false })
            .append("person", &person)
            .unwrap();
        assert_eq!(url.query(), Some("person=first_name,John,last_name,Doe"));
    }

    #[test]
    fn test_object_deep_object() {
        #[derive(Serialize)]
        struct Filter {
            #[serde(rename = "type")]
            type_field: String,
            location: String,
        }

        let mut url = Url::parse("http://example.com/").unwrap();
        let filter = Filter {
            type_field: "cocktail".to_owned(),
            location: "bar".to_owned(),
        };
        QuerySerializer::new(&mut url)
            .style(QueryStyle::DeepObject)
            .append("filter", &filter)
            .unwrap();
        assert_eq!(
            url.query(),
            Some("filter%5Btype%5D=cocktail&filter%5Blocation%5D=bar")
        );
    }

    #[test]
    fn test_multiple_params_chained() {
        let mut url = Url::parse("http://example.com/").unwrap();
        let tags = vec!["dog", "cat"];
        QuerySerializer::new(&mut url)
            .append("limit", &10)
            .unwrap()
            .append("tags", &tags)
            .unwrap();
        assert_eq!(url.query(), Some("limit=10&tags=dog&tags=cat"));
    }

    #[test]
    fn test_string_with_special_chars() {
        let mut url = Url::parse("http://example.com/").unwrap();
        QuerySerializer::new(&mut url)
            .append("name", &"John Doe & Co.")
            .unwrap();
        assert_eq!(url.query(), Some("name=John+Doe+%26+Co."));
    }

    #[test]
    fn test_array_of_strings_with_special_chars() {
        let mut url = Url::parse("http://example.com/").unwrap();
        let values = vec!["hello world", "foo&bar"];
        QuerySerializer::new(&mut url)
            .style(QueryStyle::Form { exploded: false })
            .append("tags", &values)
            .unwrap();
        assert_eq!(url.query(), Some("tags=hello%20world,foo%26bar"));
    }

    #[test]
    fn test_nested_deep_object() {
        #[derive(Serialize)]
        struct Address {
            city: String,
            country: String,
        }

        #[derive(Serialize)]
        struct Person {
            name: String,
            address: Address,
        }

        let mut url = Url::parse("http://example.com/").unwrap();
        let person = Person {
            name: "Alice".to_owned(),
            address: Address {
                city: "Paris".to_owned(),
                country: "France".to_owned(),
            },
        };
        QuerySerializer::new(&mut url)
            .style(QueryStyle::DeepObject)
            .append("person", &person)
            .unwrap();
        assert_eq!(
            url.query(),
            Some(
                "person%5Bname%5D=Alice&person%5Baddress%5D%5Bcity%5D=Paris&person%5Baddress%5D%5Bcountry%5D=France"
            )
        );
    }

    #[test]
    fn test_deep_object_with_array_field() {
        #[derive(Serialize)]
        struct Filter {
            category: String,
            tags: Vec<String>,
        }

        let mut url = Url::parse("http://example.com/").unwrap();
        let filter = Filter {
            category: "electronics".to_owned(),
            tags: vec!["new".to_owned(), "sale".to_owned()],
        };
        QuerySerializer::new(&mut url)
            .style(QueryStyle::DeepObject)
            .append("filter", &filter)
            .unwrap();
        assert_eq!(
            url.query(),
            Some(
                "filter%5Bcategory%5D=electronics&filter%5Btags%5D%5B0%5D=new&filter%5Btags%5D%5B1%5D=sale"
            )
        );
    }

    #[test]
    fn test_serde_skip_if() {
        #[derive(Serialize)]
        struct Params {
            required: i32,
            #[serde(skip_serializing_if = "Option::is_none")]
            optional: Option<String>,
        }

        let mut url = Url::parse("http://example.com/").unwrap();
        let params = Params {
            required: 42,
            optional: None,
        };
        QuerySerializer::new(&mut url)
            .append("params", &params)
            .unwrap();
        assert_eq!(url.query(), Some("required=42"));
    }

    #[test]
    fn test_unit_variant_enum() {
        #[derive(Serialize)]
        #[allow(dead_code)]
        enum Status {
            Active,
            Inactive,
        }

        let mut url = Url::parse("http://example.com/").unwrap();
        QuerySerializer::new(&mut url)
            .append("status", &Status::Active)
            .unwrap();
        assert_eq!(url.query(), Some("status=Active"));
    }

    #[test]
    fn test_unicode_string() {
        let mut url = Url::parse("http://example.com/").unwrap();
        QuerySerializer::new(&mut url)
            .append("name", &"日本語")
            .unwrap();
        assert_eq!(url.query(), Some("name=%E6%97%A5%E6%9C%AC%E8%AA%9E"));
    }

    #[test]
    fn test_deep_object_rejects_arrays() {
        let mut url = Url::parse("http://example.com/").unwrap();
        let values = vec![1, 2, 3];
        let result = QuerySerializer::new(&mut url)
            .style(QueryStyle::DeepObject)
            .append("ids", &values);
        assert!(matches!(
            result,
            Err(QueryParamError::UnspecifiedStyleExploded)
        ));
    }

    #[test]
    fn test_space_delimited_object() {
        #[derive(Serialize)]
        struct Color {
            r: u32,
            g: u32,
            b: u32,
        }

        let mut url = Url::parse("http://example.com/").unwrap();
        let color = Color {
            r: 100,
            g: 200,
            b: 150,
        };
        QuerySerializer::new(&mut url)
            .style(QueryStyle::SpaceDelimited)
            .append("color", &color)
            .unwrap();

        // Per OpenAPI spec: `color=R%20100%20G%20200%20B%20150`.
        assert_eq!(url.query(), Some("color=r%20100%20g%20200%20b%20150"));
    }

    #[test]
    fn test_pipe_delimited_object() {
        #[derive(Serialize)]
        struct Color {
            r: u32,
            g: u32,
            b: u32,
        }

        let mut url = Url::parse("http://example.com/").unwrap();
        let color = Color {
            r: 100,
            g: 200,
            b: 150,
        };
        QuerySerializer::new(&mut url)
            .style(QueryStyle::PipeDelimited)
            .append("color", &color)
            .unwrap();

        // Per OpenAPI spec: `color=R%7C100%7CG%7C200%7CB%7C150`.
        assert_eq!(url.query(), Some("color=r%7C100%7Cg%7C200%7Cb%7C150"));
    }

    #[test]
    fn test_tuple_form_exploded() {
        let mut url = Url::parse("http://example.com/").unwrap();
        let coords = (42, 24, 10);
        QuerySerializer::new(&mut url)
            .append("coords", &coords)
            .unwrap();
        assert_eq!(url.query(), Some("coords=42&coords=24&coords=10"));
    }

    #[test]
    fn test_tuple_form_non_exploded() {
        let mut url = Url::parse("http://example.com/").unwrap();
        let coords = (42, 24, 10);
        QuerySerializer::new(&mut url)
            .style(QueryStyle::Form { exploded: false })
            .append("coords", &coords)
            .unwrap();
        assert_eq!(url.query(), Some("coords=42,24,10"));
    }

    #[test]
    fn test_tuple_space_delimited() {
        let mut url = Url::parse("http://example.com/").unwrap();
        let coords = (42, 24, 10);
        QuerySerializer::new(&mut url)
            .style(QueryStyle::SpaceDelimited)
            .append("coords", &coords)
            .unwrap();
        assert_eq!(url.query(), Some("coords=42%2024%2010"));
    }

    #[test]
    fn test_tuple_pipe_delimited() {
        let mut url = Url::parse("http://example.com/").unwrap();
        let coords = (42, 24, 10);
        QuerySerializer::new(&mut url)
            .style(QueryStyle::PipeDelimited)
            .append("coords", &coords)
            .unwrap();
        assert_eq!(url.query(), Some("coords=42%7C24%7C10"));
    }
}
