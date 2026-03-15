//! Enum types: schemas with a fixed set of literal values.
//!
//! In OpenAPI, a schema with `enum` restricts a value to
//! one of a fixed set of literals:
//!
//! ```yaml
//! components:
//!   schemas:
//!     Status:
//!       type: string
//!       enum: [active, paused, canceled]
//! ```
//!
//! Ploidy represents this as an [`EnumView`]. Each variant carries a
//! literal value: string, number, or boolean. See [`EnumVariant`]
//! for the full set.

use petgraph::graph::NodeIndex;

use crate::ir::{
    graph::CookedGraph,
    types::{Enum, EnumVariant},
};

use super::ViewNode;

/// A graph-aware view of an [enum type][Enum].
#[derive(Debug)]
pub struct EnumView<'a> {
    cooked: &'a CookedGraph<'a>,
    index: NodeIndex<usize>,
    ty: Enum<'a>,
}

impl<'a> EnumView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: Enum<'a>,
    ) -> Self {
        Self { cooked, index, ty }
    }

    /// Returns the description, if present in the schema.
    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.ty.description
    }

    /// Returns the enum's variants.
    #[inline]
    pub fn variants(&self) -> &'a [EnumVariant<'a>] {
        self.ty.variants
    }
}

impl<'a> ViewNode<'a> for EnumView<'a> {
    #[inline]
    fn cooked(&self) -> &'a CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}
