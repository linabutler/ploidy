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

use super::{TypeViewId, ViewNode};

/// A graph-aware view of an [enum type][Enum].
#[derive(Debug)]
pub struct EnumView<'graph, 'a> {
    cooked: &'graph CookedGraph<'a>,
    index: NodeIndex<usize>,
    ty: Enum<'a>,
}

impl<'graph, 'a> EnumView<'graph, 'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'graph CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: Enum<'a>,
    ) -> Self {
        Self { cooked, index, ty }
    }

    /// Returns an opaque identity for this schema type.
    #[inline]
    pub fn id(&self) -> TypeViewId {
        TypeViewId(self.index())
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

impl<'graph, 'a> ViewNode<'graph, 'a> for EnumView<'graph, 'a> {
    #[inline]
    fn cooked(&self) -> &'graph CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}
