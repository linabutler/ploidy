use std::collections::BTreeSet;
use std::sync::OnceLock;

use indexmap::IndexMap;
use itertools::Itertools;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::Bfs;
use ploidy_pointer::JsonPointee;

use crate::parse::{
    self, Document, Info, Parameter, ParameterLocation, RefOrParameter, RefOrRequestBody,
    RefOrResponse, RefOrSchema, RequestBody, Response,
};

use super::{
    error::IrError,
    transform::transform,
    types::{
        InlineIrTypePath, InlineIrTypePathRoot, InlineIrTypePathSegment, IrOperation, IrParameter,
        IrParameterInfo, IrRequest, IrResponse, IrType, IrTypeName,
    },
    visitor::InnerRef,
};

#[derive(Debug)]
pub struct IrSpec<'a> {
    info: &'a Info,
    operations: Vec<IrOperation<'a>>,
    schemas: IndexMap<&'a str, (NodeIndex, IrType<'a>)>,
    refs: DiGraph<&'a str, ()>,
    circular_refs: OnceLock<BTreeSet<(NodeIndex, NodeIndex)>>,
}

impl<'a> IrSpec<'a> {
    pub fn from_doc(doc: &'a Document) -> Result<Self, IrError> {
        let mut schemas = IndexMap::new();
        let mut refs = DiGraph::new();

        if let Some(components) = &doc.components {
            for (name, schema) in &components.schemas {
                let index = refs.add_node(name.as_str());
                schemas.insert(
                    name.as_str(),
                    (index, transform(doc, IrTypeName::Schema(name), schema)),
                );
            }
        }

        for (from, ty) in schemas.values() {
            for r in ty.visit::<InnerRef>() {
                let &(to, _) = &schemas[r.name()];
                refs.add_edge(*from, to, ());
            }
        }

        let mut operations = vec![];
        for (path, item) in &doc.paths {
            let segments = parse::path::parse(path.as_str())?;
            for (method, op) in item.operations() {
                let resource = op.extension("x-resource-name").unwrap_or("full");
                let id = op.operation_id.as_deref().ok_or(IrError::NoOperationId)?;

                let params = op
                    .parameters
                    .iter()
                    .filter_map(|param_or_ref| {
                        let param = match param_or_ref {
                            RefOrParameter::Other(p) => p,
                            RefOrParameter::Ref(r) => doc
                                .resolve(r.path.pointer().clone())
                                .ok()
                                .and_then(|p| p.downcast_ref::<Parameter>())?,
                        };

                        let ty = match &param.schema {
                            Some(RefOrSchema::Ref(r)) => IrType::Ref(&r.path),
                            Some(RefOrSchema::Other(schema)) => transform(
                                doc,
                                InlineIrTypePath {
                                    root: InlineIrTypePathRoot::Resource(resource),
                                    segments: vec![
                                        InlineIrTypePathSegment::Operation(id),
                                        InlineIrTypePathSegment::Parameter(param.name.as_str()),
                                    ],
                                },
                                schema,
                            ),
                            None => IrType::Any,
                        };
                        let info = IrParameterInfo {
                            name: param.name.as_str(),
                            ty,
                            required: param.required,
                            description: param.description.as_deref(),
                        };
                        Some(match param.location {
                            ParameterLocation::Path => IrParameter::Path(info),
                            ParameterLocation::Query => IrParameter::Query(info),
                            _ => return None,
                        })
                    })
                    .collect_vec();

                let request = op
                    .request_body
                    .as_ref()
                    .and_then(|request_or_ref| {
                        let request = match request_or_ref {
                            RefOrRequestBody::Other(rb) => rb,
                            RefOrRequestBody::Ref(r) => doc
                                .resolve(r.path.pointer().clone())
                                .ok()
                                .and_then(|p| p.downcast_ref::<RequestBody>())?,
                        };

                        Some(if request.content.contains_key("multipart/form-data") {
                            RequestContent::Multipart
                        } else if let Some(content) = request.content.get("application/json")
                            && let Some(schema) = &content.schema
                        {
                            RequestContent::Json(schema)
                        } else if let Some(content) = request.content.get("*/*")
                            && let Some(schema) = &content.schema
                        {
                            RequestContent::Json(schema)
                        } else {
                            RequestContent::Any
                        })
                    })
                    .map(|content| match content {
                        RequestContent::Multipart => IrRequest::Multipart,
                        RequestContent::Json(RefOrSchema::Ref(r)) => {
                            IrRequest::Json(IrType::Ref(&r.path))
                        }
                        RequestContent::Json(RefOrSchema::Other(schema)) => {
                            IrRequest::Json(transform(
                                doc,
                                InlineIrTypePath {
                                    root: InlineIrTypePathRoot::Resource(resource),
                                    segments: vec![
                                        InlineIrTypePathSegment::Operation(id),
                                        InlineIrTypePathSegment::Request,
                                    ],
                                },
                                schema,
                            ))
                        }
                        RequestContent::Any => IrRequest::Json(IrType::Any),
                    });

                let response = {
                    let mut statuses = op
                        .responses
                        .keys()
                        .filter_map(|status| Some((status.as_str(), status.parse::<u16>().ok()?)))
                        .collect_vec();
                    statuses.sort_unstable_by_key(|&(_, code)| code);
                    let key = statuses
                        .iter()
                        .find(|&(_, code)| matches!(code, 200..300))
                        .map(|&(key, _)| key)
                        .unwrap_or("default");

                    op.responses
                        .get(key)
                        .and_then(|response_or_ref| {
                            let response = match response_or_ref {
                                RefOrResponse::Other(r) => r,
                                RefOrResponse::Ref(r) => doc
                                    .resolve(r.path.pointer().clone())
                                    .ok()
                                    .and_then(|p| p.downcast_ref::<Response>())?,
                            };
                            response.content.as_ref()
                        })
                        .map(|content| {
                            if let Some(content) = content.get("application/json")
                                && let Some(schema) = &content.schema
                            {
                                ResponseContent::Json(schema)
                            } else if let Some(content) = content.get("*/*")
                                && let Some(schema) = &content.schema
                            {
                                ResponseContent::Json(schema)
                            } else {
                                ResponseContent::Any
                            }
                        })
                        .map(|content| match content {
                            ResponseContent::Json(RefOrSchema::Ref(r)) => {
                                IrResponse::Json(IrType::Ref(&r.path))
                            }
                            ResponseContent::Json(RefOrSchema::Other(schema)) => {
                                IrResponse::Json(transform(
                                    doc,
                                    InlineIrTypePath {
                                        root: InlineIrTypePathRoot::Resource(resource),
                                        segments: vec![
                                            InlineIrTypePathSegment::Operation(id),
                                            InlineIrTypePathSegment::Request,
                                        ],
                                    },
                                    schema,
                                ))
                            }
                            ResponseContent::Any => IrResponse::Json(IrType::Any),
                        })
                };

                operations.push(IrOperation {
                    resource,
                    id,
                    method,
                    path: segments.clone(),
                    description: op.description.as_deref(),
                    params,
                    request,
                    response,
                });
            }
        }

        Ok(IrSpec {
            info: &doc.info,
            operations,
            schemas,
            refs,
            circular_refs: OnceLock::new(),
        })
    }

    #[inline]
    pub fn info(&self) -> &'a Info {
        self.info
    }

    /// Yields views for all operations in this spec.
    #[inline]
    pub fn operations(&self) -> impl Iterator<Item = IrOperationView<'_>> {
        self.operations
            .iter()
            .map(|op| IrOperationView { spec: self, op })
    }

    /// Yields views for all schemas in this spec.
    #[inline]
    pub fn schemas(&self) -> impl Iterator<Item = IrSchemaView<'_>> {
        self.schemas
            .iter()
            .map(|(_, &(index, _))| IrSchemaView::new(self, index))
    }

    /// Looks up a schema by name, and returns a view for that schema.
    #[inline]
    pub fn lookup(&self, name: &str) -> Option<IrSchemaView<'_>> {
        self.schemas
            .get(name)
            .map(|&(index, _)| IrSchemaView::new(self, index))
    }

    /// Finds all circular reference edges in this spec. Each edge
    /// in the returned set participates in a circular reference,
    /// and requires indirection to break the cycle.
    fn circular_refs(&self) -> &BTreeSet<(NodeIndex, NodeIndex)> {
        self.circular_refs.get_or_init(|| {
            let mut edges = BTreeSet::new();
            let sccs = tarjan_scc(&self.refs);
            for scc in sccs {
                let scc = BTreeSet::from_iter(scc);
                for &node in &scc {
                    // Collect internal edges within this
                    // strongly connected component.
                    edges.extend(
                        self.refs
                            .neighbors(node)
                            .filter(|neighbor| scc.contains(neighbor))
                            .map(|neighbor| (node, neighbor)),
                    );
                }
            }
            edges
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct IrOperationView<'a> {
    spec: &'a IrSpec<'a>,
    op: &'a IrOperation<'a>,
}

impl<'a> IrOperationView<'a> {
    #[inline]
    pub fn op(self) -> &'a IrOperation<'a> {
        self.op
    }

    pub fn refs(self) -> impl Iterator<Item = IrSchemaView<'a>> {
        itertools::chain!(
            // Parameter type references.
            self.op
                .params
                .iter()
                .map(|param| match param {
                    IrParameter::Path(info) => info,
                    IrParameter::Query(info) => info,
                })
                .flat_map(|info| info.ty.visit()),
            // Request type references.
            self.op
                .request
                .iter()
                .filter_map(|request| match request {
                    IrRequest::Multipart => None,
                    IrRequest::Json(ty) => Some(ty),
                })
                .flat_map(|ty| ty.visit()),
            // Response type references.
            self.op
                .response
                .iter()
                .map(|response| match response {
                    IrResponse::Json(ty) => ty,
                })
                .flat_map(|ty| ty.visit()),
        )
        .flat_map(move |r: InnerRef| {
            let &(index, _) = &self.spec.schemas[r.name()];
            let mut bfs = Bfs::new(&self.spec.refs, index);
            std::iter::from_fn(move || bfs.next(&self.spec.refs))
        })
        .map(|index| IrSchemaView {
            spec: self.spec,
            index,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct IrSchemaView<'a> {
    spec: &'a IrSpec<'a>,
    index: NodeIndex,
}

impl<'a> IrSchemaView<'a> {
    #[inline]
    fn new(spec: &'a IrSpec<'a>, index: NodeIndex) -> Self {
        Self { spec, index }
    }

    #[inline]
    pub fn name(self) -> &'a str {
        self.spec.refs[self.index]
    }

    #[inline]
    pub fn ty(self) -> &'a IrType<'a> {
        let (_, ty) = &self.spec.schemas[self.name()];
        ty
    }

    /// Returns `true` if a reference from this schema to the `other` schema
    /// requires indirection (with [`Box`], [`Vec`], etc.)
    #[inline]
    pub fn requires_indirection_to(&self, other: IrSchemaView<'_>) -> bool {
        self.spec
            .circular_refs()
            .contains(&(self.index, other.index))
    }

    #[inline]
    pub fn refs(self) -> impl Iterator<Item = IrSchemaView<'a>> {
        let mut bfs = Bfs::new(&self.spec.refs, self.index);
        std::iter::from_fn(move || bfs.next(&self.spec.refs))
            .map(|index| IrSchemaView::new(self.spec, index))
    }
}

#[derive(Clone, Copy, Debug)]
enum RequestContent<'a> {
    Multipart,
    Json(&'a RefOrSchema),
    Any,
}

#[derive(Clone, Copy, Debug)]
enum ResponseContent<'a> {
    Json(&'a RefOrSchema),
    Any,
}
