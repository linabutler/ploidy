use petgraph::graph::NodeIndex;

use crate::ir::{
    IrUntaggedVariantNameHint,
    graph::IrGraph,
    types::{IrUntagged, IrUntaggedVariant},
};

use super::{ViewNode, ir::IrTypeView};

/// A graph-aware view of an [`IrUntagged`] union.
#[derive(Debug)]
pub struct IrUntaggedView<'a> {
    graph: &'a IrGraph<'a>,
    index: NodeIndex<usize>,
    ty: &'a IrUntagged<'a>,
}

impl<'a> IrUntaggedView<'a> {
    pub(in crate::ir) fn new(
        graph: &'a IrGraph<'a>,
        index: NodeIndex<usize>,
        ty: &'a IrUntagged<'a>,
    ) -> Self {
        Self { graph, index, ty }
    }

    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.ty.description
    }

    /// Returns an iterator over this untagged union's variants.
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
    fn graph(&self) -> &'a IrGraph<'a> {
        self.graph
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
    variant: &'a IrUntaggedVariant<'a>,
}

impl<'view, 'a> IrUntaggedVariantView<'view, 'a> {
    /// Returns a view of this variant's type, if it's not `null`.
    pub fn ty(&self) -> Option<SomeIrUntaggedVariant<'a>> {
        match self.variant {
            IrUntaggedVariant::Some(hint, ty) => {
                let node = self.parent.graph.resolve_type(ty.as_ref());
                Some(SomeIrUntaggedVariant {
                    hint: *hint,
                    view: IrTypeView::new(self.parent.graph, self.parent.graph.indices[&node]),
                })
            }
            IrUntaggedVariant::Null => None,
        }
    }
}

#[derive(Debug)]
pub struct SomeIrUntaggedVariant<'a> {
    pub hint: IrUntaggedVariantNameHint,
    pub view: IrTypeView<'a>,
}
