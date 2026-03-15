//! Container types: arrays, maps, and optionals.
//!
//! In OpenAPI, `type: array` with `items` defines a list,
//! and `type: object` without `properties` and with
//! `additionalProperties` defines a map. Schemas with
//! `nullable: true` (OpenAPI 3.0), `type: [T, "null"]`
//! (OpenAPI 3.1), or `oneOf` with a `null` branch all
//! become optionals:
//!
//! ```yaml
//! components:
//!   schemas:
//!     Tags:
//!       type: array
//!       items:
//!         type: string
//!     Metadata:
//!       type: object
//!       additionalProperties:
//!         type: string
//!     NullableName:
//!       type: [string, null]
//! ```
//!
//! Ploidy represents all three as [`ContainerView`] variants—
//! [`Array`][array], [`Map`][map], and [`Optional`][opt]—
//! each wrapping an [`InnerView`] that provides access to
//! the contained type.
//!
//! [array]: ContainerView::Array
//! [map]: ContainerView::Map
//! [opt]: ContainerView::Optional

use petgraph::graph::NodeIndex;

use crate::ir::{
    graph::CookedGraph,
    types::{GraphContainer, GraphInlineType, GraphSchemaType, GraphType},
};

use super::{TypeView, ViewNode};

/// A graph-aware view of a [container type][GraphContainer].
#[derive(Debug)]
pub enum ContainerView<'a> {
    Array(InnerView<'a>),
    Map(InnerView<'a>),
    Optional(InnerView<'a>),
}

impl<'a> ContainerView<'a> {
    /// Returns a type view of this container type.
    #[inline]
    pub fn ty(&self) -> TypeView<'a> {
        TypeView::new(self.cooked(), self.index())
    }
}

impl<'a> ViewNode<'a> for ContainerView<'a> {
    #[inline]
    fn cooked(&self) -> &'a CookedGraph<'a> {
        let (Self::Array(c) | Self::Map(c) | Self::Optional(c)) = self;
        c.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        let (Self::Array(c) | Self::Map(c) | Self::Optional(c)) = self;
        c.container
    }
}

/// A graph-aware view of the inner type of a [container][ContainerView].
#[derive(Debug)]
pub struct InnerView<'a> {
    cooked: &'a CookedGraph<'a>,
    container: NodeIndex<usize>,
    inner: NodeIndex<usize>,
}

impl<'a> InnerView<'a> {
    /// Returns a view of the contained type.
    #[inline]
    pub fn ty(&self) -> TypeView<'a> {
        TypeView::new(self.cooked, self.inner)
    }

    /// Returns a human-readable description of the contained type, if present.
    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        match self.cooked.graph[self.container] {
            GraphType::Schema(GraphSchemaType::Container(_, container))
            | GraphType::Inline(GraphInlineType::Container(_, container)) => {
                container.inner().description
            }
            _ => None,
        }
    }
}

impl<'a> ContainerView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        container: GraphContainer<'a>,
    ) -> Self {
        let inner = InnerView {
            cooked,
            container: index,
            inner: container.inner().ty,
        };
        match container {
            GraphContainer::Array(_) => Self::Array(inner),
            GraphContainer::Map(_) => Self::Map(inner),
            GraphContainer::Optional(_) => Self::Optional(inner),
        }
    }

    /// Returns an iterator over all the types that this container depends on.
    #[inline]
    pub fn dependencies(&self) -> impl Iterator<Item = TypeView<'a>> + use<'a> {
        let (Self::Array(view) | Self::Map(view) | Self::Optional(view)) = self;
        let inner = TypeView::new(view.cooked, view.inner);
        let dependencies = inner.dependencies();
        std::iter::once(inner).chain(dependencies)
    }
}
