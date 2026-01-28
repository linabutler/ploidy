use petgraph::graph::NodeIndex;

use crate::ir::{
    graph::{IrGraph, IrGraphNode},
    types::{IrStruct, IrStructField, IrStructFieldName},
};

use super::{ViewNode, ir::IrTypeView};

/// A graph-aware view of an [`IrStruct`].
#[derive(Debug)]
pub struct IrStructView<'a> {
    graph: &'a IrGraph<'a>,
    index: NodeIndex<usize>,
    ty: &'a IrStruct<'a>,
}

impl<'a> IrStructView<'a> {
    pub(in crate::ir) fn new(
        graph: &'a IrGraph<'a>,
        index: NodeIndex<usize>,
        ty: &'a IrStruct<'a>,
    ) -> Self {
        Self { graph, index, ty }
    }

    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.ty.description
    }

    /// Returns an iterator over this struct's fields.
    #[inline]
    pub fn fields(&self) -> impl Iterator<Item = IrStructFieldView<'_, 'a>> {
        self.ty.fields.iter().map(move |field| IrStructFieldView {
            parent: self,
            field,
        })
    }
}

impl<'a> ViewNode<'a> for IrStructView<'a> {
    #[inline]
    fn graph(&self) -> &'a IrGraph<'a> {
        self.graph
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}

/// A graph-aware view of an [`IrStructField`].
#[derive(Debug)]
pub struct IrStructFieldView<'view, 'a> {
    parent: &'view IrStructView<'a>,
    field: &'a IrStructField<'a>,
}

impl<'view, 'a> IrStructFieldView<'view, 'a> {
    #[inline]
    pub fn name(&self) -> IrStructFieldName<'a> {
        self.field.name
    }

    /// Returns a view of the inner type that this type wraps.
    #[inline]
    pub fn ty(&self) -> IrTypeView<'a> {
        let node = IrGraphNode::from_ref(self.parent.graph.spec, self.field.ty.as_ref());
        IrTypeView::new(self.parent.graph, self.parent.graph.indices[&node])
    }

    #[inline]
    pub fn required(&self) -> bool {
        self.field.required
    }

    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.field.description
    }

    #[inline]
    pub fn discriminator(&self) -> bool {
        self.field.discriminator
    }

    #[inline]
    pub fn flattened(&self) -> bool {
        self.field.flattened
    }

    /// Returns `true` if this field needs indirection to break a cycle.
    pub fn needs_indirection(&self) -> bool {
        let node = IrGraphNode::from_ref(self.parent.graph.spec, self.field.ty.as_ref());
        let index = self.parent.graph.indices[&node];
        self.parent
            .graph
            .circular_refs
            .contains(&(self.parent.index, index))
    }
}
