use indexmap::IndexMap;
use itertools::Itertools;
use ploidy_pointer::JsonPointee;

use crate::{
    arena::Arena,
    parse::{
        self, Document, Info, Parameter, ParameterLocation, ParameterStyle as ParsedParameterStyle,
        RefOrParameter, RefOrRequestBody, RefOrResponse, RefOrSchema, RequestBody, Response,
    },
};

use super::{
    error::IrError,
    graph::RawGraphNode,
    transform::transform,
    types::{
        InlineTypePath, InlineTypePathRoot, InlineTypePathSegment,
        ParameterStyle as IrParameterStyle, RawInlineType, RawOperation, RawParameter,
        RawParameterInfo, RawRequest, RawResponse, RawType, SchemaTypeInfo, TypeInfo,
    },
};

#[derive(Debug)]
pub struct IrSpec<'a> {
    pub info: &'a Info,
    pub operations: Vec<RawOperation<'a>>,
    pub schemas: IndexMap<&'a str, RawType<'a>>,
}

impl<'a> IrSpec<'a> {
    pub fn from_doc(arena: &'a Arena, doc: &'a Document) -> Result<Self, IrError> {
        let schemas = match &doc.components {
            Some(components) => components
                .schemas
                .iter()
                .map(|(name, schema)| {
                    let ty = transform(
                        arena,
                        doc,
                        TypeInfo::Schema(SchemaTypeInfo {
                            name,
                            resource: schema.extension("x-resourceId"),
                        }),
                        schema,
                    );
                    (name.as_str(), ty)
                })
                .collect(),
            None => IndexMap::new(),
        };

        let operations = doc
            .paths
            .iter()
            .map(|(path, item)| {
                let segments = parse::path::parse(path.as_str())?;
                Ok(item
                    .operations()
                    .map(move |(method, op)| (method, segments.clone(), op)))
            })
            .flatten_ok()
            .map_ok(|(method, path, op)| -> Result<_, IrError> {
                let resource = op.extension("x-resource-name");
                let id = op.operation_id.as_deref().ok_or(IrError::NoOperationId)?;
                let params = arena.alloc_slice(op.parameters.iter().filter_map(|param_or_ref| {
                    let param = match param_or_ref {
                        RefOrParameter::Other(p) => p,
                        RefOrParameter::Ref(r) => doc
                            .resolve(r.path.pointer().clone())
                            .ok()
                            .and_then(|p| p.downcast_ref::<Parameter>())?,
                    };
                    let ty: &_ = match &param.schema {
                        Some(RefOrSchema::Ref(r)) => arena.alloc(RawType::Ref(&r.path)),
                        Some(RefOrSchema::Other(schema)) => arena.alloc(transform(
                            arena,
                            doc,
                            InlineTypePath {
                                root: InlineTypePathRoot::Resource(resource),
                                segments: arena.alloc_slice_copy(&[
                                    InlineTypePathSegment::Operation(id),
                                    InlineTypePathSegment::Parameter(param.name.as_str()),
                                ]),
                            },
                            schema,
                        )),
                        None => arena.alloc(
                            RawInlineType::Any(InlineTypePath {
                                root: InlineTypePathRoot::Resource(resource),
                                segments: arena.alloc_slice_copy(&[
                                    InlineTypePathSegment::Operation(id),
                                    InlineTypePathSegment::Parameter(param.name.as_str()),
                                ]),
                            })
                            .into(),
                        ),
                    };
                    let style = match (param.style, param.explode) {
                        (Some(ParsedParameterStyle::DeepObject), Some(true) | None) => {
                            Some(IrParameterStyle::DeepObject)
                        }
                        (Some(ParsedParameterStyle::SpaceDelimited), Some(false) | None) => {
                            Some(IrParameterStyle::SpaceDelimited)
                        }
                        (Some(ParsedParameterStyle::PipeDelimited), Some(false) | None) => {
                            Some(IrParameterStyle::PipeDelimited)
                        }
                        (Some(ParsedParameterStyle::Form) | None, Some(true) | None) => {
                            Some(IrParameterStyle::Form { exploded: true })
                        }
                        (Some(ParsedParameterStyle::Form) | None, Some(false)) => {
                            Some(IrParameterStyle::Form { exploded: false })
                        }
                        _ => None,
                    };
                    let info = RawParameterInfo {
                        name: param.name.as_str(),
                        ty,
                        required: param.required,
                        description: param.description.as_deref(),
                        style,
                    };
                    Some(match param.location {
                        ParameterLocation::Path => RawParameter::Path(info),
                        ParameterLocation::Query => RawParameter::Query(info),
                        _ => return None,
                    })
                }));

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
                        RequestContent::Multipart => RawRequest::Multipart,
                        RequestContent::Json(RefOrSchema::Ref(r)) => {
                            RawRequest::Json(arena.alloc(RawType::Ref(&r.path)))
                        }
                        RequestContent::Json(RefOrSchema::Other(schema)) => {
                            RawRequest::Json(arena.alloc(transform(
                                arena,
                                doc,
                                InlineTypePath {
                                    root: InlineTypePathRoot::Resource(resource),
                                    segments: arena.alloc_slice_copy(&[
                                        InlineTypePathSegment::Operation(id),
                                        InlineTypePathSegment::Request,
                                    ]),
                                },
                                schema,
                            )))
                        }
                        RequestContent::Any => RawRequest::Json(
                            arena.alloc(
                                RawInlineType::Any(InlineTypePath {
                                    root: InlineTypePathRoot::Resource(resource),
                                    segments: arena.alloc_slice_copy(&[
                                        InlineTypePathSegment::Operation(id),
                                        InlineTypePathSegment::Request,
                                    ]),
                                })
                                .into(),
                            ),
                        ),
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
                                RawResponse::Json(arena.alloc(RawType::Ref(&r.path)))
                            }
                            ResponseContent::Json(RefOrSchema::Other(schema)) => {
                                RawResponse::Json(arena.alloc(transform(
                                    arena,
                                    doc,
                                    InlineTypePath {
                                        root: InlineTypePathRoot::Resource(resource),
                                        segments: arena.alloc_slice_copy(&[
                                            InlineTypePathSegment::Operation(id),
                                            InlineTypePathSegment::Response,
                                        ]),
                                    },
                                    schema,
                                )))
                            }
                            ResponseContent::Any => RawResponse::Json(
                                arena.alloc(
                                    RawInlineType::Any(InlineTypePath {
                                        root: InlineTypePathRoot::Resource(resource),
                                        segments: arena.alloc_slice_copy(&[
                                            InlineTypePathSegment::Operation(id),
                                            InlineTypePathSegment::Response,
                                        ]),
                                    })
                                    .into(),
                                ),
                            ),
                        })
                };

                Ok(RawOperation {
                    resource,
                    id,
                    method,
                    path: arena.alloc_slice_clone(&path),
                    description: op.description.as_deref(),
                    params,
                    request,
                    response,
                })
            })
            .flatten_ok()
            .collect::<Result<_, IrError>>()?;

        Ok(IrSpec {
            info: &doc.info,
            operations,
            schemas,
        })
    }

    /// Resolves a [`RawType`] to a [`RawGraphNode`], following
    /// [`RawType::Ref`]s through the spec.
    #[inline]
    pub fn resolve(&'a self, mut ty: &'a RawType<'a>) -> RawGraphNode<'a> {
        loop {
            match ty {
                RawType::Schema(ty) => return RawGraphNode::Schema(ty),
                RawType::Inline(ty) => return RawGraphNode::Inline(ty),
                RawType::Ref(r) => ty = &self.schemas[r.name()],
            }
        }
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
