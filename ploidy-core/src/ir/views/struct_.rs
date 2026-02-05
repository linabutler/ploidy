use petgraph::{
    Direction,
    graph::NodeIndex,
    visit::{Bfs, DfsPostOrder, EdgeFiltered},
};
use rustc_hash::FxHashSet;

use crate::ir::{
    graph::{EdgeKind, IrGraph, IrGraphNode},
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

    /// Returns an iterator over all fields, including fields inherited
    /// from `allOf` schemas. Fields are returned in declaration order:
    /// ancestor fields first, in the order of their parents in `allOf`;
    /// then this struct's own fields.
    pub fn fields(&self) -> impl Iterator<Item = IrStructFieldView<'_, 'a>> {
        // Walk inheritance edges in post-order so that the most distant
        // ancestors are yielded first. `DfsPostOrder` also tracks visited
        // nodes internally, which handles circular `allOf` references.
        let inherits =
            EdgeFiltered::from_fn(&self.graph.g, |e| matches!(e.weight(), EdgeKind::Inherits));
        let mut dfs = DfsPostOrder::new(&inherits, self.index);
        let ancestors = std::iter::from_fn(move || dfs.next(&inherits))
            .filter(move |&index| index != self.index)
            .filter_map(|index| match &self.graph.g[index] {
                IrGraphNode::Schema(SchemaIrType::Struct(_, s))
                | IrGraphNode::Inline(InlineIrType::Struct(_, s)) => Some(s),
                _ => None,
            });

        // Track our own field names, so that we can skip yielding
        // overridden inherited fields.
        let mut seen: FxHashSet<_> = self.ty.fields.iter().map(|field| field.name).collect();

        itertools::chain!(
            // Inherited fields first, in declaration order.
            ancestors
                .flat_map(|ancestor| &ancestor.fields)
                .filter(move |field| seen.insert(field.name))
                .map(|field| IrStructFieldView {
                    parent: self,
                    field,
                    inherited: true,
                }),
            // Own fields.
            self.own_fields(),
        )
    }

    /// Returns an iterator over fields declared directly on this struct,
    /// excluding inherited fields.
    #[inline]
    pub fn own_fields(&self) -> impl Iterator<Item = IrStructFieldView<'_, 'a>> {
        self.ty.fields.iter().map(move |field| IrStructFieldView {
            parent: self,
            field,
            inherited: false,
        })
    }

    /// Returns an iterator over immediate parent types from `allOf`,
    /// including named and inline schemas.
    #[inline]
    pub fn parents(&self) -> impl Iterator<Item = IrTypeView<'a>> + '_ {
        self.ty.parents.iter().map(move |parent| {
            let node = IrGraphNode::from_ref(self.graph.spec, parent.as_ref());
            IrTypeView::new(self.graph, self.graph.indices[&node])
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
    inherited: bool,
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
        let IrStructFieldName::Name(name) = self.field.name else {
            return false;
        };

        // Check if our parent struct, or any of its ancestors,
        // declare this field as a discriminator.
        let inherits = EdgeFiltered::from_fn(&self.parent.graph.g, |e| {
            matches!(*e.weight(), EdgeKind::Inherits)
        });
        let mut bfs = Bfs::new(&inherits, self.parent.index);
        let is_ancestor_discriminator = std::iter::from_fn(|| bfs.next(&inherits))
            .filter_map(|index| match self.parent.graph.g[index] {
                IrGraphNode::Schema(SchemaIrType::Struct(_, s))
                | IrGraphNode::Inline(InlineIrType::Struct(_, s)) => Some(s),
                _ => None,
            })
            .any(|ancestor| ancestor.discriminator == Some(name));
        if is_ancestor_discriminator {
            return true;
        }

        // Check whether any tagged unions that include our parent struct
        // declare this field as their discriminators.
        self.parent
            .graph
            .g
            .neighbors_directed(self.parent.index, Direction::Incoming)
            .filter_map(|index| match self.parent.graph.g[index] {
                IrGraphNode::Schema(SchemaIrType::Tagged(_, tagged))
                | IrGraphNode::Inline(InlineIrType::Tagged(_, tagged)) => Some(tagged),
                _ => None,
            })
            .any(|neighbor| neighbor.tag == name)
    }

    #[inline]
    pub fn flattened(&self) -> bool {
        self.field.flattened
    }

    /// Returns `true` if this field was inherited from a parent via `allOf`.
    #[inline]
    pub fn inherited(&self) -> bool {
        self.inherited
    }

    /// Returns `true` if this field needs indirection to break a cycle.
    ///
    /// A field needs indirection if its target type is in the same strongly
    /// connected component as the struct that contains it.
    #[inline]
    pub fn needs_indirection(&self) -> bool {
        let graph = self.parent.graph;
        let node = IrGraphNode::from_ref(graph.spec, self.field.ty.as_ref());
        let target = graph.indices[&node];
        graph.metadata.scc_indices[self.parent.index.index()]
            == graph.metadata.scc_indices[target.index()]
    }
}
