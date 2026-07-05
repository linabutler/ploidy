use indexmap::IndexMap;
use itertools::Itertools;
use rustc_hash::FxHashSet;

use crate::{
    arena::Arena,
    ir::OperationId,
    parse::{
        self, Document, Header, Info, Method, Operation, Parameter, ParameterLocation,
        ParameterStyle as ParsedParameterStyle, RefOrHeader, RefOrParameter, RefOrRequestBody,
        RefOrResponse, RefOrSchema, RequestBody, Response,
        path::{ParsedPath, PathFragment, PathSegment},
    },
};

use super::{
    error::IrError,
    transform::{TransformContext, TypeInfo, transform_with_context},
    types::{
        InlineTypeIds, ParameterStyle as IrParameterStyle, PrimitiveType, SchemaTypeInfo,
        SpecContainer, SpecInlineType, SpecInner, SpecOperation, SpecParameter, SpecParameterInfo,
        SpecRequest, SpecResponse, SpecSchemaType, SpecStruct, SpecStructField, SpecType,
        StructFieldName, shape::ResponseCase,
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
    /// Allocates inline type IDs.
    pub(crate) ids: InlineTypeIds<'a>,
}

impl<'a> Spec<'a> {
    /// Builds a [`Spec`] from a parsed OpenAPI [`Document`].
    ///
    /// Lowers each schema and operation to IR types, allocating all
    /// long-lived data in the `arena`. Returns an error if the document is
    /// malformed.
    pub fn from_doc(arena: &'a Arena, doc: &'a Document) -> Result<Self, IrError> {
        let ids = InlineTypeIds::new(arena);
        let context = TransformContext::new(arena, doc, ids);

        let schemas = match &doc.components {
            Some(components) => components
                .schemas
                .iter()
                .map(|(name, schema)| {
                    let ty = transform_with_context(
                        &context,
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
                let path = parse::path::parse(arena, path.as_str())?;
                Ok(item.operations().map(move |(method, op)| PathOperation {
                    path,
                    method,
                    params: &item.parameters,
                    op,
                }))
            })
            .flatten_ok()
            .map_ok(|item| -> Result<_, IrError> {
                let resource = item.op.extension("x-resource-name");
                let id = item
                    .op
                    .operation_id
                    .as_deref()
                    .ok_or(IrError::NoOperationId)?;

                let params = {
                    enum Source<'a> {
                        Declared(&'a Parameter),
                        Synthesized(&'a str),
                    }

                    // Merge path item and operation parameters.
                    // Operation parameters override path item ones.
                    let mut declared = IndexMap::new();
                    for param in item
                        .params
                        .iter()
                        .chain(item.op.parameters.iter())
                        .filter_map(|p| match p {
                            RefOrParameter::Other(p) => Some(p),
                            RefOrParameter::Ref(r) => {
                                r.ref_.pointer().follow::<&Parameter>(doc).ok()
                            }
                        })
                    {
                        declared.insert((param.name.as_str(), param.location), param);
                    }

                    // Walk the path template to produce path parameters in
                    // template order. Each template parameter name pulls its
                    // declaration from the merged map; undeclared names get
                    // a synthesized string parameter.
                    let mut sources = {
                        let mut seen = FxHashSet::default();
                        item.path
                            .segments
                            .iter()
                            .flat_map(|segment| match segment {
                                &PathSegment::Templated(fragments) => fragments,
                                PathSegment::Literal(_) => &[],
                            })
                            .filter_map(|fragment| match fragment {
                                &PathFragment::Param(name) => Some(name),
                                _ => None,
                            })
                            .filter(|&name| seen.insert(name))
                            .map(|name| {
                                match declared.shift_remove(&(name, ParameterLocation::Path)) {
                                    Some(param) => Source::Declared(param),
                                    None => Source::Synthesized(name),
                                }
                            })
                            .collect_vec()
                    };

                    // Append remaining parameters in declaration order.
                    sources.extend(declared.into_iter().filter_map(|((_, location), param)| {
                        match location {
                            // Drop declared path parameters that are
                            // absent from the template.
                            ParameterLocation::Path => None,
                            _ => Some(Source::Declared(param)),
                        }
                    }));

                    // Lower all sources to spec parameters.
                    let params = sources.into_iter().filter_map(|source| match source {
                        Source::Declared(param) => {
                            let ty: &_ = match &param.schema {
                                Some(RefOrSchema::Ref(r)) => arena.alloc(SpecType::Ref(r)),
                                Some(RefOrSchema::Inline(schema)) => arena
                                    .alloc(transform_with_context(&context, ids.next(), schema)),
                                None => arena.alloc(SpecInlineType::Any(ids.next()).into()),
                            };
                            let style = match (param.style, param.explode) {
                                (Some(ParsedParameterStyle::DeepObject), Some(true) | None) => {
                                    Some(IrParameterStyle::DeepObject)
                                }
                                (
                                    Some(ParsedParameterStyle::SpaceDelimited),
                                    Some(false) | None,
                                ) => Some(IrParameterStyle::SpaceDelimited),
                                (Some(ParsedParameterStyle::PipeDelimited), Some(false) | None) => {
                                    Some(IrParameterStyle::PipeDelimited)
                                }
                                (None, None) => None,
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
                        }
                        Source::Synthesized(name) => {
                            let ty: &_ = arena.alloc(SpecInlineType::Any(ids.next()).into());
                            Some(SpecParameter::Path(SpecParameterInfo {
                                name,
                                ty,
                                required: true,
                                description: None,
                                style: None,
                            }))
                        }
                    });

                    arena.alloc_slice(params)
                };

                let request = item
                    .op
                    .request_body
                    .as_ref()
                    .and_then(|request_or_ref| {
                        let request = match request_or_ref {
                            RefOrRequestBody::Other(rb) => rb,
                            RefOrRequestBody::Ref(r) => {
                                r.ref_.pointer().follow::<&RequestBody>(doc).ok()?
                            }
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
                            SpecRequest::Json(arena.alloc(SpecType::Ref(r)))
                        }
                        RequestContent::Json(RefOrSchema::Inline(schema)) => SpecRequest::Json(
                            arena.alloc(transform_with_context(&context, ids.next(), schema)),
                        ),
                        RequestContent::Any => {
                            SpecRequest::Json(arena.alloc(SpecInlineType::Any(ids.next()).into()))
                        }
                    });

                let responses = {
                    let mut statuses = item
                        .op
                        .responses
                        .keys()
                        .filter_map(|status| Some((status.as_str(), status.parse::<u16>().ok()?)))
                        .filter(|&(_, status)| matches!(status, 100..400))
                        .collect_vec();
                    statuses.sort_unstable_by_key(|&(_, code)| code);

                    if statuses.is_empty() {
                        statuses.extend(
                            item.op
                                .responses
                                .contains_key("default")
                                .then_some(("default", 200)),
                        );
                    }

                    let responses = statuses
                        .into_iter()
                        .filter_map(|(key, status)| {
                            let response_or_ref = item.op.responses.get(key)?;
                            let response = match response_or_ref {
                                RefOrResponse::Other(r) => r,
                                RefOrResponse::Ref(r) => {
                                    r.ref_.pointer().follow::<&Response>(doc).ok()?
                                }
                            };
                            let body = response
                                .content
                                .as_ref()
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
                                        SpecResponse::Json(arena.alloc(SpecType::Ref(r)))
                                    }
                                    ResponseContent::Json(RefOrSchema::Inline(schema)) => {
                                        SpecResponse::Json(arena.alloc(transform_with_context(
                                            &context,
                                            ids.next(),
                                            schema,
                                        )))
                                    }
                                    ResponseContent::Any => SpecResponse::Json(
                                        arena.alloc(SpecInlineType::Any(ids.next()).into()),
                                    ),
                                });
                            let body = body.or_else(|| {
                                let fields = response
                                    .headers
                                    .as_ref()?
                                    .iter()
                                    // Per OpenAPI, a `Content-Type` header
                                    // definition SHALL be ignored.
                                    .filter(|(name, _)| !name.eq_ignore_ascii_case("content-type"))
                                    .filter_map(|(name, header)| {
                                        let header = match header {
                                            RefOrHeader::Other(header) => header,
                                            RefOrHeader::Ref(r) => {
                                                r.ref_.pointer().follow::<&Header>(doc).ok()?
                                            }
                                        };
                                        let name: &_ = arena.alloc_str(&name.to_ascii_lowercase());
                                        // Header values arrive as strings on
                                        // the wire, so every header field is a
                                        // string regardless of its declared
                                        // schema type.
                                        let string: &_ = arena.alloc(
                                            SpecInlineType::Primitive(
                                                ids.next(),
                                                PrimitiveType::String,
                                            )
                                            .into(),
                                        );
                                        // Optional headers become nullable
                                        // fields, so the generated struct
                                        // holds `Option<String>`.
                                        let ty: &_ = if header.required {
                                            string
                                        } else {
                                            arena.alloc(
                                                SpecInlineType::Container(
                                                    ids.next(),
                                                    SpecContainer::Optional(SpecInner {
                                                        description: None,
                                                        ty: string,
                                                    }),
                                                )
                                                .into(),
                                            )
                                        };
                                        Some(SpecStructField {
                                            name: StructFieldName::Name(name),
                                            ty,
                                            required: true,
                                            description: header.description.as_deref(),
                                            flattened: false,
                                        })
                                    })
                                    .collect_vec();
                                (!fields.is_empty()).then(|| {
                                    SpecResponse::Headers(
                                        arena.alloc(
                                            SpecInlineType::Struct(
                                                ids.next(),
                                                SpecStruct {
                                                    description: response.description.as_deref(),
                                                    fields: arena.alloc_slice_copy(&fields),
                                                    parents: &[],
                                                },
                                            )
                                            .into(),
                                        ),
                                    )
                                })
                            });
                            Some(ResponseCase { status, body })
                        })
                        .collect_vec();
                    arena.alloc_slice_copy(&responses)
                };

                Ok(SpecOperation {
                    resource,
                    id: OperationId::new(id),
                    method: item.method,
                    path: item.path,
                    description: item.op.description.as_deref(),
                    params,
                    request,
                    responses,
                })
            })
            .flatten_ok()
            .collect::<Result<_, IrError>>()?;

        Ok(Spec {
            info: &doc.info,
            operations,
            schemas,
            ids,
        })
    }

    /// Resolves a [`SpecType`], following type references through the spec.
    #[inline]
    pub(super) fn resolve(&'a self, mut ty: &'a SpecType<'a>) -> ResolvedSpecType<'a> {
        loop {
            match ty {
                SpecType::Schema(ty) => return ResolvedSpecType::Schema(ty),
                SpecType::Inline(ty) => return ResolvedSpecType::Inline(ty),
                SpecType::Ref(r) => ty = &self.schemas[&*r.name()],
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

#[derive(Clone, Copy, Debug)]
struct PathOperation<'a> {
    path: ParsedPath<'a>,
    method: Method,
    params: &'a [RefOrParameter],
    op: &'a Operation,
}
