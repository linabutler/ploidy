use petgraph::graph::NodeIndex;

use crate::ir::{
    IrUntaggedVariantNameHint,
    graph::CookedGraph,
    types::{IrUntagged, IrUntaggedVariant},
};

use super::{ViewNode, ir::IrTypeView};

/// A graph-aware view of an [`IrUntagged`] union.
#[derive(Debug)]
pub struct IrUntaggedView<'a> {
    cooked: &'a CookedGraph<'a>,
    index: NodeIndex<usize>,
    ty: &'a IrUntagged<'a, NodeIndex<usize>>,
}

impl<'a> IrUntaggedView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: &'a IrUntagged<'a, NodeIndex<usize>>,
    ) -> Self {
        Self { cooked, index, ty }
    }

    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.ty.description
    }

    /// Returns an iterator over this untagged union's variants.
    #[inline]
    pub fn variants(&self) -> impl Iterator<Item = IrUntaggedVariantView<'_, 'a>> {
        self.ty
            .variants
            .iter()
            .map(|variant| IrUntaggedVariantView {
                parent: self,
                variant,
            })
    }
}

impl<'a> ViewNode<'a> for IrUntaggedView<'a> {
    #[inline]
    fn cooked(&self) -> &'a CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}

/// A graph-aware view of an [`IrUntaggedVariant`].
#[derive(Debug)]
pub struct IrUntaggedVariantView<'view, 'a> {
    parent: &'view IrUntaggedView<'a>,
    variant: &'a IrUntaggedVariant<NodeIndex<usize>>,
}

impl<'view, 'a> IrUntaggedVariantView<'view, 'a> {
    /// Returns a view of this variant's type, if it's not `null`.
    #[inline]
    pub fn ty(&self) -> Option<SomeIrUntaggedVariant<'a>> {
        match self.variant {
            &IrUntaggedVariant::Some(hint, index) => Some(SomeIrUntaggedVariant {
                hint,
                view: IrTypeView::new(self.parent.cooked, index),
            }),
            IrUntaggedVariant::Null => None,
        }
    }
}

#[derive(Debug)]
pub struct SomeIrUntaggedVariant<'a> {
    pub hint: IrUntaggedVariantNameHint,
    pub view: IrTypeView<'a>,
}
