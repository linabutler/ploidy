//! Transparent schema compositions.
//!
//! A [`CompositionView`] represents an `allOf` intersection without assuming
//! that the composed schemas are object-shaped. Struct views use inheritance
//! for authored object composition; this view preserves transparent wrappers,
//! such as `$ref` with adjacent schema keywords.

use itertools::Itertools;
use petgraph::{Direction, graph::NodeIndex, visit::EdgeRef};

use crate::ir::{
    graph::{CookedGraph, GraphEdge},
    types::GraphComposition,
};

use super::{TypeView, ViewNode};

/// A graph-aware view of a transparent schema composition.
#[derive(Debug)]
pub struct CompositionView<'graph, 'a> {
    cooked: &'graph CookedGraph<'a>,
    index: NodeIndex<usize>,
    ty: GraphComposition<'a>,
}

impl<'graph, 'a> CompositionView<'graph, 'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'graph CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: GraphComposition<'a>,
    ) -> Self {
        Self { cooked, index, ty }
    }

    /// Returns the human-readable description for this composition, if present.
    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.ty.description
    }

    /// Returns the composed schemas, in declaration order.
    #[inline]
    pub fn all_of(&self) -> impl Iterator<Item = TypeView<'graph, 'a>> + use<'graph, 'a> {
        self.cooked
            .graph
            .edges_directed(self.index, Direction::Outgoing)
            .filter(|e| matches!(e.weight(), GraphEdge::Composes))
            .map(|e| e.target())
            .collect_vec()
            .into_iter()
            .rev()
            .map(|index| TypeView::new(self.cooked, index))
    }
}

impl<'graph, 'a> ViewNode<'graph, 'a> for CompositionView<'graph, 'a> {
    #[inline]
    fn cooked(&self) -> &'graph CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}
