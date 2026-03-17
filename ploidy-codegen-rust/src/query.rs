use itertools::Itertools;
use ploidy_core::{
    codegen::UniqueNames,
    ir::{ContainerView, OperationView, ParameterStyle},
};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, format_ident, quote};

use super::{
    derives::ExtraDerive,
    ext::ViewExt,
    naming::{CodegenIdent, CodegenIdentScope, CodegenIdentUsage},
    ref_::CodegenRef,
};

/// Generates a query parameter struct for an API operation.
///
/// The generated struct is named `{OperationId}Query`, and
/// bundles all query parameters for that operation. Its generated
/// `append_to` method appends each parameter to a `QuerySerializer`.
#[derive(Debug)]
pub struct CodegenQueryParameters<'a> {
    op: &'a OperationView<'a>,
}

impl<'a> CodegenQueryParameters<'a> {
    /// Creates a new query parameter struct for the given operation.
    #[inline]
    pub fn new(op: &'a OperationView<'a>) -> Self {
        Self { op }
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

        let fields = params
            .iter()
            .map(|(ident, param)| {
                let field_name = CodegenIdentUsage::Field(ident);

                let view = param.ty();
                let ty = if param.required()
                    || matches!(view.as_container(), Some(ContainerView::Optional(_)))
                {
                    let path = CodegenRef::new(&view);
                    quote!(#path)
                } else {
                    let path = CodegenRef::new(&view);
                    quote! { ::std::option::Option<#path> }
                };

                quote! {
                    pub #field_name: #ty,
                }
            })
            .collect_vec();

        let names = params.iter().map(|(_, param)| param.name());

        let styles = params.iter().map(|(_, param)| match param.style() {
            Some(ParameterStyle::DeepObject) => {
                quote!(::ploidy_util::QueryStyle::DeepObject)
            }
            Some(ParameterStyle::SpaceDelimited) => {
                quote!(::ploidy_util::QueryStyle::SpaceDelimited)
            }
            Some(ParameterStyle::PipeDelimited) => {
                quote!(::ploidy_util::QueryStyle::PipeDelimited)
            }
            Some(ParameterStyle::Form { exploded }) => {
                quote!(::ploidy_util::QueryStyle::Form { exploded: #exploded })
            }
            None => quote!(::ploidy_util::QueryStyle::Form { exploded: true }),
        });

        let field_idents = params
            .iter()
            .map(|(ident, _)| CodegenIdentUsage::Field(ident))
            .collect_vec();

        tokens.append_all(quote! {
            #[derive(Debug, Clone, PartialEq, #(#extra_derives),*)]
            pub struct #query_name {
                #(#fields)*
            }

            impl #query_name {
                /// Serializes and appends query parameters to the URL.
                pub fn append_to(
                    &self,
                    url: &mut ::ploidy_util::url::Url,
                ) -> ::std::result::Result<(), ::ploidy_util::QueryParamError> {
                    let mut serializer = ::ploidy_util::QuerySerializer::new(url);
                    serializer #(
                        .append(#names, &self.#field_idents, #styles)?
                    )*;
                    Ok(())
                }
            }
        });
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
        let codegen = CodegenQueryParameters::new(&op);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
            pub struct FetchChartQuery {
                pub refresh: ::std::option::Option<bool>,
                pub client_job_id: ::std::option::Option<::std::string::String>,
                pub partition_idx: ::std::option::Option<i32>,
            }

            impl FetchChartQuery {
                /// Serializes and appends query parameters to the URL.
                pub fn append_to(
                    &self,
                    url: &mut ::ploidy_util::url::Url,
                ) -> ::std::result::Result<(), ::ploidy_util::QueryParamError> {
                    let mut serializer = ::ploidy_util::QuerySerializer::new(url);
                    serializer
                        .append("refresh", &self.refresh, ::ploidy_util::QueryStyle::Form { exploded: true })?
                        .append("client_job_id", &self.client_job_id, ::ploidy_util::QueryStyle::Form { exploded: true })?
                        .append("partition_idx", &self.partition_idx, ::ploidy_util::QueryStyle::Form { exploded: true })?;
                    Ok(())
                }
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
        let codegen = CodegenQueryParameters::new(&op);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
            pub struct ListItemsQuery {
                pub page: i32,
                pub per_page: ::std::option::Option<i32>,
            }

            impl ListItemsQuery {
                /// Serializes and appends query parameters to the URL.
                pub fn append_to(
                    &self,
                    url: &mut ::ploidy_util::url::Url,
                ) -> ::std::result::Result<(), ::ploidy_util::QueryParamError> {
                    let mut serializer = ::ploidy_util::QuerySerializer::new(url);
                    serializer
                        .append("page", &self.page, ::ploidy_util::QueryStyle::Form { exploded: true })?
                        .append("perPage", &self.per_page, ::ploidy_util::QueryStyle::Form { exploded: true })?;
                    Ok(())
                }
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
        let codegen = CodegenQueryParameters::new(&op);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Default)]
            pub struct GetItemsQuery {
                pub threshold: ::std::option::Option<f64>,
            }

            impl GetItemsQuery {
                /// Serializes and appends query parameters to the URL.
                pub fn append_to(
                    &self,
                    url: &mut ::ploidy_util::url::Url,
                ) -> ::std::result::Result<(), ::ploidy_util::QueryParamError> {
                    let mut serializer = ::ploidy_util::QuerySerializer::new(url);
                    serializer
                        .append("threshold", &self.threshold, ::ploidy_util::QueryStyle::Form { exploded: true })?;
                    Ok(())
                }
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
        let codegen = CodegenQueryParameters::new(&op);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash)]
            pub struct GetItemsQuery {
                pub callback: ::ploidy_util::url::Url,
            }

            impl GetItemsQuery {
                /// Serializes and appends query parameters to the URL.
                pub fn append_to(
                    &self,
                    url: &mut ::ploidy_util::url::Url,
                ) -> ::std::result::Result<(), ::ploidy_util::QueryParamError> {
                    let mut serializer = ::ploidy_util::QuerySerializer::new(url);
                    serializer
                        .append("callback", &self.callback, ::ploidy_util::QueryStyle::Form { exploded: true })?;
                    Ok(())
                }
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
        let codegen = CodegenQueryParameters::new(&op);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
            pub struct ListItemsQuery {
                pub filter: ::std::option::Option<crate::client::default::types::ListItemsFilter>,
                pub tags: ::std::option::Option<::std::vec::Vec<::std::string::String>>,
                pub ids: ::std::option::Option<::std::vec::Vec<i32>>,
                pub colors: ::std::option::Option<::std::vec::Vec<::std::string::String>>,
            }

            impl ListItemsQuery {
                /// Serializes and appends query parameters to the URL.
                pub fn append_to(
                    &self,
                    url: &mut ::ploidy_util::url::Url,
                ) -> ::std::result::Result<(), ::ploidy_util::QueryParamError> {
                    let mut serializer = ::ploidy_util::QuerySerializer::new(url);
                    serializer
                        .append("filter", &self.filter, ::ploidy_util::QueryStyle::DeepObject)?
                        .append("tags", &self.tags, ::ploidy_util::QueryStyle::PipeDelimited)?
                        .append("ids", &self.ids, ::ploidy_util::QueryStyle::SpaceDelimited)?
                        .append("colors", &self.colors, ::ploidy_util::QueryStyle::Form { exploded: false })?;
                    Ok(())
                }
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
        let codegen = CodegenQueryParameters::new(&op);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
            pub struct ListItemsQuery {
                pub sort: ::std::option::Option<crate::types::SortOrder>,
            }

            impl ListItemsQuery {
                /// Serializes and appends query parameters to the URL.
                pub fn append_to(
                    &self,
                    url: &mut ::ploidy_util::url::Url,
                ) -> ::std::result::Result<(), ::ploidy_util::QueryParamError> {
                    let mut serializer = ::ploidy_util::QuerySerializer::new(url);
                    serializer
                        .append("sort", &self.sort, ::ploidy_util::QueryStyle::Form { exploded: true })?;
                    Ok(())
                }
            }
        };
        assert_eq!(actual, expected);
    }
}
