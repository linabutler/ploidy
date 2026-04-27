//! Language-agnostic intermediate representation types.

use std::{
    cmp::Ordering,
    fmt::{self, Display},
    hash::{Hash, Hasher},
    ops::Deref,
};

use ref_cast::{RefCastCustom, ref_cast_custom};

use super::views::TypeViewId;

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

/// Opaque identity for an inline type node.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InlineTypeId(usize);

impl InlineTypeId {
    /// Creates a new inline type ID from a raw index.
    #[inline]
    pub(in crate::ir) fn new(id: usize) -> Self {
        Self(id)
    }
}

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

/// A structural step from a parent type to a child inline type,
/// derived from a single graph edge during the canonical-path BFS.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InlineTypePathSegment<'a> {
    /// A struct field edge. Carries the parent [`TypeViewId`] so
    /// codegen can resolve the uniquified field name.
    Field(TypeViewId, StructFieldName<'a>),
    /// A tagged union variant edge. Carries the parent [`TypeViewId`]
    /// so codegen can resolve the uniquified variant name.
    TaggedVariant(TypeViewId, &'a str),
    /// An untagged union variant edge; 1-indexed from edge position.
    UntaggedVariant(usize),
    /// Array contains its item type.
    ArrayItem,
    /// Map contains its value type.
    MapValue,
    /// Optional contains its inner type. Naming-invisible — produces
    /// no name segment in Rust, but preserves path continuity so
    /// the opaque ID disambiguates the wrapper from its inner node.
    Optional,
    /// Inherits from the n-th `allOf` parent (1-indexed).
    Inherits(usize),
}

/// The root context for an inline path. Determines both the module
/// path and the type-name prefix.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InlineTypePathRoot<'a, S, O> {
    Schema(S),
    Operation(InlineTypePathOperation<'a, O>),
}

impl<'a, S, O> From<InlineTypePathOperation<'a, O>> for InlineTypePathRoot<'a, S, O> {
    fn from(op: InlineTypePathOperation<'a, O>) -> Self {
        Self::Operation(op)
    }
}

/// An inline type under an operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InlineTypePathOperation<'a, Id> {
    /// The `operationId`, which drives the type-name prefix.
    pub id: Id,
    /// The resource name from `x-resource-name`, which drives the
    /// module path. `None` falls back to `"default"`.
    pub resource: Option<&'a str>,
    /// The role of this inline within the operation.
    pub role: OperationRole<'a>,
}

/// The role of an inline type root within an operation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum OperationRole<'a> {
    /// A query parameter with the given name.
    Path(&'a str),
    /// A query parameter with the given name.
    Query(&'a str),
    /// The request body.
    Request,
    /// The response body.
    Response,
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

/// A hint that's used to generate a more descriptive name
/// for an untagged union variant. These are emitted for
/// `oneOf` schemas without a discriminator.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UntaggedVariantNameHint {
    Primitive(PrimitiveType),
    Array,
    Map,
    Index(usize),
}

/// A struct field name, either explicit or generated from a hint.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StructFieldName<'a> {
    /// Explicit name from a schema or reference.
    Name(&'a str),
    /// Generated name, deferred until generation time.
    Hint(StructFieldNameHint),
}

/// A hint that's used to generate a name for a struct field.
/// These are emitted for inline `anyOf` schemas and
/// additional properties fields.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StructFieldNameHint {
    Index(usize),
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
    fn cmp(&self, other: &Self) -> Ordering {
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
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
