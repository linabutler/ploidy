use petgraph::graph::NodeIndex;

use crate::ir::{
    graph::CookedGraph,
    types::{Enum, EnumVariant},
};

use super::ViewNode;

/// A graph-aware view of an [`Enum`].
#[derive(Debug)]
pub struct EnumView<'a> {
    cooked: &'a CookedGraph<'a>,
    index: NodeIndex<usize>,
    ty: &'a Enum<'a>,
}

impl<'a> EnumView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: &'a Enum<'a>,
    ) -> Self {
        Self { cooked, index, ty }
    }

    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.ty.description
    }

    #[inline]
    pub fn variants(&self) -> &'a [EnumVariant<'a>] {
        self.ty.variants
    }
}

impl<'a> ViewNode<'a> for EnumView<'a> {
    #[inline]
    fn cooked(&self) -> &'a CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}
