use petgraph::{Direction, graph::NodeIndex};

use crate::ir::{
    graph::{IrGraph, IrGraphNode},
    types::{InlineIrType, IrStruct, IrStructField, IrStructFieldName, SchemaIrType},
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

    /// Returns `true` if this field is a discriminator property.
    ///
    /// A field is a discriminator if it's explicitly named as the `discriminator`
    /// in its parent struct's definition, if it's named as an ancestor struct's
    /// discriminator, or if its parent struct is a variant of a tagged union
    /// whose `tag` matches this field's name.
    #[inline]
    pub fn discriminator(&self) -> bool {
        if self.field.discriminator {
            return true;
        }

        // Check whether an incoming tagged union uses this field
        // as _its_ discriminator.
        let IrStructFieldName::Name(name) = self.field.name else {
            return false;
        };
        self.parent
            .graph
            .g
            .neighbors_directed(self.parent.index, Direction::Incoming)
            .any(|neighbor| match self.parent.graph.g[neighbor] {
                IrGraphNode::Schema(SchemaIrType::Tagged(_, tagged)) => tagged.tag == name,
                IrGraphNode::Inline(InlineIrType::Tagged(_, tagged)) => tagged.tag == name,
                _ => false,
            })
    }

    #[inline]
    pub fn flattened(&self) -> bool {
        self.field.flattened
    }

    /// Returns `true` if this field needs indirection to break a cycle.
    #[inline]
    pub fn needs_indirection(&self) -> bool {
        let node = IrGraphNode::from_ref(self.parent.graph.spec, self.field.ty.as_ref());
        let index = self.parent.graph.indices[&node];
        self.parent
            .graph
            .circular_refs
            .contains(&(self.parent.index, index))
    }
}
