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
    types::{GraphStructField, GraphTagged, GraphTaggedVariant},
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
    pub fn fields(&self) -> &'a [GraphStructField<'a>] {
        self.ty.fields
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

    /// Returns the discriminator value that selects this
    /// variant.
    #[inline]
    pub fn name(&self) -> &'a str {
        self.variant.name
    }

    /// Returns additional discriminator values that also
    /// select this variant.
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
