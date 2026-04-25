//! Tagged unions: `oneOf` with a discriminator.
//!
//! In OpenAPI, a `oneOf` schema with a `discriminator` defines
//! a tagged union, where the discriminator property is a tag
//! that selects the concrete type:
//!
//! ```yaml
//! components:
//!   schemas:
//!     Shape:
//!       oneOf:
//!         - $ref: '#/components/schemas/Circle'
//!         - $ref: '#/components/schemas/Square'
//!       discriminator:
//!         propertyName: kind
//! ```
//!
//! The `kind` field in the JSON payload determines the variant.
//! Ploidy represents this as a [`TaggedView`] with a [tag] and [variants].
//! Each [`TaggedVariantView`] carries:
//!
//! * A [name]: the discriminator value that selects this variant
//!   (e.g., `"Circle"`).
//! * Optional [aliases]: additional discriminator values that
//!   also select this variant, defined via the discriminator's
//!   `mapping` in the spec.
//! * A [type]: the schema that the variant deserializes into,
//!   accessed as a [`TypeView`].
//!
//! [tag]: TaggedView::tag
//! [variants]: TaggedView::variants
//! [name]: TaggedVariantView::name
//! [aliases]: TaggedVariantView::aliases
//! [type]: TaggedVariantView::ty

use petgraph::graph::NodeIndex;

use crate::ir::{
    graph::CookedGraph,
    types::{GraphTagged, TaggedVariantMeta, VariantMeta},
};

use super::{ViewNode, ir::TypeView, struct_::FieldView};

/// A graph-aware view of a [tagged union type][GraphTagged].
#[derive(Debug)]
pub struct TaggedView<'graph, 'a> {
    cooked: &'graph CookedGraph<'a>,
    index: NodeIndex<usize>,
    ty: GraphTagged<'a>,
}

impl<'graph, 'a> TaggedView<'graph, 'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'graph CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: GraphTagged<'a>,
    ) -> Self {
        Self { cooked, index, ty }
    }

    /// Returns the description, if present in the schema.
    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.ty.description
    }

    /// Returns the discriminator property name.
    #[inline]
    pub fn tag(&self) -> &'a str {
        self.ty.tag
    }

    /// Returns the common fields declared alongside `oneOf`,
    /// shared across all variants.
    #[inline]
    pub fn fields(&self) -> impl Iterator<Item = TaggedFieldView<'_, 'graph, 'a>> {
        self.cooked
            .fields(self.index)
            .map(move |info| TaggedFieldView::new(self, info.meta, info.target, false))
    }

    /// Returns an iterator over this tagged union's variants.
    #[inline]
    pub fn variants(&self) -> impl Iterator<Item = TaggedVariantView<'graph, 'a>> {
        self.cooked
            .variants(self.index)
            .filter_map(move |info| match info.meta {
                VariantMeta::Tagged(meta) => {
                    Some(TaggedVariantView::new(self.cooked, info.target, meta))
                }
                _ => None,
            })
    }
}

impl<'graph, 'a> ViewNode<'graph, 'a> for TaggedView<'graph, 'a> {
    #[inline]
    fn cooked(&self) -> &'graph CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}

/// A graph-aware view of a common tagged union field.
pub type TaggedFieldView<'view, 'graph, 'a> = FieldView<'view, 'a, TaggedView<'graph, 'a>>;

/// A graph-aware view of a tagged union variant.
#[derive(Debug)]
pub struct TaggedVariantView<'graph, 'a> {
    cooked: &'graph CookedGraph<'a>,
    index: NodeIndex<usize>,
    meta: TaggedVariantMeta<'a>,
}

impl<'graph, 'a> TaggedVariantView<'graph, 'a> {
    #[inline]
    fn new(
        cooked: &'graph CookedGraph<'a>,
        index: NodeIndex<usize>,
        meta: TaggedVariantMeta<'a>,
    ) -> Self {
        Self {
            cooked,
            index,
            meta,
        }
    }

    /// Returns the discriminator value that selects this
    /// variant.
    #[inline]
    pub fn name(&self) -> &'a str {
        self.meta.name
    }

    /// Returns additional discriminator values that also
    /// select this variant.
    #[inline]
    pub fn aliases(&self) -> &'a [&'a str] {
        self.meta.aliases
    }

    /// Returns a view of this variant's type.
    #[inline]
    pub fn ty(&self) -> TypeView<'graph, 'a> {
        TypeView::new(self.cooked, self.index)
    }
}

impl<'graph, 'a> ViewNode<'graph, 'a> for TaggedVariantView<'graph, 'a> {
    #[inline]
    fn cooked(&self) -> &'graph CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}
