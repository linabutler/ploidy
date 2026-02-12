use petgraph::graph::NodeIndex;

use crate::ir::{
    graph::IrGraph,
    types::{IrTagged, IrTaggedVariant},
};

use super::{ViewNode, ir::IrTypeView};

/// A graph-aware view of an [`IrTagged`] union.
#[derive(Debug)]
pub struct IrTaggedView<'a> {
    graph: &'a IrGraph<'a>,
    index: NodeIndex<usize>,
    ty: &'a IrTagged<'a>,
}

impl<'a> IrTaggedView<'a> {
    pub(in crate::ir) fn new(
        graph: &'a IrGraph<'a>,
        index: NodeIndex<usize>,
        ty: &'a IrTagged<'a>,
    ) -> Self {
        Self { graph, index, ty }
    }

    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.ty.description
    }

    #[inline]
    pub fn tag(&self) -> &'a str {
        self.ty.tag
    }

    /// Returns an iterator over this tagged union's variants.
    pub fn variants(&self) -> impl Iterator<Item = IrTaggedVariantView<'a>> {
        self.ty.variants.iter().map(move |variant| {
            let node = self.graph.resolve_type(variant.ty.as_ref());
            IrTaggedVariantView::new(self.graph, self.graph.indices[&node], variant)
        })
    }
}

impl<'a> ViewNode<'a> for IrTaggedView<'a> {
    #[inline]
    fn graph(&self) -> &'a IrGraph<'a> {
        self.graph
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}

/// A graph-aware view of an [`IrTaggedVariant`].
#[derive(Debug)]
pub struct IrTaggedVariantView<'a> {
    graph: &'a IrGraph<'a>,
    index: NodeIndex<usize>,
    variant: &'a IrTaggedVariant<'a>,
}

impl<'a> IrTaggedVariantView<'a> {
    fn new(
        graph: &'a IrGraph<'a>,
        index: NodeIndex<usize>,
        variant: &'a IrTaggedVariant<'a>,
    ) -> Self {
        Self {
            graph,
            index,
            variant,
        }
    }

    #[inline]
    pub fn name(&self) -> &'a str {
        self.variant.name
    }

    #[inline]
    pub fn aliases(&self) -> &'a [&'a str] {
        &self.variant.aliases
    }

    /// Returns a view of this variant's type.
    pub fn ty(&self) -> IrTypeView<'a> {
        let node = self.graph.resolve_type(self.variant.ty.as_ref());
        IrTypeView::new(self.graph, self.graph.indices[&node])
    }
}

impl<'a> ViewNode<'a> for IrTaggedVariantView<'a> {
    #[inline]
    fn graph(&self) -> &'a IrGraph<'a> {
        self.graph
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}
