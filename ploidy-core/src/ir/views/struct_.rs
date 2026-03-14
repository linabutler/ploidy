use petgraph::{
    Direction,
    graph::NodeIndex,
    visit::{DfsPostOrder, EdgeFiltered},
};
use rustc_hash::FxHashSet;

use crate::ir::{
    GraphInlineType,
    graph::{CookedGraph, EdgeKind},
    types::{GraphSchemaType, GraphStruct, GraphStructField, GraphType, StructFieldName},
};

use super::{ViewNode, ir::TypeView};

/// A graph-aware view of a [struct type][GraphStruct].
#[derive(Debug)]
pub struct StructView<'a> {
    cooked: &'a CookedGraph<'a>,
    index: NodeIndex<usize>,
    ty: GraphStruct<'a>,
}

impl<'a> StructView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: GraphStruct<'a>,
    ) -> Self {
        Self { cooked, index, ty }
    }

    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.ty.description
    }

    /// Returns an iterator over all fields, including fields inherited
    /// from `allOf` schemas. Fields are returned in declaration order:
    /// ancestor fields first, in the order of their parents in `allOf`;
    /// then this struct's own fields.
    pub fn fields(&self) -> impl Iterator<Item = StructFieldView<'_, 'a>> {
        // Walk inheritance edges in post-order so that the most distant
        // ancestors are yielded first. `DfsPostOrder` also tracks visited
        // nodes internally, which handles circular `allOf` references.
        let inherits = EdgeFiltered::from_fn(&self.cooked.graph, |e| {
            matches!(e.weight(), EdgeKind::Inherits)
        });
        let mut dfs = DfsPostOrder::new(&inherits, self.index);
        let ancestors = std::iter::from_fn(move || dfs.next(&inherits))
            .filter(move |&index| index != self.index)
            .filter_map(|index| match self.cooked.graph[index] {
                GraphType::Schema(GraphSchemaType::Struct(_, s))
                | GraphType::Inline(GraphInlineType::Struct(_, s)) => Some(s),
                _ => None,
            });

        // Track our own field names, so that we can skip yielding
        // overridden inherited fields.
        let mut seen: FxHashSet<_> = self.ty.fields.iter().map(|field| field.name).collect();

        itertools::chain!(
            // Inherited fields first, in declaration order.
            ancestors
                .flat_map(|ancestor| ancestor.fields)
                .filter(move |field| seen.insert(field.name))
                .map(|field| StructFieldView {
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
    pub fn own_fields(&self) -> impl Iterator<Item = StructFieldView<'_, 'a>> {
        self.ty.fields.iter().map(move |field| StructFieldView {
            parent: self,
            field,
            inherited: false,
        })
    }

    /// Returns an iterator over immediate parent types from `allOf`,
    /// including named and inline schemas.
    #[inline]
    pub fn parents(&self) -> impl Iterator<Item = TypeView<'a>> {
        self.ty
            .parents
            .iter()
            .map(move |&parent| TypeView::new(self.cooked, parent))
    }
}

impl<'a> ViewNode<'a> for StructView<'a> {
    #[inline]
    fn cooked(&self) -> &'a CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}

/// A graph-aware view of a [struct field][GraphStructField].
#[derive(Debug)]
pub struct StructFieldView<'view, 'a> {
    parent: &'view StructView<'a>,
    field: &'a GraphStructField<'a>,
    inherited: bool,
}

impl<'view, 'a> StructFieldView<'view, 'a> {
    #[inline]
    pub fn name(&self) -> StructFieldName<'a> {
        self.field.name
    }

    /// Returns a view of the inner type that this type wraps.
    #[inline]
    pub fn ty(&self) -> TypeView<'a> {
        TypeView::new(self.parent.cooked, self.field.ty)
    }

    #[inline]
    pub fn required(&self) -> bool {
        self.field.required
    }

    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.field.description
    }

    /// Returns `true` if this field is a tag.
    ///
    /// A field is a tag if it matches the tag of a tagged union
    /// that references this struct as one of its variants.
    #[inline]
    pub fn tag(&self) -> bool {
        let StructFieldName::Name(name) = self.field.name else {
            return false;
        };
        self.parent
            .cooked
            .graph
            .neighbors_directed(self.parent.index, Direction::Incoming)
            .filter_map(|index| match self.parent.cooked.graph[index] {
                GraphType::Schema(GraphSchemaType::Tagged(_, tagged))
                | GraphType::Inline(GraphInlineType::Tagged(_, tagged)) => Some(tagged),
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
        let graph = self.parent.cooked;
        graph.metadata.scc_indices[self.parent.index.index()]
            == graph.metadata.scc_indices[self.field.ty.index()]
    }
}
