//! IR types with graph node references.

use petgraph::graph::NodeIndex;

use super::shape::{
    Container, InlineType, Inner, Operation, Parameter, ParameterInfo, Request, Response,
    SchemaType, Struct, StructField, Tagged, TaggedVariant, Untagged, UntaggedVariant,
};

/// A type in the dependency graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GraphType<'a> {
    Schema(GraphSchemaType<'a>),
    Inline(GraphInlineType<'a>),
}

/// A named schema type with graph node references.
pub type GraphSchemaType<'a> = SchemaType<'a, NodeIndex<usize>>;

/// An array, map, or optional type with graph node references.
pub type GraphContainer<'a> = Container<'a, NodeIndex<usize>>;

/// A struct type with graph node references.
pub type GraphStruct<'a> = Struct<'a, NodeIndex<usize>>;

/// A struct field with graph node references.
pub type GraphStructField<'a> = StructField<'a, NodeIndex<usize>>;

/// A tagged union with graph node references.
pub type GraphTagged<'a> = Tagged<'a, NodeIndex<usize>>;

/// A variant of a tagged union with graph node references.
pub type GraphTaggedVariant<'a> = TaggedVariant<'a, NodeIndex<usize>>;

/// An untagged union with graph node references.
pub type GraphUntagged<'a> = Untagged<'a, NodeIndex<usize>>;

/// A variant of an untagged union with graph node references.
pub type GraphUntaggedVariant = UntaggedVariant<NodeIndex<usize>>;

/// An inline type with graph node references.
pub type GraphInlineType<'a> = InlineType<'a, NodeIndex<usize>>;

/// The type contained within an array, map, or optional type,
/// with graph node references.
pub type GraphInner<'a> = Inner<'a, NodeIndex<usize>>;

/// An operation with graph node references.
pub type GraphOperation<'a> = Operation<'a, NodeIndex<usize>>;

/// A path or query parameter with graph node references.
pub type GraphParameter<'a> = Parameter<'a, NodeIndex<usize>>;

/// The name, type, and metadata of an operation parameter,
/// with graph node references.
pub type GraphParameterInfo<'a> = ParameterInfo<'a, NodeIndex<usize>>;

/// A request body with graph node references.
pub type GraphRequest = Request<NodeIndex<usize>>;

/// A response body with graph node references.
pub type GraphResponse = Response<NodeIndex<usize>>;
