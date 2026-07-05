//! Language-agnostic intermediate representation types.

use std::{
    cmp::Ordering as CmpOrdering,
    fmt::{self, Display},
    hash::{Hash, Hasher},
    num::NonZeroUsize,
    ops::Deref,
    sync::atomic::{AtomicUsize, Ordering as AtomicOrdering},
};

use ref_cast::{RefCastCustom, ref_cast_custom};

use crate::{arena::Arena, ir::views::TypeId};

pub use self::{graph::*, spec::*};

mod graph;
pub mod shape;
mod spec;

/// Metadata for a named schema type.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SchemaTypeInfo<'a> {
    /// The name of the schema type.
    pub name: &'a str,
    /// The `x-resourceId` extension value, if present.
    pub resource: Option<&'a str>,
}

/// Generates unique opaque identities for inline types.
#[derive(Clone, Copy, Debug)]
pub struct InlineTypeIds<'a>(&'a AtomicUsize);

impl<'a> InlineTypeIds<'a> {
    #[inline]
    pub fn new(arena: &'a Arena) -> Self {
        Self(arena.alloc_atomic(0))
    }

    #[inline]
    pub fn next(&self) -> InlineTypeId {
        InlineTypeId(self.0.fetch_add(1, AtomicOrdering::Relaxed))
    }
}

/// Opaque identity for an inline type.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InlineTypeId(usize);

/// An `operationId` from the OpenAPI spec.
#[derive(Debug, Eq, Hash, Ord, PartialEq, PartialOrd, RefCastCustom)]
#[repr(transparent)]
pub struct OperationId(str);

impl OperationId {
    #[ref_cast_custom]
    #[inline]
    pub(in crate::ir) fn new(s: &str) -> &Self;
}

impl Deref for OperationId {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq<str> for OperationId {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.0 == *other
    }
}

impl Display for OperationId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The root of an inline type path.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InlineTypePathRoot<'a, S, O> {
    Schema(S),
    Operation {
        resource: Option<&'a str>,
        id: O,
        usage: OperationUsage<'a>,
    },
}

/// How an operation uses an inline type.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum OperationUsage<'a> {
    /// A path parameter with the given name.
    Path(&'a str),
    /// A query parameter with the given name.
    Query(&'a str),
    /// The request body.
    Request,
    /// The response body.
    Response {
        /// The response status code, when needed to distinguish response bodies.
        status: Option<u16>,
    },
}

/// A segment in an inline type path.
///
/// Segments scoped to a parent type carry that parent type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InlineTypePathSegment<'a> {
    /// Enters an inline type declared as a struct field.
    Field(TypeId, StructFieldName<'a>),
    /// Enters an inline type declared as a tagged union variant.
    TaggedVariant(TypeId, &'a str),
    /// Enters the nth untagged union variant, counted from 1 in declaration order.
    UntaggedVariant(TypeId, NonZeroUsize),
    /// Enters the item type of an array.
    ArrayItem,
    /// Enters the value type of a map.
    MapValue,
    /// Enters the inner type of an optional container.
    Optional,
    /// Enters the nth inherited parent, counted from 1 in declaration order.
    Inherits(TypeId, NonZeroUsize),
}

/// A primitive type in the dependency graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PrimitiveType {
    String,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    Bool,
    DateTime,
    UnixTime,
    Date,
    Url,
    Uuid,
    Bytes,
    Binary,
}

/// An enum type in the dependency graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Enum<'a> {
    pub description: Option<&'a str>,
    pub variants: &'a [EnumVariant<'a>],
}

/// A variant of an enum.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EnumVariant<'a> {
    String(&'a str),
    I64(i64),
    U64(u64),
    F64(JsonF64),
    Bool(bool),
}

/// A struct field name.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StructFieldName<'a> {
    /// A name declared by a property or schema reference.
    Name(&'a str),
    /// A synthetic name, counted from 1 in declaration order.
    Ordinal(NonZeroUsize),
    /// The synthetic field for additional properties.
    AdditionalProperties,
}

/// The serialization style for query parameters.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ParameterStyle {
    Form { exploded: bool },
    PipeDelimited,
    SpaceDelimited,
    DeepObject,
}

/// A floating-point number that's representable in JSON.
///
/// JSON doesn't allow `NaN`, so unlike [`f64`], [`JsonF64`]
/// implements [`Eq`] and [`Ord`]. [`JsonF64`] is functionally
/// equivalent to [`serde_json::Number`], but is [`Copy`].
#[derive(Clone, Copy, Debug)]
pub struct JsonF64(f64);

impl JsonF64 {
    pub(crate) fn new(f: f64) -> Self {
        assert!(!f.is_nan());
        Self(f)
    }

    #[inline]
    pub fn to_f64(self) -> f64 {
        self.into()
    }
}

impl Eq for JsonF64 {}

impl From<JsonF64> for f64 {
    #[inline]
    fn from(value: JsonF64) -> Self {
        value.0
    }
}

impl Hash for JsonF64 {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        // `+0.0` and `-0.0` compare equal, but have different bit layouts;
        // use the `+0.0` hash for both to uphold the property that
        // `k1 == k2 -> hash(k1) == hash(k2)`.
        let value = if self.0 == 0.0 { 0.0 } else { self.0 };
        value.to_bits().hash(state);
    }
}

impl Ord for JsonF64 {
    #[inline]
    fn cmp(&self, other: &Self) -> CmpOrdering {
        // JSON numbers can't be `NaN`, so `unwrap()` is OK.
        self.0.partial_cmp(&other.0).unwrap()
    }
}

impl PartialEq for JsonF64 {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl PartialOrd for JsonF64 {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}
