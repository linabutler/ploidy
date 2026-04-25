//! Untagged unions: `type` arrays and `oneOf` without a discriminator.
//!
//! In OpenAPI, a `oneOf` schema without a `discriminator` defines
//! an untagged union: there's no explicit tag, so deserialization tries
//! each variant in order until one matches. In OpenAPI 3.1+, an array of
//! `type`s also expresses an untagged union:
//!
//! ```yaml
//! components:
//!   schemas:
//!     StringOrInt:
//!       oneOf:
//!         - type: string
//!         - type: integer
//!     DateOrUnix:
//!       type: [string, integer]
//!       format: date-time
//! ```
//!
//! Ploidy represents this as an [`UntaggedView`] with a list of variants.
//! Each variant is either a typed value or `null`, modeled as
//! [`Option<SomeUntaggedVariant>`]. The typed case pairs an
//! [`UntaggedVariantNameHint`] with a [`TypeView`]; the hint helps codegen
//! produce readable variant names when the schema has no explicit name.
//!
//! [`Option<SomeUntaggedVariant>`]: SomeUntaggedVariant

use petgraph::graph::NodeIndex;

use crate::ir::{
    UntaggedVariantMeta, UntaggedVariantNameHint,
    graph::CookedGraph,
    types::{GraphUntagged, VariantMeta},
};

use super::{ViewNode, ir::TypeView, struct_::FieldView};

/// A graph-aware view of an [untagged union type][GraphUntagged].
#[derive(Debug)]
pub struct UntaggedView<'graph, 'a> {
    cooked: &'graph CookedGraph<'a>,
    index: NodeIndex<usize>,
    ty: GraphUntagged<'a>,
}

impl<'graph, 'a> UntaggedView<'graph, 'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'graph CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: GraphUntagged<'a>,
    ) -> Self {
        Self { cooked, index, ty }
    }

    /// Returns the description, if present in the schema.
    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.ty.description
    }

    /// Returns the common fields declared alongside `oneOf`,
    /// shared across all variants.
    #[inline]
    pub fn fields(&self) -> impl Iterator<Item = UntaggedFieldView<'_, 'graph, 'a>> {
        self.cooked
            .fields(self.index)
            .map(move |info| UntaggedFieldView::new(self, info.meta, info.target, false))
    }

    /// Returns an iterator over this untagged union's variants.
    pub fn variants(&self) -> impl Iterator<Item = UntaggedVariantView<'_, 'graph, 'a>> {
        self.cooked
            .variants(self.index)
            .map(move |info| UntaggedVariantView {
                parent: self,
                meta: info.meta,
                index: info.target,
            })
    }
}

impl<'graph, 'a> ViewNode<'graph, 'a> for UntaggedView<'graph, 'a> {
    #[inline]
    fn cooked(&self) -> &'graph CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}

/// A graph-aware view of a common untagged union field.
pub type UntaggedFieldView<'view, 'graph, 'a> = FieldView<'view, 'a, UntaggedView<'graph, 'a>>;

/// A graph-aware view of an untagged union variant.
#[derive(Debug)]
pub struct UntaggedVariantView<'view, 'graph, 'a> {
    parent: &'view UntaggedView<'graph, 'a>,
    meta: VariantMeta<'a>,
    /// The node index of this variant's type (from the `Variant` edge).
    index: NodeIndex<usize>,
}

impl<'view, 'graph, 'a> UntaggedVariantView<'view, 'graph, 'a> {
    /// Returns a view of this variant's type, if it's not a unit
    /// variant.
    #[inline]
    pub fn ty(&self) -> Option<SomeUntaggedVariant<'graph, 'a>> {
        match self.meta {
            VariantMeta::Untagged(UntaggedVariantMeta::Type { hint }) => {
                Some(SomeUntaggedVariant {
                    hint,
                    view: TypeView::new(self.parent.cooked, self.index),
                })
            }
            _ => None,
        }
    }
}

/// A non-unit variant of an untagged union, pairing a name hint
/// with the variant's type.
#[derive(Debug)]
pub struct SomeUntaggedVariant<'graph, 'a> {
    /// A hint for generating a readable variant name.
    pub hint: UntaggedVariantNameHint,
    /// A view of this variant's type.
    pub view: TypeView<'graph, 'a>,
}
