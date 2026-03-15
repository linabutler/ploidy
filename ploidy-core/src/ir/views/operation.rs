//! Operations: per-path methods with parameter, request, and response schemas.
//!
//! In OpenAPI, each path item defines operations for HTTP methods like
//! `GET` and `POST`. An operation has path and query parameters, an
//! optional request body, and an optional response body:
//!
//! ```yaml
//! paths:
//!   /pets/{pet_id}:
//!     post:
//!       operationId: updatePet
//!       parameters:
//!         - name: pet_id
//!           in: path
//!           required: true
//!           schema:
//!             type: string
//!         - name: expand
//!           in: query
//!           schema:
//!             type: boolean
//!       requestBody:
//!         content:
//!           application/json:
//!             schema:
//!               $ref: '#/components/schemas/PetUpdate'
//!       responses:
//!         '200':
//!           content:
//!             application/json:
//!               schema:
//!                 $ref: '#/components/schemas/Pet'
//! ```
//!
//! Ploidy represents this as an [`OperationView`] with:
//!
//! * An [ID], an [HTTP method], and a [path template] with
//!   segments and path parameters.
//! * [Query parameters], each with a name, type, and
//!   optional serialization style.
//! * An optional [request] and [response] body, each wrapping
//!   a [`TypeView`] of the body schema.
//! * An optional [resource name] from the `x-resource-name` extension,
//!   used to group operations by resource.
//!
//! Unlike types, operations are not nodes in Ploidy's dependency graph,
//! but they implement [`View`] for traversal.
//!
//! [ID]: OperationView::id
//! [HTTP method]: OperationView::method
//! [path template]: OperationView::path
//! [Query parameters]: OperationView::query
//! [request]: OperationView::request
//! [response]: OperationView::response
//! [resource name]: OperationView::resource

use std::marker::PhantomData;

use enum_map::enum_map;
use fixedbitset::FixedBitSet;
use petgraph::{
    Direction,
    graph::NodeIndex,
    visit::{Bfs, EdgeFiltered, EdgeRef, VisitMap, Visitable},
};

use crate::{
    ir::{
        graph::{CookedGraph, EdgeKind, Traversal, Traverse},
        types::{
            GraphOperation, GraphParameter, GraphParameterInfo, GraphRequest, GraphResponse,
            GraphType, ParameterStyle,
        },
    },
    parse::{Method, path::PathSegment},
};

use super::{Reach, View, inline::InlineTypeView, ir::TypeView};

/// A graph-aware view of an [operation][GraphOperation].
#[derive(Debug)]
pub struct OperationView<'a> {
    cooked: &'a CookedGraph<'a>,
    op: &'a GraphOperation<'a>,
}

impl<'a> OperationView<'a> {
    #[inline]
    pub(in crate::ir) fn new(cooked: &'a CookedGraph<'a>, op: &'a GraphOperation<'a>) -> Self {
        Self { cooked, op }
    }

    /// Returns the `operationId`.
    #[inline]
    pub fn id(&self) -> &'a str {
        self.op.id
    }

    /// Returns the HTTP method.
    #[inline]
    pub fn method(&self) -> Method {
        self.op.method
    }

    /// Returns a view of this operation's path template.
    #[inline]
    pub fn path(&self) -> OperationViewPath<'_, 'a> {
        OperationViewPath(self)
    }

    /// Returns the description, if present in the spec.
    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.op.description
    }

    /// Returns an iterator over this operation's query parameters.
    #[inline]
    pub fn query(&self) -> impl Iterator<Item = ParameterView<'a, QueryParameter>> + '_ {
        self.op.params.iter().filter_map(move |param| match param {
            GraphParameter::Query(info) => Some(ParameterView::new(self.cooked, info)),
            _ => None,
        })
    }

    /// Returns a view of the request body, if present.
    #[inline]
    pub fn request(&self) -> Option<RequestView<'a>> {
        self.op.request.as_ref().map(|ty| match ty {
            GraphRequest::Json(index) => RequestView::Json(TypeView::new(self.cooked, *index)),
            GraphRequest::Multipart => RequestView::Multipart,
        })
    }

    /// Returns a view of the response body, if present.
    #[inline]
    pub fn response(&self) -> Option<ResponseView<'a>> {
        self.op.response.as_ref().map(|ty| match ty {
            GraphResponse::Json(index) => ResponseView::Json(TypeView::new(self.cooked, *index)),
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
            matches!(cooked.graph[e.target()], GraphType::Inline(_))
        });
        let mut bfs = {
            let meta = &self.cooked.metadata.operations[self.op];
            let stack = meta
                .types
                .ones()
                .map(NodeIndex::new)
                .filter(|&index| {
                    // Exclude operation types that aren't inline schemas;
                    // those types, and their inlines, are already emitted
                    // as named schema types.
                    matches!(cooked.graph[index], GraphType::Inline(_))
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
                GraphType::Inline(ty) => Some(InlineTypeView::new(cooked, index, ty)),
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
        let meta = &self.cooked.metadata.operations[self.op];
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
                let meta = &cooked.metadata.operations[self.op];
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
    /// Returns an iterator over this path's segments.
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
                GraphParameter::Path(info) => Some(ParameterView::new(self.0.cooked, info)),
                _ => None,
            })
    }
}

/// A graph-aware view of an operation parameter.
#[derive(Debug)]
pub struct ParameterView<'a, T> {
    cooked: &'a CookedGraph<'a>,
    info: &'a GraphParameterInfo<'a>,
    phantom: PhantomData<T>,
}

impl<'a, T> ParameterView<'a, T> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        info: &'a GraphParameterInfo<'a>,
    ) -> Self {
        Self {
            cooked,
            info,
            phantom: PhantomData,
        }
    }

    /// Returns the parameter name.
    #[inline]
    pub fn name(&self) -> &'a str {
        self.info.name
    }

    /// Returns a view of the parameter's type.
    #[inline]
    pub fn ty(&self) -> TypeView<'a> {
        TypeView::new(self.cooked, self.info.ty)
    }

    /// Returns `true` if this parameter is required.
    #[inline]
    pub fn required(&self) -> bool {
        self.info.required
    }

    /// Returns the serialization style, if specified.
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
