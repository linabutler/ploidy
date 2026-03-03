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
        graph::{EdgeKind, IrGraph, IrGraphNode, Traversal, Traverse},
        types::{
            IrOperation, IrParameter, IrParameterInfo, IrParameterStyle, IrRequest, IrResponse,
        },
    },
    parse::{Method, path::PathSegment},
};

use super::{Reach, View, inline::InlineIrTypeView, ir::IrTypeView};

/// A graph-aware view of an [`IrOperation`].
#[derive(Debug)]
pub struct IrOperationView<'a> {
    graph: &'a IrGraph<'a>,
    op: &'a IrOperation<'a>,
}

impl<'a> IrOperationView<'a> {
    pub(in crate::ir) fn new(graph: &'a IrGraph<'a>, op: &'a IrOperation<'a>) -> Self {
        Self { graph, op }
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
    pub fn path(&self) -> IrOperationViewPath<'_, 'a> {
        IrOperationViewPath(self)
    }

    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.op.description
    }

    /// Returns an iterator over this operation's query parameters.
    #[inline]
    pub fn query(&self) -> impl Iterator<Item = IrParameterView<'a, IrQueryParameter>> + '_ {
        self.op.params.iter().filter_map(move |param| match param {
            IrParameter::Query(info) => Some(IrParameterView::new(self.graph, info)),
            _ => None,
        })
    }

    /// Returns a view of the request body, if present.
    #[inline]
    pub fn request(&self) -> Option<IrRequestView<'a>> {
        self.op.request.as_ref().map(|ty| match ty {
            IrRequest::Json(ty) => {
                let node = self.graph.resolve_type(ty.as_ref());
                IrRequestView::Json(IrTypeView::new(self.graph, self.graph.indices[&node]))
            }
            IrRequest::Multipart => IrRequestView::Multipart,
        })
    }

    /// Returns a view of the response body, if present.
    #[inline]
    pub fn response(&self) -> Option<IrResponseView<'a>> {
        self.op.response.as_ref().map(|ty| match ty {
            IrResponse::Json(ty) => {
                let node = self.graph.resolve_type(ty.as_ref());
                IrResponseView::Json(IrTypeView::new(self.graph, self.graph.indices[&node]))
            }
        })
    }

    /// Returns the resource name that this operation declares
    /// in its `x-resource-name` extension field.
    #[inline]
    pub fn resource(&self) -> Option<&'a str> {
        self.op.resource
    }
}

impl<'a> View<'a> for IrOperationView<'a> {
    /// Returns an iterator over all the inline types that are
    /// contained within this operation's referenced types.
    #[inline]
    fn inlines(&self) -> impl Iterator<Item = InlineIrTypeView<'a>> + use<'a> {
        let graph = self.graph;
        // Only include edges to other inline schemas.
        let filtered = EdgeFiltered::from_fn(&graph.g, |r| {
            matches!(graph.g[r.target()], IrGraphNode::Inline(_))
        });
        let mut bfs = {
            let meta = &self.graph.metadata.operations[&ByAddress(self.op)];
            let stack = meta
                .types
                .ones()
                .map(NodeIndex::new)
                .filter(|&index| {
                    // Exclude operation types that aren't inline schemas;
                    // those types, and their inlines, are already emitted
                    // as named schema types.
                    matches!(graph.g[index], IrGraphNode::Inline(_))
                })
                .collect();
            let mut discovered = self.graph.g.visit_map();
            for &index in &stack {
                discovered.visit(index);
            }
            Bfs { stack, discovered }
        };
        std::iter::from_fn(move || bfs.next(&filtered)).filter_map(|index| match graph.g[index] {
            IrGraphNode::Inline(ty) => Some(InlineIrTypeView::new(graph, index, ty)),
            _ => None,
        })
    }

    /// Returns an empty iterator. Operations aren't "used by" other operations;
    /// they use types.
    #[inline]
    fn used_by(&self) -> impl Iterator<Item = IrOperationView<'a>> + use<'a> {
        std::iter::empty()
    }

    #[inline]
    fn dependencies(&self) -> impl Iterator<Item = IrTypeView<'a>> + use<'a> {
        let meta = &self.graph.metadata.operations[&ByAddress(self.op)];
        let mut types = meta.types.clone();
        // Collect the transitive dependencies of each of the operation's
        // direct dependencies.
        for node in meta.types.ones().map(NodeIndex::new) {
            let meta = &self.graph.metadata.schemas[&node];
            types.union_with(&meta.dependencies);
        }
        types
            .into_ones()
            .map(NodeIndex::new)
            .map(|index| IrTypeView::new(self.graph, index))
    }

    /// Returns an empty iterator. Operations don't have dependents.
    #[inline]
    fn dependents(&self) -> impl Iterator<Item = IrTypeView<'a>> + use<'a> {
        std::iter::empty()
    }

    #[inline]
    fn traverse<F>(
        &self,
        reach: Reach,
        filter: F,
    ) -> impl Iterator<Item = IrTypeView<'a>> + use<'a, F>
    where
        F: Fn(EdgeKind, &IrTypeView<'a>) -> Traversal,
    {
        either!(match reach {
            Reach::Dependents => std::iter::empty(),
            Reach::Dependencies => {
                let graph = self.graph;
                let meta = &graph.metadata.operations[&ByAddress(self.op)];
                let traverse = Traverse::from_roots(
                    &graph.g,
                    enum_map! {
                        EdgeKind::Reference => meta.types.clone(),
                        EdgeKind::Inherits => FixedBitSet::new(),
                    },
                    Direction::Outgoing,
                );
                traverse
                    .run(move |kind, index| {
                        let view = IrTypeView::new(graph, index);
                        filter(kind, &view)
                    })
                    .map(|index| IrTypeView::new(graph, index))
            }
        })
    }
}

/// A graph-aware view of operation's path template and parameters.
#[derive(Clone, Copy, Debug)]
pub struct IrOperationViewPath<'view, 'a>(&'view IrOperationView<'a>);

impl<'view, 'a> IrOperationViewPath<'view, 'a> {
    #[inline]
    pub fn segments(self) -> std::slice::Iter<'view, PathSegment<'a>> {
        self.0.op.path.iter()
    }

    /// Returns an iterator over this operation's path parameters.
    #[inline]
    pub fn params(self) -> impl Iterator<Item = IrParameterView<'a, IrPathParameter>> + 'view {
        self.0
            .op
            .params
            .iter()
            .filter_map(move |param| match param {
                IrParameter::Path(info) => Some(IrParameterView::new(self.0.graph, info)),
                _ => None,
            })
    }
}

/// A graph-aware view of an operation parameter.
#[derive(Debug)]
pub struct IrParameterView<'a, T> {
    graph: &'a IrGraph<'a>,
    info: &'a IrParameterInfo<'a>,
    phantom: PhantomData<T>,
}

impl<'a, T> IrParameterView<'a, T> {
    pub(in crate::ir) fn new(graph: &'a IrGraph<'a>, info: &'a IrParameterInfo<'a>) -> Self {
        Self {
            graph,
            info,
            phantom: PhantomData,
        }
    }

    #[inline]
    pub fn name(&self) -> &'a str {
        self.info.name
    }

    #[inline]
    pub fn ty(&self) -> IrTypeView<'a> {
        let graph = self.graph;
        let node = graph.resolve_type(self.info.ty.as_ref());
        IrTypeView::new(graph, graph.indices[&node])
    }

    #[inline]
    pub fn required(&self) -> bool {
        self.info.required
    }

    #[inline]
    pub fn style(&self) -> Option<IrParameterStyle> {
        self.info.style
    }
}

/// A marker type for a path parameter.
#[derive(Clone, Copy, Debug)]
pub enum IrPathParameter {}

/// A marker type for a query parameter.
#[derive(Clone, Copy, Debug)]
pub enum IrQueryParameter {}

/// A graph-aware view of an operation's request body.
#[derive(Debug)]
pub enum IrRequestView<'a> {
    Json(IrTypeView<'a>),
    Multipart,
}

/// A graph-aware view of an operation's response body.
#[derive(Debug)]
pub enum IrResponseView<'a> {
    Json(IrTypeView<'a>),
}
