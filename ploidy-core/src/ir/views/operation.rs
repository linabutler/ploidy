use std::marker::PhantomData;

use by_address::ByAddress;
use enum_map::enum_map;
use fixedbitset::FixedBitSet;
use petgraph::{
    Direction,
    graph::NodeIndex,
    visit::{Bfs, EdgeFiltered, EdgeRef, VisitMap, Visitable},
};

use crate::{
    ir::{
        graph::{CookedGraph, EdgeKind, GraphNode, Traversal, Traverse},
        types::{
            CookedOperation, CookedParameter, CookedParameterInfo, CookedRequest, CookedResponse,
            ParameterStyle,
        },
    },
    parse::{Method, path::PathSegment},
};

use super::{Reach, View, inline::InlineTypeView, ir::TypeView};

/// A graph-aware view of an [`Operation`][crate::ir::CookedOperation].
#[derive(Debug)]
pub struct OperationView<'a> {
    cooked: &'a CookedGraph<'a>,
    op: &'a CookedOperation<'a>,
}

impl<'a> OperationView<'a> {
    #[inline]
    pub(in crate::ir) fn new(cooked: &'a CookedGraph<'a>, op: &'a CookedOperation<'a>) -> Self {
        Self { cooked, op }
    }

    #[inline]
    pub fn id(&self) -> &'a str {
        self.op.id
    }

    #[inline]
    pub fn method(&self) -> Method {
        self.op.method
    }

    #[inline]
    pub fn path(&self) -> OperationViewPath<'_, 'a> {
        OperationViewPath(self)
    }

    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.op.description
    }

    /// Returns an iterator over this operation's query parameters.
    #[inline]
    pub fn query(&self) -> impl Iterator<Item = ParameterView<'a, QueryParameter>> + '_ {
        self.op.params.iter().filter_map(move |param| match param {
            CookedParameter::Query(info) => Some(ParameterView::new(self.cooked, info)),
            _ => None,
        })
    }

    /// Returns a view of the request body, if present.
    #[inline]
    pub fn request(&self) -> Option<RequestView<'a>> {
        self.op.request.as_ref().map(|ty| match ty {
            CookedRequest::Json(index) => RequestView::Json(TypeView::new(self.cooked, *index)),
            CookedRequest::Multipart => RequestView::Multipart,
        })
    }

    /// Returns a view of the response body, if present.
    #[inline]
    pub fn response(&self) -> Option<ResponseView<'a>> {
        self.op.response.as_ref().map(|ty| match ty {
            CookedResponse::Json(index) => ResponseView::Json(TypeView::new(self.cooked, *index)),
        })
    }

    /// Returns the resource name that this operation declares
    /// in its `x-resource-name` extension field.
    #[inline]
    pub fn resource(&self) -> Option<&'a str> {
        self.op.resource
    }
}

impl<'a> View<'a> for OperationView<'a> {
    /// Returns an iterator over all the inline types that are
    /// contained within this operation's referenced types.
    #[inline]
    fn inlines(&self) -> impl Iterator<Item = InlineTypeView<'a>> + use<'a> {
        let cooked = self.cooked;
        // Only include edges to other inline schemas.
        let filtered = EdgeFiltered::from_fn(&cooked.graph, |e| {
            matches!(cooked.graph[e.target()], GraphNode::Inline(_))
        });
        let mut bfs = {
            let meta = &self.cooked.metadata.operations[&ByAddress(self.op)];
            let stack = meta
                .types
                .ones()
                .map(NodeIndex::new)
                .filter(|&index| {
                    // Exclude operation types that aren't inline schemas;
                    // those types, and their inlines, are already emitted
                    // as named schema types.
                    matches!(cooked.graph[index], GraphNode::Inline(_))
                })
                .collect();
            let mut discovered = self.cooked.graph.visit_map();
            for &index in &stack {
                discovered.visit(index);
            }
            Bfs { stack, discovered }
        };
        std::iter::from_fn(move || bfs.next(&filtered)).filter_map(|index| {
            match cooked.graph[index] {
                GraphNode::Inline(ty) => Some(InlineTypeView::new(cooked, index, ty)),
                _ => None,
            }
        })
    }

    /// Returns an empty iterator. Operations aren't "used by" other operations;
    /// they use types.
    #[inline]
    fn used_by(&self) -> impl Iterator<Item = OperationView<'a>> + use<'a> {
        std::iter::empty()
    }

    #[inline]
    fn dependencies(&self) -> impl Iterator<Item = TypeView<'a>> + use<'a> {
        let meta = &self.cooked.metadata.operations[&ByAddress(self.op)];
        let mut types = meta.types.clone();
        // Collect the transitive dependencies of each of the operation's
        // direct dependencies.
        for node in meta.types.ones() {
            let meta = &self.cooked.metadata.schemas[node];
            types.union_with(&meta.dependencies);
        }
        types
            .into_ones()
            .map(NodeIndex::new)
            .map(|index| TypeView::new(self.cooked, index))
    }

    /// Returns an empty iterator. Operations don't have dependents.
    #[inline]
    fn dependents(&self) -> impl Iterator<Item = TypeView<'a>> + use<'a> {
        std::iter::empty()
    }

    #[inline]
    fn traverse<F>(
        &self,
        reach: Reach,
        filter: F,
    ) -> impl Iterator<Item = TypeView<'a>> + use<'a, F>
    where
        F: Fn(EdgeKind, &TypeView<'a>) -> Traversal,
    {
        either!(match reach {
            Reach::Dependents => std::iter::empty(),
            Reach::Dependencies => {
                let cooked = self.cooked;
                let meta = &cooked.metadata.operations[&ByAddress(self.op)];
                let traverse = Traverse::from_roots(
                    &cooked.graph,
                    enum_map! {
                        EdgeKind::Reference => meta.types.clone(),
                        EdgeKind::Inherits => FixedBitSet::new(),
                    },
                    Direction::Outgoing,
                );
                traverse
                    .run(move |kind, index| {
                        let view = TypeView::new(cooked, index);
                        filter(kind, &view)
                    })
                    .map(|index| TypeView::new(cooked, index))
            }
        })
    }
}

/// A graph-aware view of operation's path template and parameters.
#[derive(Clone, Copy, Debug)]
pub struct OperationViewPath<'view, 'a>(&'view OperationView<'a>);

impl<'view, 'a> OperationViewPath<'view, 'a> {
    #[inline]
    pub fn segments(self) -> std::slice::Iter<'view, PathSegment<'a>> {
        self.0.op.path.iter()
    }

    /// Returns an iterator over this operation's path parameters.
    #[inline]
    pub fn params(self) -> impl Iterator<Item = ParameterView<'a, PathParameter>> + 'view {
        self.0
            .op
            .params
            .iter()
            .filter_map(move |param| match param {
                CookedParameter::Path(info) => Some(ParameterView::new(self.0.cooked, info)),
                _ => None,
            })
    }
}

/// A graph-aware view of an operation parameter.
#[derive(Debug)]
pub struct ParameterView<'a, T> {
    cooked: &'a CookedGraph<'a>,
    info: &'a CookedParameterInfo<'a>,
    phantom: PhantomData<T>,
}

impl<'a, T> ParameterView<'a, T> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        info: &'a CookedParameterInfo<'a>,
    ) -> Self {
        Self {
            cooked,
            info,
            phantom: PhantomData,
        }
    }

    #[inline]
    pub fn name(&self) -> &'a str {
        self.info.name
    }

    #[inline]
    pub fn ty(&self) -> TypeView<'a> {
        TypeView::new(self.cooked, self.info.ty)
    }

    #[inline]
    pub fn required(&self) -> bool {
        self.info.required
    }

    #[inline]
    pub fn style(&self) -> Option<ParameterStyle> {
        self.info.style
    }
}

/// A marker type for a path parameter.
#[derive(Clone, Copy, Debug)]
pub enum PathParameter {}

/// A marker type for a query parameter.
#[derive(Clone, Copy, Debug)]
pub enum QueryParameter {}

/// A graph-aware view of an operation's request body.
#[derive(Debug)]
pub enum RequestView<'a> {
    Json(TypeView<'a>),
    Multipart,
}

/// A graph-aware view of an operation's response body.
#[derive(Debug)]
pub enum ResponseView<'a> {
    Json(TypeView<'a>),
}
