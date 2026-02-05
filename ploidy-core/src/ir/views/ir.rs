use either::Either;
use petgraph::graph::NodeIndex;

use crate::ir::graph::{IrGraph, IrGraphNode};

use super::{
    View, container::ContainerView, inline::InlineIrTypeView, primitive::IrPrimitiveView,
    schema::SchemaIrTypeView,
};

/// Generates a `match` expression that wraps each arm in nested [`Either`] variants.
/// All arms except the last are wrapped in `depth` [`Either::Right`]s around an
/// [`Either::Left`]. The last arm is wrapped in `depth` [`Either::Right`]s around
/// the last expression.
macro_rules! either {
    (match $val:tt { $($body:tt)+ }) => {
        either!(@collect $val; []; []; $($body)+)
    };
    // All arms except the last.
    (@collect $val:expr; [$($arms:tt)*]; [$($depth:tt)*]; $pat:pat => $expr:expr, $($rest:tt)+) => {
        either!(@collect $val;
            [$($arms)* $pat => either!(@left [$($depth)*] $expr),];
            [$($depth)* R];
            $($rest)+)
    };
    // Last arm.
    (@collect $val:expr; [$($arms:tt)*]; [$($depth:tt)*]; $pat:pat => $expr:expr $(,)?) => {
        match $val {
            $($arms)*
            $pat => either!(@right [$($depth)*] $expr),
        }
    };
    // Wrap with `depth` `Right`s, then a `Left`.
    (@left [] $expr:expr) => { Either::Left($expr) };
    (@left [R $($rest:tt)*] $expr:expr) => {
        Either::Right(either!(@left [$($rest)*] $expr))
    };
    // Wrap with `depth` `Right`s only, for the last arm.
    (@right [] $expr:expr) => { $expr };
    (@right [R $($rest:tt)*] $expr:expr) => {
        Either::Right(either!(@right [$($rest)*] $expr))
    };
}

/// A graph-aware view of an [`IrType`][crate::ir::IrType].
#[derive(Debug)]
pub enum IrTypeView<'a> {
    Any,
    Primitive(IrPrimitiveView<'a>),
    Schema(SchemaIrTypeView<'a>),
    Inline(InlineIrTypeView<'a>),
}

impl<'a> IrTypeView<'a> {
    pub(in crate::ir) fn new(graph: &'a IrGraph<'a>, index: NodeIndex<usize>) -> Self {
        match &graph.g[index] {
            IrGraphNode::Any => Self::Any,
            &IrGraphNode::Primitive(ty) => Self::Primitive(IrPrimitiveView::new(graph, index, ty)),
            IrGraphNode::Schema(ty) => Self::Schema(SchemaIrTypeView::new(graph, index, ty)),
            IrGraphNode::Inline(ty) => Self::Inline(InlineIrTypeView::new(graph, index, ty)),
        }
    }

    /// If this is a view of a named schema type, returns the view for that type.
    #[inline]
    pub fn as_schema(self) -> Option<SchemaIrTypeView<'a>> {
        match self {
            Self::Schema(view) => Some(view),
            _ => None,
        }
    }

    /// If this is a view of a named or inline container type,
    /// returns the container view.
    #[inline]
    pub fn as_container(&self) -> Option<&ContainerView<'a>> {
        match self {
            Self::Schema(SchemaIrTypeView::Container(_, view)) => Some(view),
            Self::Inline(InlineIrTypeView::Container(_, view)) => Some(view),
            _ => None,
        }
    }

    /// Returns an iterator over all the types that this type transitively depends on.
    #[inline]
    pub fn dependencies(&self) -> impl Iterator<Item = IrTypeView<'a>> + use<'a> {
        either!(match self {
            Self::Any => std::iter::empty(),
            Self::Primitive(v) => v.dependencies(),
            Self::Schema(v) => v.dependencies(),
            Self::Inline(v) => v.dependencies(),
        })
    }
}
