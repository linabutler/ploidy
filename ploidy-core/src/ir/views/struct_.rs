//! Struct types: object schemas and `allOf` composition.
//!
//! In OpenAPI, a `type: object` schema with `properties` describes a record
//! with named fields. A schema can also inherit fields from other schemas via
//! `allOf`, which is how OpenAPI models composition and inheritance:
//!
//! ```yaml
//! components:
//!   schemas:
//!     Address:
//!       type: object
//!       required: [city]
//!       properties:
//!         city:
//!           type: string
//!         zip:
//!           type: string
//!     Office:
//!       allOf:
//!         - $ref: '#/components/schemas/Address'
//!         - type: object
//!           required: [floor]
//!           properties:
//!             floor:
//!               type: integer
//! ```
//!
//! Ploidy represents both cases as a [`StructView`]. A struct has
//! its own fields plus fields inherited from its `allOf` parents.
//! Each field carries properties that guide codegen:
//!
//! * **Required vs. optional.** A field listed in `required` is
//!   non-optional; others are wrapped in [`ContainerView::Optional`].
//! * **Flattened.** Fields originating from `anyOf` parents are
//!   flattened into the struct as optional fields.
//! * **Tag.** A field is a tag if its name matches the discriminator of a
//!   [tagged union] that references this struct as a variant.
//! * **Indirection.** A field needs indirection (e.g., [`Box<T>`] in Rust)
//!   when it and any of its parent structs form a cycle in the type graph.
//! * **Inherited.** A field that comes from an `allOf` parent rather than
//!   this struct's own `properties`.
//!
//! [`ContainerView::Optional`]: super::container::ContainerView::Optional
//! [tagged union]: super::tagged::TaggedView

use std::collections::VecDeque;

use fixedbitset::FixedBitSet;
use itertools::Itertools;
use petgraph::{
    Direction,
    graph::NodeIndex,
    visit::{EdgeFiltered, EdgeRef, IntoNeighbors},
};
use rustc_hash::FxHashSet;

use crate::ir::{
    graph::{CookedGraph, GraphEdge},
    types::{FieldMeta, GraphInlineType, GraphSchemaType, GraphStruct, GraphType, StructFieldName},
};

use super::{ViewNode, container::ContainerView, ir::TypeView};

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

    /// Returns the description, if present in the schema.
    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.ty.description
    }

    /// Returns an iterator over all fields, including fields inherited
    /// from `allOf` schemas. Fields are returned in declaration order:
    /// ancestor fields first, in the order of their parents in `allOf`;
    /// then this struct's own fields.
    #[inline]
    pub fn fields(&self) -> impl Iterator<Item = StructFieldView<'_, 'a>> {
        let all = self
            .inherited_fields() // Not a `DoubleEndedIterator`; can't reverse directly.
            .chain(self.own_fields())
            .collect_vec();

        // Deduplicate fields right-to-left, so that later (closer) fields
        // win over earlier (distant) ones; then reverse again to
        // restore declaration order.
        let mut seen = FxHashSet::default();
        let deduped = all
            .into_iter()
            .rev()
            .filter(|f| seen.insert(f.meta.name))
            .collect_vec();
        deduped.into_iter().rev()
    }

    /// Returns an iterator over all fields inherited from
    /// this struct's ancestors.
    fn inherited_fields(&self) -> impl Iterator<Item = StructFieldView<'_, 'a>> {
        // Walk inheritance edges in post-order, so that the most distant
        // ancestors are yielded first. The LIFO stack explores ancestors in
        // reverse declaration order (right-to-left); collecting them into a
        // `VecDeque` lets us iterate over them in declaration order.
        let inherits = EdgeFiltered::from_fn(&self.cooked.graph, |e| {
            matches!(e.weight(), GraphEdge::Inherits { .. })
        });
        let mut stack = vec![self.index];
        let mut visited = FixedBitSet::with_capacity(self.cooked.graph.node_count());
        let mut ancestors = VecDeque::new();
        while let Some(node) = stack.pop() {
            if visited.put(node.index()) {
                continue;
            }
            if node != self.index {
                ancestors.push_front(node);
            }
            for child in inherits.neighbors(node) {
                stack.push(child);
            }
        }

        ancestors
            .into_iter()
            .flat_map(|index| self.cooked.fields(index))
            .map(|info| StructFieldView::new(self, info.meta, info.target, true))
    }

    /// Returns an iterator over fields declared directly on this struct,
    /// excluding inherited fields.
    #[inline]
    pub fn own_fields(&self) -> impl Iterator<Item = StructFieldView<'_, 'a>> {
        self.cooked
            .fields(self.index)
            .map(move |info| StructFieldView::new(self, info.meta, info.target, false))
    }

    /// Returns an iterator over immediate parent types from `allOf`,
    /// including named and inline schemas.
    #[inline]
    pub fn parents(&self) -> impl Iterator<Item = TypeView<'a>> {
        self.cooked
            .inherits(self.index)
            .map(move |info| TypeView::new(self.cooked, info.target))
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

/// A graph-aware view of a struct field.
pub type StructFieldView<'view, 'a> = FieldView<'view, 'a, StructView<'a>>;

/// A graph-aware view of a struct or union field.
#[derive(Debug)]
pub struct FieldView<'view, 'a, P> {
    parent: &'view P,
    meta: FieldMeta<'a>,
    ty: NodeIndex<usize>,
    inherited: bool,
}

#[allow(private_bounds, reason = "`ViewNode` is sealed")]
impl<'view, 'a, P: ViewNode<'a>> FieldView<'view, 'a, P> {
    #[inline]
    pub(in crate::ir) fn new(
        parent: &'view P,
        meta: FieldMeta<'a>,
        ty: NodeIndex<usize>,
        inherited: bool,
    ) -> Self {
        Self {
            parent,
            meta,
            ty,
            inherited,
        }
    }

    /// Returns the field name.
    #[inline]
    pub fn name(&self) -> StructFieldName<'a> {
        self.meta.name
    }

    /// Returns a view of the inner type that this type wraps.
    #[inline]
    pub fn ty(&self) -> TypeView<'a> {
        TypeView::new(self.parent.cooked(), self.ty)
    }

    /// Returns whether this field is required or optional.
    #[inline]
    pub fn required(&self) -> Required {
        if self.meta.required {
            let nullable = matches!(self.ty().as_container(), Some(ContainerView::Optional(_)));
            Required::Required { nullable }
        } else {
            Required::Optional
        }
    }

    /// Returns the description, if present in the schema.
    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.meta.description
    }

    /// Returns `true` if this field is flattened from an
    /// `anyOf` parent.
    #[inline]
    pub fn flattened(&self) -> bool {
        self.meta.flattened
    }
}

/// Whether a field is required or optional.
///
/// Required fields are always present, but may be nullable; optional fields
/// may be absent entirely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Required {
    /// The field must be present in the payload.
    Required {
        /// Whether the field can be `null` if present.
        nullable: bool,
    },
    /// The field may be absent from the payload.
    Optional,
}

impl<'view, 'a> FieldView<'view, 'a, StructView<'a>> {
    /// Returns `true` if this field was inherited from a parent via `allOf`.
    #[inline]
    pub fn inherited(&self) -> bool {
        self.inherited
    }

    /// Returns `true` if this field is a tag.
    ///
    /// A field is a tag only if this struct inherits from or is a variant of
    /// a tagged union, and the field name matches that union's tag.
    #[inline]
    pub fn tag(&self) -> bool {
        let StructFieldName::Name(name) = self.meta.name else {
            return false;
        };
        let cooked = self.parent.cooked();
        cooked
            .graph
            .edges_directed(self.parent.index(), Direction::Incoming)
            .filter(|e| {
                matches!(
                    e.weight(),
                    GraphEdge::Variant(_) | GraphEdge::Inherits { .. }
                )
            })
            .filter_map(|e| match cooked.graph[e.source()] {
                GraphType::Schema(GraphSchemaType::Tagged(_, tagged))
                | GraphType::Inline(GraphInlineType::Tagged(_, tagged)) => Some(tagged),
                _ => None,
            })
            .any(|neighbor| neighbor.tag == name)
    }

    /// Returns `true` if this field needs `Box<T>` to break a cycle.
    ///
    /// A field needs boxing if its target type is in the same strongly
    /// connected component as the type that contains it, excluding
    /// edges through heap-allocating containers (arrays and maps).
    #[inline]
    pub fn needs_box(&self) -> bool {
        let graph = self.parent.cooked();
        graph.metadata.box_sccs[self.parent.index().index()]
            == graph.metadata.box_sccs[self.ty.index()]
    }
}
