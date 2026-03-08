use petgraph::graph::NodeIndex;

use crate::ir::{
    graph::CookedGraph,
    types::{IrTagged, IrTaggedVariant},
};

use super::{ViewNode, ir::IrTypeView};

/// A graph-aware view of an [`IrTagged`] union.
#[derive(Debug)]
pub struct IrTaggedView<'a> {
    cooked: &'a CookedGraph<'a>,
    index: NodeIndex<usize>,
    ty: &'a IrTagged<'a>,
}

impl<'a> IrTaggedView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: &'a IrTagged<'a>,
    ) -> Self {
        Self { cooked, index, ty }
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
    #[inline]
    pub fn variants(&self) -> impl Iterator<Item = IrTaggedVariantView<'a>> {
        self.ty.variants.iter().map(move |variant| {
            let node = self.cooked.resolve(&variant.ty);
            IrTaggedVariantView::new(self.cooked, self.cooked.indices[&node], variant)
        })
    }
}

impl<'a> ViewNode<'a> for IrTaggedView<'a> {
    #[inline]
    fn cooked(&self) -> &'a CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}

/// A graph-aware view of an [`IrTaggedVariant`].
#[derive(Debug)]
pub struct IrTaggedVariantView<'a> {
    cooked: &'a CookedGraph<'a>,
    index: NodeIndex<usize>,
    variant: &'a IrTaggedVariant<'a>,
}

impl<'a> IrTaggedVariantView<'a> {
    #[inline]
    fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        variant: &'a IrTaggedVariant<'a>,
    ) -> Self {
        Self {
            cooked,
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
    #[inline]
    pub fn ty(&self) -> IrTypeView<'a> {
        let node = self.cooked.resolve(&self.variant.ty);
        IrTypeView::new(self.cooked, self.cooked.indices[&node])
    }
}

impl<'a> ViewNode<'a> for IrTaggedVariantView<'a> {
    #[inline]
    fn cooked(&self) -> &'a CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}
