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
    transform::transform,
    types::{
        InlineTypePath, InlineTypePathRoot, InlineTypePathSegment,
        ParameterStyle as IrParameterStyle, SchemaTypeInfo, SpecInlineType, SpecOperation,
        SpecParameter, SpecParameterInfo, SpecRequest, SpecResponse, SpecSchemaType, SpecType,
        TypeInfo,
    },
};

/// The intermediate representation of an OpenAPI document.
///
/// A [`Spec`] is a type tree lowered from a parsed document, with references
/// still unresolved. Construct one with [`Spec::from_doc()`], then pass it to
/// [`RawGraph::new()`] to build the type graph.
///
/// [`RawGraph::new()`]: crate::ir::RawGraph::new
#[derive(Debug)]
pub struct Spec<'a> {
    /// The document's `info` section: title, OpenAPI version, etc.
    pub info: &'a Info,
    /// All operations extracted from the document's `paths` section.
    pub operations: Vec<SpecOperation<'a>>,
    /// Named schemas from `components/schemas`, keyed by name.
    pub schemas: IndexMap<&'a str, SpecType<'a>>,
}

impl<'a> Spec<'a> {
    /// Builds a [`Spec`] from a parsed OpenAPI [`Document`].
    ///
    /// Lowers each schema and operation to IR types, allocating all
    /// long-lived data in the `arena`. Returns an error if the document is
    /// malformed.
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
                let segments = parse::path::parse(arena, path.as_str())?;
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
                        Some(RefOrSchema::Ref(r)) => arena.alloc(SpecType::Ref(&r.path)),
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
                            SpecInlineType::Any(InlineTypePath {
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
                    let info = SpecParameterInfo {
                        name: param.name.as_str(),
                        ty,
                        required: param.required,
                        description: param.description.as_deref(),
                        style,
                    };
                    Some(match param.location {
                        ParameterLocation::Path => SpecParameter::Path(info),
                        ParameterLocation::Query => SpecParameter::Query(info),
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
                        RequestContent::Multipart => SpecRequest::Multipart,
                        RequestContent::Json(RefOrSchema::Ref(r)) => {
                            SpecRequest::Json(arena.alloc(SpecType::Ref(&r.path)))
                        }
                        RequestContent::Json(RefOrSchema::Other(schema)) => {
                            SpecRequest::Json(arena.alloc(transform(
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
                        RequestContent::Any => SpecRequest::Json(
                            arena.alloc(
                                SpecInlineType::Any(InlineTypePath {
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
                                SpecResponse::Json(arena.alloc(SpecType::Ref(&r.path)))
                            }
                            ResponseContent::Json(RefOrSchema::Other(schema)) => {
                                SpecResponse::Json(arena.alloc(transform(
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
                            ResponseContent::Any => SpecResponse::Json(
                                arena.alloc(
                                    SpecInlineType::Any(InlineTypePath {
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

                Ok(SpecOperation {
                    resource,
                    id,
                    method,
                    path: arena.alloc_slice_copy(&path),
                    description: op.description.as_deref(),
                    params,
                    request,
                    response,
                })
            })
            .flatten_ok()
            .collect::<Result<_, IrError>>()?;

        Ok(Spec {
            info: &doc.info,
            operations,
            schemas,
        })
    }

    /// Resolves a [`SpecType`], following type references through the spec.
    #[inline]
    pub(super) fn resolve(&'a self, mut ty: &'a SpecType<'a>) -> ResolvedSpecType<'a> {
        loop {
            match ty {
                SpecType::Schema(ty) => return ResolvedSpecType::Schema(ty),
                SpecType::Inline(ty) => return ResolvedSpecType::Inline(ty),
                SpecType::Ref(r) => ty = &self.schemas[r.name()],
            }
        }
    }
}

/// A dereferenced type in the spec.
///
/// The derived [`Eq`] and [`Hash`][std::hash::Hash] implementations
/// use structural equality, not pointer identity. Multiple [`SpecType`]s
/// in a [`Spec`] may resolve to the same logical type, so value-based
/// comparison is necessary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum ResolvedSpecType<'a> {
    Schema(&'a SpecSchemaType<'a>),
    Inline(&'a SpecInlineType<'a>),
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
