use petgraph::graph::NodeIndex;

use crate::ir::{
    UntaggedVariantNameHint,
    graph::CookedGraph,
    types::{GraphUntagged, GraphUntaggedVariant},
};

use super::{ViewNode, ir::TypeView};

/// A graph-aware view of an [untagged union type][GraphUntagged].
#[derive(Debug)]
pub struct UntaggedView<'a> {
    cooked: &'a CookedGraph<'a>,
    index: NodeIndex<usize>,
    ty: GraphUntagged<'a>,
}

impl<'a> UntaggedView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: GraphUntagged<'a>,
    ) -> Self {
        Self { cooked, index, ty }
    }

    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.ty.description
    }

    /// Returns an iterator over this untagged union's variants.
    #[inline]
    pub fn variants(&self) -> impl Iterator<Item = UntaggedVariantView<'_, 'a>> {
        self.ty.variants.iter().map(|variant| UntaggedVariantView {
            parent: self,
            variant,
        })
    }
}

impl<'a> ViewNode<'a> for UntaggedView<'a> {
    #[inline]
    fn cooked(&self) -> &'a CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}

/// A graph-aware view of an [untagged union variant][GraphUntaggedVariant].
#[derive(Debug)]
pub struct UntaggedVariantView<'view, 'a> {
    parent: &'view UntaggedView<'a>,
    variant: &'a GraphUntaggedVariant,
}

impl<'view, 'a> UntaggedVariantView<'view, 'a> {
    /// Returns a view of this variant's type, if it's not `null`.
    #[inline]
    pub fn ty(&self) -> Option<SomeUntaggedVariant<'a>> {
        match self.variant {
            &GraphUntaggedVariant::Some(hint, index) => Some(SomeUntaggedVariant {
                hint,
                view: TypeView::new(self.parent.cooked, index),
            }),
            GraphUntaggedVariant::Null => None,
        }
    }
}

#[derive(Debug)]
pub struct SomeUntaggedVariant<'a> {
    pub hint: UntaggedVariantNameHint,
    pub view: TypeView<'a>,
}
