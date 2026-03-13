//! IR types after cooking, where type references are
//! node indices in the [`CookedGraph`][crate::ir::CookedGraph].

use petgraph::graph::NodeIndex;

use super::shape::{
    Container, InlineType, Inner, Operation, Parameter, ParameterInfo, Request, Response,
    SchemaType, Struct, StructField, Tagged, TaggedVariant, Untagged, UntaggedVariant,
};

/// A named schema type with graph node references.
pub type CookedSchemaType<'a> = SchemaType<'a, NodeIndex<usize>>;

/// An array, map, or optional type with graph node references.
pub type CookedContainer<'a> = Container<'a, NodeIndex<usize>>;

/// A struct type with graph node references.
pub type CookedStruct<'a> = Struct<'a, NodeIndex<usize>>;

/// A struct field with graph node references.
pub type CookedStructField<'a> = StructField<'a, NodeIndex<usize>>;

/// A tagged union with graph node references.
pub type CookedTagged<'a> = Tagged<'a, NodeIndex<usize>>;

/// A variant of a tagged union with graph node references.
pub type CookedTaggedVariant<'a> = TaggedVariant<'a, NodeIndex<usize>>;

/// An untagged union with graph node references.
pub type CookedUntagged<'a> = Untagged<'a, NodeIndex<usize>>;

/// A variant of an untagged union with graph node references.
pub type CookedUntaggedVariant = UntaggedVariant<NodeIndex<usize>>;

/// An inline type with graph node references.
pub type CookedInlineType<'a> = InlineType<'a, NodeIndex<usize>>;

/// The inner type of a [`Container`] with graph node references.
pub type CookedInner<'a> = Inner<'a, NodeIndex<usize>>;

/// An operation with graph node references.
pub type CookedOperation<'a> = Operation<'a, NodeIndex<usize>>;

/// An operation parameter with graph node references.
pub type CookedParameter<'a> = Parameter<'a, NodeIndex<usize>>;

/// Information about a [`Parameter`] with graph node references.
pub type CookedParameterInfo<'a> = ParameterInfo<'a, NodeIndex<usize>>;

/// An operation request body with graph node references.
pub type CookedRequest = Request<NodeIndex<usize>>;

/// An operation response with graph node references.
pub type CookedResponse = Response<NodeIndex<usize>>;
