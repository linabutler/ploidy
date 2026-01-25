use indexmap::IndexMap;
use itertools::Itertools;
use ploidy_pointer::JsonPointee;

use crate::{
    ir::SchemaTypeInfo,
    parse::{
        self, Document, Info, Parameter, ParameterLocation, ParameterStyle, RefOrParameter,
        RefOrRequestBody, RefOrResponse, RefOrSchema, RequestBody, Response,
    },
};

use super::{
    error::IrError,
    transform::transform,
    types::{
        InlineIrTypePath, InlineIrTypePathRoot, InlineIrTypePathSegment, IrOperation, IrParameter,
        IrParameterInfo, IrParameterStyle, IrRequest, IrResponse, IrType, IrTypeName,
    },
};

#[derive(Debug)]
pub struct IrSpec<'a> {
    pub info: &'a Info,
    pub operations: Vec<IrOperation<'a>>,
    pub schemas: IndexMap<&'a str, IrType<'a>>,
}

impl<'a> IrSpec<'a> {
    pub fn from_doc(doc: &'a Document) -> Result<Self, IrError> {
        let schemas = match &doc.components {
            Some(components) => components
                .schemas
                .iter()
                .map(|(name, schema)| {
                    let ty = transform(
                        doc,
                        IrTypeName::Schema(SchemaTypeInfo {
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
                        let style = match (param.style, param.explode) {
                            (Some(ParameterStyle::DeepObject), Some(true) | None) => {
                                Some(IrParameterStyle::DeepObject)
                            }
                            (Some(ParameterStyle::SpaceDelimited), Some(false) | None) => {
                                Some(IrParameterStyle::SpaceDelimited)
                            }
                            (Some(ParameterStyle::PipeDelimited), Some(false) | None) => {
                                Some(IrParameterStyle::PipeDelimited)
                            }
                            (Some(ParameterStyle::Form) | None, Some(true) | None) => {
                                Some(IrParameterStyle::Form { exploded: true })
                            }
                            (Some(ParameterStyle::Form) | None, Some(false)) => {
                                Some(IrParameterStyle::Form { exploded: false })
                            }
                            _ => None,
                        };
                        let info = IrParameterInfo {
                            name: param.name.as_str(),
                            ty,
                            required: param.required,
                            description: param.description.as_deref(),
                            style,
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
                                            InlineIrTypePathSegment::Response,
                                        ],
                                    },
                                    schema,
                                ))
                            }
                            ResponseContent::Any => IrResponse::Json(IrType::Any),
                        })
                };

                Ok(IrOperation {
                    resource,
                    id,
                    method,
                    path,
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
