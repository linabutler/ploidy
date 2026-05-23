//! Inline type paths.

use std::num::NonZeroUsize;

use itertools::Itertools;
use petgraph::visit::EdgeRef;

use crate::ir::{
    InlineTypeId, InlineTypePathRoot, InlineTypePathSegment, OperationId,
    graph::{CookedGraph, GraphEdge},
    types::{GraphContainer, GraphInlineType, GraphSchemaType, GraphType, VariantMeta},
};

use super::TypeId;

/// A view of a canonical inline type path.
#[derive(Clone, Copy, Debug)]
pub struct InlineTypePathView<'graph, 'a> {
    cooked: &'graph CookedGraph<'a>,
    id: InlineTypeId,
}

impl<'graph, 'a> InlineTypePathView<'graph, 'a> {
    #[inline]
    pub(in crate::ir) fn new(cooked: &'graph CookedGraph<'a>, id: InlineTypeId) -> Self {
        Self { cooked, id }
    }

    /// Returns the root of this path.
    #[inline]
    pub fn root(&self) -> InlineTypePathRoot<'a, TypeId, &'a OperationId> {
        match self.cooked.metadata.paths[&self.id].root {
            InlineTypePathRoot::Schema(index) => InlineTypePathRoot::Schema(TypeId(index)),
            InlineTypePathRoot::Operation {
                id,
                resource,
                usage,
            } => InlineTypePathRoot::Operation {
                id: OperationId::new(id),
                resource,
                usage,
            },
        }
    }

    /// Returns an iterator over this path's segments.
    #[inline]
    pub fn segments(&self) -> impl Iterator<Item = InlineTypePathSegment<'a>> + use<'graph, 'a> {
        let cooked = self.cooked;
        cooked.metadata.paths[&self.id]
            .edges
            .iter()
            .filter_map(|&index| {
                let (from, to) = cooked.graph.edge_endpoints(index)?;
                Some(match cooked.graph[index] {
                    GraphEdge::Field { meta, .. } => {
                        InlineTypePathSegment::Field(TypeId(from), meta.name)
                    }
                    GraphEdge::Contains => {
                        let source = match cooked.graph[from] {
                            GraphType::Schema(GraphSchemaType::Container(_, container)) => {
                                container
                            }
                            GraphType::Inline(GraphInlineType::Container(_, container)) => {
                                container
                            }
                            _ => return None,
                        };
                        match source {
                            GraphContainer::Array { .. } => InlineTypePathSegment::ArrayItem,
                            GraphContainer::Map { .. } => InlineTypePathSegment::MapValue,
                            GraphContainer::Optional { .. } => InlineTypePathSegment::Optional,
                        }
                    }
                    GraphEdge::Variant(VariantMeta::Tagged(m)) => {
                        InlineTypePathSegment::TaggedVariant(TypeId(from), m.name)
                    }
                    GraphEdge::Variant(VariantMeta::Untagged(m)) => {
                        InlineTypePathSegment::UntaggedVariant(TypeId(from), m.ordinal)
                    }
                    GraphEdge::Inherits { .. } => {
                        let (index, _) = cooked
                            .graph
                            .edges(from)
                            .filter(|e| matches!(e.weight(), GraphEdge::Inherits { .. }))
                            .find_position(|e| e.target() == to)?;
                        InlineTypePathSegment::Inherits(
                            TypeId(from),
                            NonZeroUsize::new(index + 1).unwrap(),
                        )
                    }
                })
            })
    }

    /// Returns the number of edge indices in the path.
    #[inline]
    pub fn len(&self) -> usize {
        self.cooked.metadata.paths[&self.id].edges.len()
    }

    /// Returns `true` if the path has no edges.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.cooked.metadata.paths[&self.id].edges.is_empty()
    }
}
