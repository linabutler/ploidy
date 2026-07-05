use itertools::Itertools;
use ploidy_core::ir::{HasTypeId, InlineTypeView, OperationView, View};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    cfg::CfgFeature, enum_::CodegenEnum, graph::CodegenGraph, naming::CodegenIdentUsage,
    struct_::CodegenStruct, tagged::CodegenTagged, untagged::CodegenUntagged,
};

/// Generates a `mod types` for operation-local types.
#[derive(Debug)]
pub struct CodegenInlines<'a> {
    graph: &'a CodegenGraph<'a>,
    inlines: Vec<InlineTypeView<'a, 'a>>,
    cfg: bool,
}

impl<'a> CodegenInlines<'a> {
    /// Creates a codegen node for a schema module's inline types.
    pub fn for_schema_inlines(
        graph: &'a CodegenGraph<'a>,
        inlines: Vec<InlineTypeView<'a, 'a>>,
    ) -> Self {
        Self {
            graph,
            inlines,
            cfg: false,
        }
    }

    /// Creates a codegen node for a resource module's inline types.
    #[cfg(test)]
    pub fn for_resource_inlines(
        graph: &'a CodegenGraph<'a>,
        inlines: Vec<InlineTypeView<'a, 'a>>,
    ) -> Self {
        Self {
            graph,
            inlines,
            cfg: true,
        }
    }

    /// Creates a codegen node for a resource module's `types` submodule.
    pub fn for_resource_types(
        graph: &'a CodegenGraph<'a>,
        ops: &'a [OperationView<'a, 'a>],
    ) -> Self {
        Self {
            graph,
            inlines: ops.iter().flat_map(|op| op.inlines()).collect(),
            cfg: true,
        }
    }
}

impl ToTokens for CodegenInlines<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let graph = self.graph;

        let mut items = self
            .inlines
            .iter()
            .filter_map(|view| {
                let (ident, ty) = match view {
                    InlineTypeView::Struct(_, view) => (
                        graph.ident(view.id()),
                        CodegenStruct::new(graph, view).into_token_stream(),
                    ),
                    InlineTypeView::Enum(_, view) => (
                        graph.ident(view.id()),
                        CodegenEnum::new(graph, view).into_token_stream(),
                    ),
                    InlineTypeView::Tagged(_, view) => (
                        graph.ident(view.id()),
                        CodegenTagged::new(graph, view).into_token_stream(),
                    ),
                    InlineTypeView::Untagged(_, view) => (
                        graph.ident(view.id()),
                        CodegenUntagged::new(graph, view).into_token_stream(),
                    ),
                    InlineTypeView::Container(..)
                    | InlineTypeView::Primitive(..)
                    | InlineTypeView::Any(..) => {
                        // Container types, primitive types, and untyped values
                        // are emitted directly; they don't need type aliases.
                        return None;
                    }
                };
                let item = if self.cfg {
                    // Wrap each type in an inner module, so that the
                    // `#[cfg(...)]` applies to all items (types and `impl`s).
                    let cfg = CfgFeature::for_inline_type(graph, view);
                    let mod_name = CodegenIdentUsage::Module(ident);
                    quote! {
                        #cfg
                        mod #mod_name {
                            #ty
                        }
                        #cfg
                        pub use #mod_name::*;
                    }
                } else {
                    ty
                };
                Some((ident, item))
            })
            .collect_vec();

        items.sort_by_key(|&(ident, _)| ident);
        let mut items = items.into_iter().map(|(_, item)| item);

        if let Some(first) = items.next() {
            tokens.append_all(quote! {
                pub mod types {
                    #first
                    #(#items)*
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        arena::Arena,
        ir::{RawGraph, Spec, View},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    use crate::graph::CodegenGraph;

    #[test]
    fn test_includes_inline_types_from_operation_parameters() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: getItems
                  parameters:
                    - name: filter
                      in: query
                      schema:
                        type: object
                        properties:
                          status:
                            type: string
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines = CodegenInlines::for_resource_inlines(
            &graph,
            graph.operations().flat_map(|op| op.inlines()).collect(),
        );

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_items_query_filter {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetItemsQueryFilter {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub status: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_items_query_filter::*;
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_resource_types_include_header_struct() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /jobs:
                get:
                  operationId: getJob
                  responses:
                    '304':
                      description: Not modified.
                      headers:
                        etag:
                          required: true
                          schema:
                            type: string
                        cache-control:
                          schema:
                            type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines = CodegenInlines::for_resource_inlines(
            &graph,
            graph.operations().flat_map(|op| op.inlines()).collect(),
        );

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_job_response {
                    #[doc = " Not modified."]
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetJobResponse {
                        pub etag: ::std::string::String,
                        #[serde(rename = "cache-control")]
                        #[ploidy(pointer(rename = "cache-control"))]
                        pub cache_control: ::std::option::Option<::std::string::String>,
                    }
                }
                pub use get_job_response::*;
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_operation_parameter_inline_type_names_do_not_collide_across_roles() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /things/{id}:
                get:
                  operationId: getThing
                  parameters:
                    - name: id
                      in: path
                      required: true
                      schema:
                        type: object
                        properties:
                          path_value:
                            type: string
                    - name: id
                      in: query
                      schema:
                        type: object
                        properties:
                          query_value:
                            type: string
                    - name: request
                      in: query
                      schema:
                        type: object
                        properties:
                          query_request_value:
                            type: string
                    - name: response
                      in: query
                      schema:
                        type: object
                        properties:
                          query_response_value:
                            type: string
                  requestBody:
                    content:
                      application/json:
                        schema:
                          type: object
                          properties:
                            body_request_value:
                              type: string
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            type: object
                            properties:
                              body_response_value:
                                type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines = CodegenInlines::for_resource_inlines(
            &graph,
            graph.operations().flat_map(|op| op.inlines()).collect(),
        );

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_thing_path_id {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetThingPathId {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub path_value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_thing_path_id::*;
                mod get_thing_query_id {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetThingQueryId {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub query_value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_thing_query_id::*;
                mod get_thing_query_request {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetThingQueryRequest {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub query_request_value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_thing_query_request::*;
                mod get_thing_query_response {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetThingQueryResponse {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub query_response_value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_thing_query_response::*;
                mod get_thing_request {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetThingRequest {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub body_request_value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_thing_request::*;
                mod get_thing_response {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetThingResponse {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub body_response_value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_thing_response::*;
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_excludes_inline_types_from_referenced_schemas() {
        // The operation references `Item`, which has an inline type `Details`.
        // `Details` should _not_ be emitted by `CodegenInlines`; it belongs in
        // the schema's module instead.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: getItems
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Item'
            components:
              schemas:
                Item:
                  type: object
                  properties:
                    details:
                      type: object
                      properties:
                        description:
                          type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines = CodegenInlines::for_resource_inlines(
            &graph,
            graph.operations().flat_map(|op| op.inlines()).collect(),
        );

        // No inline types should be emitted, since the only inline (`Details`)
        // belongs to the referenced schema.
        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {};
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_sorts_inline_types_alphabetically() {
        // Parameters defined in reverse order: zebra, mango, apple.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: getItems
                  parameters:
                    - name: zebra
                      in: query
                      schema:
                        type: object
                        properties:
                          value:
                            type: string
                    - name: mango
                      in: query
                      schema:
                        type: object
                        properties:
                          value:
                            type: string
                    - name: apple
                      in: query
                      schema:
                        type: object
                        properties:
                          value:
                            type: string
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines = CodegenInlines::for_resource_inlines(
            &graph,
            graph.operations().flat_map(|op| op.inlines()).collect(),
        );

        let actual: syn::File = parse_quote!(#inlines);
        // Types should be sorted: Apple, Mango, Zebra.
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_items_query_apple {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetItemsQueryApple {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_items_query_apple::*;
                mod get_items_query_mango {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetItemsQueryMango {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_items_query_mango::*;
                mod get_items_query_zebra {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetItemsQueryZebra {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_items_query_zebra::*;
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_no_output_when_no_inline_types() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: getItems
                  parameters:
                    - name: limit
                      in: query
                      schema:
                        type: integer
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines = CodegenInlines::for_resource_inlines(
            &graph,
            graph.operations().flat_map(|op| op.inlines()).collect(),
        );

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {};
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_finds_inline_types_within_optionals() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: getItems
                  parameters:
                    - name: config
                      in: query
                      schema:
                        nullable: true
                        type: object
                        properties:
                          enabled:
                            type: boolean
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines = CodegenInlines::for_resource_inlines(
            &graph,
            graph.operations().flat_map(|op| op.inlines()).collect(),
        );

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_items_query_config {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetItemsQueryConfig {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub enabled: ::ploidy_util::absent::AbsentOr<bool>,
                    }
                }
                pub use get_items_query_config::*;
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_finds_inline_types_within_arrays() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: getItems
                  parameters:
                    - name: filters
                      in: query
                      schema:
                        type: array
                        items:
                          type: object
                          properties:
                            field:
                              type: string
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines = CodegenInlines::for_resource_inlines(
            &graph,
            graph.operations().flat_map(|op| op.inlines()).collect(),
        );

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_items_query_filters_item {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetItemsQueryFiltersItem {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub field: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_items_query_filters_item::*;
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_finds_inline_types_within_maps() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: getItems
                  parameters:
                    - name: metadata
                      in: query
                      schema:
                        type: object
                        additionalProperties:
                          type: object
                          properties:
                            value:
                              type: string
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines = CodegenInlines::for_resource_inlines(
            &graph,
            graph.operations().flat_map(|op| op.inlines()).collect(),
        );

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_items_query_metadata_value {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetItemsQueryMetadataValue {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_items_query_metadata_value::*;
            }
        };
        assert_eq!(actual, expected);
    }
}
