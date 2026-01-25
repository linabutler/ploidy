use std::marker::PhantomData;

use by_address::ByAddress;
use fixedbitset::FixedBitSet;
use petgraph::{
    graph::NodeIndex,
    visit::{Bfs, VisitMap, Visitable},
};

use crate::{
    ir::{
        graph::{IrGraph, IrGraphNode},
        types::{
            IrOperation, IrParameter, IrParameterInfo, IrParameterStyle, IrRequest, IrResponse,
        },
    },
    parse::{Method, path::PathSegment},
};

use super::{Traversal, View, inline::InlineIrTypeView, ir::IrTypeView};

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

    /// Returns the resource name that this operation declares
    /// in its `x-resource-name` extension field.
    #[inline]
    pub fn resource(&self) -> Option<&'a str> {
        self.op.resource
    }

    fn bfs(&self) -> Bfs<NodeIndex<usize>, FixedBitSet> {
        // `Bfs::new()` starts with just one root on the stack,
        // but operations aren't roots; they reference types that are roots,
        // so we construct our own visitor with those types on the stack.
        let meta = &self.graph.metadata.operations[&ByAddress(self.op)];
        let mut discovered = self.graph.g.visit_map();
        discovered.union_with(&meta.types);
        let stack = discovered.ones().map(NodeIndex::new).collect();
        Bfs { stack, discovered }
    }
}

impl<'a> View<'a> for IrOperationView<'a> {
    /// Returns an iterator over all the inline types that are
    /// contained within this operation's referenced types.
    #[inline]
    fn inlines(&self) -> impl Iterator<Item = InlineIrTypeView<'a>> + use<'a> {
        self.reachable_if(|view| match view {
            // Yield inline types and continue into their fields.
            IrTypeView::Inline(_) => Traversal::Visit,

            // Stop at schema references; their inlines are emitted
            // by `CodegenSchemaType`.
            IrTypeView::Schema(_) => Traversal::Ignore,

            // Continue traversing into wrapper types without yielding.
            _ => Traversal::Skip,
        })
        .filter_map(|ty| match ty {
            IrTypeView::Inline(ty) => Some(ty),
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
    fn reachable(&self) -> impl Iterator<Item = IrTypeView<'a>> + use<'a> {
        let meta = &self.graph.metadata.operations[&ByAddress(self.op)];
        let mut types = meta.types.clone();
        // Collect the transitive dependencies of each of the operation's
        // direct dependencies.
        for node in meta.types.ones().map(NodeIndex::new) {
            let meta = &self.graph.metadata.schemas[&node];
            types.union_with(&meta.depends_on);
        }
        types
            .into_ones()
            .map(NodeIndex::new)
            .map(|index| IrTypeView::new(self.graph, index))
    }

    #[inline]
    fn reachable_if<F>(&self, filter: F) -> impl Iterator<Item = IrTypeView<'a>> + use<'a, F>
    where
        F: Fn(&IrTypeView<'a>) -> Traversal,
    {
        let graph = self.graph;
        let Bfs {
            mut stack,
            mut discovered,
        } = self.bfs();

        std::iter::from_fn(move || {
            while let Some(index) = stack.pop_front() {
                let view = IrTypeView::new(graph, index);
                let traversal = filter(&view);

                if matches!(traversal, Traversal::Visit | Traversal::Skip) {
                    // Add the neighbors to the stack of nodes to visit.
                    for neighbor in graph.g.neighbors(index) {
                        if discovered.visit(neighbor) {
                            stack.push_back(neighbor);
                        }
                    }
                }

                if matches!(traversal, Traversal::Visit | Traversal::Stop) {
                    // Yield this node.
                    return Some(view);
                }

                // (`Skip` and `Ignore` continue the loop without yielding).
            }
            None
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
