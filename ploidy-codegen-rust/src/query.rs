use itertools::Itertools;
use ploidy_core::{
    codegen::UniqueNames,
    ir::{OperationView, ParameterStyle, ParameterView, QueryParameter, View},
};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, format_ident, quote};

use super::{
    derives::ExtraDerive,
    ext::ParameterViewExt,
    graph::CodegenGraph,
    naming::{CodegenIdent, CodegenIdentScope, CodegenIdentUsage},
    ref_::CodegenRef,
};

/// Generates a query parameter struct for an API operation.
///
/// The generated struct is named `{OperationId}Query`.
/// It bundles all query parameters for that operation,
/// derives `Serialize`, and has an associated `STYLES` table
/// with per-parameter serialization style overrides.
#[derive(Debug)]
pub struct CodegenQueryParameters<'a> {
    graph: &'a CodegenGraph<'a>,
    op: &'a OperationView<'a>,
}

impl<'a> CodegenQueryParameters<'a> {
    /// Creates a new query parameter struct for the given operation.
    #[inline]
    pub fn new(graph: &'a CodegenGraph<'a>, op: &'a OperationView<'a>) -> Self {
        Self { graph, op }
    }
}

impl ToTokens for CodegenQueryParameters<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let op_ident = CodegenIdent::new(self.op.id());
        let query_name = format_ident!("{}Query", CodegenIdentUsage::Type(&op_ident));

        let mut extra_derives = vec![];

        // Derive `Eq` and `Hash` if all parameter types, and their
        // transitively referenced types, are hashable.
        if self.op.query().all(|param| param.hashable()) {
            extra_derives.push(ExtraDerive::Eq);
            extra_derives.push(ExtraDerive::Hash);
        }

        // Derive `Default` if all required parameters, and their
        // transitively referenced types, are defaultable.
        // Optional parameters become `Option<T>`, which is `Default`.
        if self
            .op
            .query()
            .all(|param| !param.required() || param.defaultable())
        {
            extra_derives.push(ExtraDerive::Default);
        }

        let unique = UniqueNames::new();
        let mut scope = CodegenIdentScope::new(&unique);

        let params = self
            .op
            .query()
            .map(|param| (scope.uniquify(param.name()), param))
            .collect_vec();

        let fields = params.iter().map(|(ident, param)| {
            let field_name = CodegenIdentUsage::Field(ident);
            let serde_attr = SerdeQueryFieldAttr::new(field_name, param);

            let ty = if param.optional() {
                let view = param.ty();
                let path = CodegenRef::new(self.graph, &view);
                quote! { ::std::option::Option<#path> }
            } else {
                let view = param.ty();
                let path = CodegenRef::new(self.graph, &view);
                quote!(#path)
            };

            quote! {
                #serde_attr
                pub #field_name: #ty,
            }
        });

        let styles = params
            .iter()
            .filter_map(|(_, param)| Some((param.name(), param.style()?)))
            .map(|(name, style)| {
                let style = match style {
                    ParameterStyle::DeepObject => {
                        quote!(::ploidy_util::QueryStyle::DeepObject)
                    }
                    ParameterStyle::SpaceDelimited => {
                        quote!(::ploidy_util::QueryStyle::SpaceDelimited)
                    }
                    ParameterStyle::PipeDelimited => {
                        quote!(::ploidy_util::QueryStyle::PipeDelimited)
                    }
                    ParameterStyle::Form { exploded } => {
                        quote!(::ploidy_util::QueryStyle::Form { exploded: #exploded })
                    }
                };
                quote!((#name, #style))
            });

        tokens.append_all(quote! {
            #[derive(Debug, Clone, PartialEq, #(#extra_derives,)* ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
            #[serde(crate = "::ploidy_util::serde")]
            pub struct #query_name {
                #(#fields)*
            }

            impl #query_name {
                pub const STYLES: &[(&str, ::ploidy_util::QueryStyle)] = &[#(#styles,)*];
            }
        });
    }
}

/// Generates a `#[serde(...)]` attribute for a query parameter struct field.
#[derive(Debug)]
struct SerdeQueryFieldAttr<'param, 'a> {
    field_name: CodegenIdentUsage<'param>,
    param: &'param ParameterView<'param, 'a, QueryParameter>,
}

impl<'param, 'a> SerdeQueryFieldAttr<'param, 'a> {
    fn new(
        field_name: CodegenIdentUsage<'param>,
        param: &'param ParameterView<'param, 'a, QueryParameter>,
    ) -> Self {
        Self { field_name, param }
    }
}

impl ToTokens for SerdeQueryFieldAttr<'_, '_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut attrs = vec![];

        let param_name = self.param.name();
        if self.field_name.display().to_string() != param_name {
            attrs.push(quote! { rename = #param_name });
        }

        if self.param.optional() {
            attrs.push(quote! { default });
            attrs.push(quote! { skip_serializing_if = "Option::is_none" });
        }

        if !attrs.is_empty() {
            tokens.append_all(quote! { #[serde(#(#attrs),*)] });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        arena::Arena,
        ir::{RawGraph, Spec},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    use crate::CodegenGraph;

    #[test]
    fn test_all_optional_query_params() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /charts/{chart_id}:
                get:
                  operationId: fetchChart
                  parameters:
                    - name: chart_id
                      in: path
                      required: true
                      schema:
                        type: string
                    - name: refresh
                      in: query
                      schema:
                        type: boolean
                    - name: client_job_id
                      in: query
                      schema:
                        type: string
                    - name: partition_idx
                      in: query
                      schema:
                        type: integer
                        format: int32
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().next().unwrap();
        let codegen = CodegenQueryParameters::new(&graph, &op);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
            #[serde(crate = "::ploidy_util::serde")]
            pub struct FetchChartQuery {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub refresh: ::std::option::Option<bool>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub client_job_id: ::std::option::Option<::std::string::String>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub partition_idx: ::std::option::Option<i32>,
            }

            impl FetchChartQuery {
                pub const STYLES: &[(&str, ::ploidy_util::QueryStyle)] = &[];
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_required_and_optional_query_params() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: listItems
                  parameters:
                    - name: page
                      in: query
                      required: true
                      schema:
                        type: integer
                        format: int32
                    - name: perPage
                      in: query
                      style: pipeDelimited
                      schema:
                        type: array
                        items:
                          type: integer
                          format: int32
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().next().unwrap();
        let codegen = CodegenQueryParameters::new(&graph, &op);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
            #[serde(crate = "::ploidy_util::serde")]
            pub struct ListItemsQuery {
                pub page: i32,
                #[serde(rename = "perPage", default, skip_serializing_if = "Option::is_none")]
                pub per_page: ::std::option::Option<::std::vec::Vec<i32>>,
            }

            impl ListItemsQuery {
                pub const STYLES: &[(&str, ::ploidy_util::QueryStyle)] = &[
                    ("perPage", ::ploidy_util::QueryStyle::PipeDelimited),
                ];
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_excludes_eq_hash_for_float_params() {
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
                    - name: threshold
                      in: query
                      schema:
                        type: number
                        format: double
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().next().unwrap();
        let codegen = CodegenQueryParameters::new(&graph, &op);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
            #[serde(crate = "::ploidy_util::serde")]
            pub struct GetItemsQuery {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub threshold: ::std::option::Option<f64>,
            }

            impl GetItemsQuery {
                pub const STYLES: &[(&str, ::ploidy_util::QueryStyle)] = &[];
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_excludes_default_for_non_defaultable_required_param() {
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
                    - name: callback
                      in: query
                      required: true
                      schema:
                        type: string
                        format: uri
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().next().unwrap();
        let codegen = CodegenQueryParameters::new(&graph, &op);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
            #[serde(crate = "::ploidy_util::serde")]
            pub struct GetItemsQuery {
                pub callback: ::ploidy_util::url::Url,
            }

            impl GetItemsQuery {
                pub const STYLES: &[(&str, ::ploidy_util::QueryStyle)] = &[];
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_query_parameter_styles() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: listItems
                  parameters:
                    - name: filter
                      in: query
                      style: deepObject
                      schema:
                        type: object
                        properties:
                          status:
                            type: string
                    - name: tags
                      in: query
                      style: pipeDelimited
                      schema:
                        type: array
                        items:
                          type: string
                    - name: ids
                      in: query
                      style: spaceDelimited
                      schema:
                        type: array
                        items:
                          type: integer
                          format: int32
                    - name: colors
                      in: query
                      style: form
                      explode: false
                      schema:
                        type: array
                        items:
                          type: string
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().next().unwrap();
        let codegen = CodegenQueryParameters::new(&graph, &op);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
            #[serde(crate = "::ploidy_util::serde")]
            pub struct ListItemsQuery {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub filter: ::std::option::Option<crate::client::default::types::ListItemsFilter>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub tags: ::std::option::Option<::std::vec::Vec<::std::string::String>>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub ids: ::std::option::Option<::std::vec::Vec<i32>>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub colors: ::std::option::Option<::std::vec::Vec<::std::string::String>>,
            }

            impl ListItemsQuery {
                pub const STYLES: &[(&str, ::ploidy_util::QueryStyle)] = &[
                    ("filter", ::ploidy_util::QueryStyle::DeepObject),
                    ("tags", ::ploidy_util::QueryStyle::PipeDelimited),
                    ("ids", ::ploidy_util::QueryStyle::SpaceDelimited),
                    ("colors", ::ploidy_util::QueryStyle::Form { exploded: false }),
                ];
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_ref_query_parameter() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: listItems
                  parameters:
                    - name: sort
                      in: query
                      schema:
                        $ref: '#/components/schemas/SortOrder'
                  responses:
                    '200':
                      description: OK
            components:
              schemas:
                SortOrder:
                  type: string
                  enum:
                    - asc
                    - desc
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().next().unwrap();
        let codegen = CodegenQueryParameters::new(&graph, &op);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
            #[serde(crate = "::ploidy_util::serde")]
            pub struct ListItemsQuery {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub sort: ::std::option::Option<crate::types::SortOrder>,
            }

            impl ListItemsQuery {
                pub const STYLES: &[(&str, ::ploidy_util::QueryStyle)] = &[];
            }
        };
        assert_eq!(actual, expected);
    }
}
