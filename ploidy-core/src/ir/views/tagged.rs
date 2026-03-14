use petgraph::graph::NodeIndex;

use crate::ir::{
    graph::CookedGraph,
    types::{GraphTagged, GraphTaggedVariant},
};

use super::{ViewNode, ir::TypeView};

/// A graph-aware view of a [tagged union type][GraphTagged].
#[derive(Debug)]
pub struct TaggedView<'a> {
    cooked: &'a CookedGraph<'a>,
    index: NodeIndex<usize>,
    ty: GraphTagged<'a>,
}

impl<'a> TaggedView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: GraphTagged<'a>,
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
    pub fn variants(&self) -> impl Iterator<Item = TaggedVariantView<'a>> {
        self.ty
            .variants
            .iter()
            .map(move |variant| TaggedVariantView::new(self.cooked, variant.ty, variant))
    }
}

impl<'a> ViewNode<'a> for TaggedView<'a> {
    #[inline]
    fn cooked(&self) -> &'a CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}

/// A graph-aware view of a [tagged union variant][GraphTaggedVariant].
#[derive(Debug)]
pub struct TaggedVariantView<'a> {
    cooked: &'a CookedGraph<'a>,
    index: NodeIndex<usize>,
    variant: &'a GraphTaggedVariant<'a>,
}

impl<'a> TaggedVariantView<'a> {
    #[inline]
    fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        variant: &'a GraphTaggedVariant<'a>,
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
        self.variant.aliases
    }

    /// Returns a view of this variant's type.
    #[inline]
    pub fn ty(&self) -> TypeView<'a> {
        TypeView::new(self.cooked, self.variant.ty)
    }
}

impl<'a> ViewNode<'a> for TaggedVariantView<'a> {
    #[inline]
    fn cooked(&self) -> &'a CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}
