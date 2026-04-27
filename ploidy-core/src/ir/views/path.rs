//! Lazy inline path views.
//!
//! [`InlinePathView`] resolves compact [`InlinePath`] storage into
//! [`InlinePathRoot`] and [`InlinePathSegment`] values that codegen
//! consumes, without eagerly materializing them during BFS.

use itertools::Itertools;
use petgraph::visit::EdgeRef;

use crate::ir::{
    InlineTypeId, InlineTypePathOperation, InlineTypePathRoot, InlineTypePathSegment, OperationId,
    graph::{CookedGraph, GraphEdge},
    types::{GraphContainer, GraphInlineType, GraphSchemaType, GraphType, VariantMeta},
};

use super::TypeViewId;

/// A lazy view over an [`InlinePath`] that resolves graph indices
/// into [`InlinePathRoot`] and [`InlinePathSegment`] on demand.
#[derive(Clone, Copy, Debug)]
pub struct InlineTypePathView<'graph, 'a> {
    cooked: &'graph CookedGraph<'a>,
    id: InlineTypeId,
}

impl<'graph, 'a> InlineTypePathView<'graph, 'a> {
    /// Creates a new path view from a graph and compact path.
    #[inline]
    pub(in crate::ir) fn new(cooked: &'graph CookedGraph<'a>, id: InlineTypeId) -> Self {
        Self { cooked, id }
    }

    /// Resolves the root context for this inline path.
    #[inline]
    pub fn root(&self) -> InlineTypePathRoot<'a, TypeViewId, &'a OperationId> {
        match self.cooked.metadata.paths[&self.id].root {
            InlineTypePathRoot::Schema(index) => InlineTypePathRoot::Schema(TypeViewId(index)),
            InlineTypePathRoot::Operation(op) => {
                InlineTypePathRoot::Operation(InlineTypePathOperation {
                    id: OperationId::new(op.id),
                    resource: op.resource,
                    role: op.role,
                })
            }
        }
    }

    /// Returns an iterator that resolves each edge index into an
    /// [`InlinePathSegment`].
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
                        InlineTypePathSegment::Field(TypeViewId(from), meta.name)
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
                        InlineTypePathSegment::TaggedVariant(TypeViewId(from), m.name)
                    }
                    GraphEdge::Variant(VariantMeta::Untagged(_)) => {
                        // Derive 1-indexed position from edge order among
                        // `Variant` edges on the source node.
                        let (index, _) = cooked
                            .graph
                            .edges(from)
                            .filter(|e| matches!(e.weight(), GraphEdge::Variant(_)))
                            .find_position(|e| e.target() == to)?;
                        InlineTypePathSegment::UntaggedVariant(index + 1)
                    }
                    GraphEdge::Inherits { .. } => {
                        // Derive 1-indexed position from edge order among
                        // `Inherits` edges on the source node.
                        let (index, _) = cooked
                            .graph
                            .edges(from)
                            .filter(|e| matches!(e.weight(), GraphEdge::Inherits { .. }))
                            .find_position(|e| e.target() == to)?;
                        InlineTypePathSegment::Inherits(index + 1)
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
