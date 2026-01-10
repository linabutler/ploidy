use std::marker::PhantomData;

use petgraph::{
    graph::NodeIndex,
    visit::{Bfs, EdgeFiltered, EdgeRef, VisitMap, Visitable},
};

use crate::{
    ir::{
        graph::{IrGraph, IrGraphG, IrGraphNode},
        types::{
            IrOperation, IrParameter, IrParameterInfo, IrParameterStyle, IrRequest, IrResponse,
        },
    },
    parse::{Method, path::PathSegment},
};

use super::{inline::InlineIrTypeView, ir::IrTypeView};

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
    pub fn resource(&self) -> &'a str {
        self.op.resource
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
                let node = IrGraphNode::from_ref(self.graph.spec, ty.as_ref());
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
                let node = IrGraphNode::from_ref(self.graph.spec, ty.as_ref());
                IrResponseView::Json(IrTypeView::new(self.graph, self.graph.indices[&node]))
            }
        })
    }

    /// Returns an iterator over all the inline types that are
    /// contained within this operation's referenced types.
    #[inline]
    pub fn inlines(&self) -> impl Iterator<Item = InlineIrTypeView<'a>> {
        // Exclude edges that reference other schemas.
        let filtered = EdgeFiltered::from_fn(&self.graph.g, |r| {
            !matches!(self.graph.g[r.target()], IrGraphNode::Schema(_))
        });
        let mut bfs = self.bfs();
        std::iter::from_fn(move || bfs.next(&filtered)).filter_map(|index| {
            match self.graph.g[index] {
                IrGraphNode::Inline(ty) => Some(InlineIrTypeView::new(self.graph, index, ty)),
                _ => None,
            }
        })
    }

    fn bfs(&self) -> Bfs<NodeIndex, <IrGraphG<'a> as Visitable>::Map> {
        // `Bfs::new()` starts with just one root on the stack,
        // but operations aren't roots; they reference types that are roots,
        // so we construct our own visitor with all those types on the stack.
        let stack = self
            .op
            .types()
            .map(|ty| IrGraphNode::from_ref(self.graph.spec, ty.as_ref()))
            .map(|node| self.graph.indices[&node])
            .collect();
        let mut discovered = self.graph.g.visit_map();
        for &index in &stack {
            discovered.visit(index);
        }
        Bfs { stack, discovered }
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
        let node = IrGraphNode::from_ref(graph.spec, self.info.ty.as_ref());
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
