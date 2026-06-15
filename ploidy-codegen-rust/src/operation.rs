use itertools::Itertools;
use ploidy_core::{
    ir::{OperationView, RequestView, ResponseView},
    parse::{
        Method,
        path::{PathFragment, PathRun},
    },
};
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, format_ident, quote};
use syn::Ident;

use super::{
    doc_attrs,
    graph::{CodegenGraph, IdentMapping},
    naming::CodegenIdentUsage,
    ref_::CodegenRef,
};

/// Generates a single client method for an API operation.
pub struct CodegenOperation<'a> {
    graph: &'a CodegenGraph<'a>,
    op: &'a OperationView<'a, 'a>,
}

impl<'a> CodegenOperation<'a> {
    pub fn new(graph: &'a CodegenGraph<'a>, op: &'a OperationView<'a, 'a>) -> Self {
        Self { graph, op }
    }

    /// Generates code to build and interpolate path and query parameters
    /// into the request URL.
    fn url(&self) -> TokenStream {
        // Path parameters and literal segments from the path template.
        let segments = self.op.path().runs().map(|run| match run {
            PathRun::Literals(literals) => match &*literals {
                [one] => quote! { .push(#one) },
                many => quote! { .extend(&[#(#many),*]) },
            },
            PathRun::Templated([PathFragment::Param(name)]) => {
                let param = CodegenIdentUsage::Param(
                    self.graph.ident(IdentMapping::Path(self.op.id(), name)),
                );
                quote! { .push(#param) }
            }
            PathRun::Templated(fragments) => {
                // Build a format string, with placeholders for parameter fragments.
                let format = fragments.iter().fold(String::new(), |mut f, fragment| {
                    match fragment {
                        PathFragment::Literal(text) => {
                            f.push_str(&text.replace('{', "{{").replace('}', "}}"))
                        }
                        PathFragment::Param(_) => f.push_str("{}"),
                    }
                    f
                });
                let args = fragments
                    .iter()
                    .filter_map(|fragment| match fragment {
                        PathFragment::Param(name) => Some(name),
                        PathFragment::Literal(_) => None,
                    })
                    .map(|name| {
                        // `url::PathSegmentsMut::push` percent-encodes the
                        // full segment, so we can interpolate fragments
                        // directly.
                        let param = CodegenIdentUsage::Param(
                            self.graph.ident(IdentMapping::Path(self.op.id(), name)),
                        );
                        quote!(#param)
                    });
                quote! { .push(&format!(#format, #(#args),*)) }
            }
        });

        // Literal query pairs from the path template.
        let pairs = self
            .op
            .path()
            .query()
            .map(|param| {
                let name = param.name;
                let value = param.value;
                quote! { .append_pair(#name, #value) }
            })
            .reduce(|a, b| quote!(#a #b))
            .map(|pairs| {
                quote! {
                    url.query_pairs_mut()
                        #pairs;
                }
            });

        // Operation query parameters.
        let query = self.op.query().next().is_some().then(|| {
            let query_name = format_ident!(
                "{}Query",
                CodegenIdentUsage::Type(self.graph.ident(self.op.id()))
            );
            quote! {
                let url = ::ploidy_util::serde::Serialize::serialize(
                    query,
                    ::ploidy_util::QuerySerializer::new(
                        url,
                        parameters::#query_name::STYLES,
                    ),
                )?;
            }
        });

        quote! {
            let url = {
                let mut url = self.base_url.clone();
                url.path_segments_mut()
                    .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                    .pop_if_empty()
                    #(#segments)*;
                #pairs
                #query
                #[cfg(feature = "tracing")]
                {
                    ::tracing::record_all!(::tracing::Span::current(),
                        server.address = url.host_str(),
                        server.port = url.port_or_known_default(),
                        // We intentionally include the full URL,
                        // without redaction.
                        url.full = url.as_str(),
                    );
                }
                url
            };
        }
    }
}

impl ToTokens for CodegenOperation<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut params = vec![];

        let paths = self.op.path().params().collect_vec();
        for param in &paths {
            let param = CodegenIdentUsage::Param(
                self.graph
                    .ident(IdentMapping::Path(self.op.id(), param.name())),
            );
            params.push(quote! { #param: &str });
        }

        if self.op.query().next().is_some() {
            // Include the `query` argument if we have
            // at least one query parameter.
            let query_type_name = format_ident!(
                "{}Query",
                CodegenIdentUsage::Type(self.graph.ident(self.op.id()))
            );
            params.push(quote! { query: &parameters::#query_type_name });
        }

        if let Some(request) = self.op.request() {
            match request {
                RequestView::Json(view) => {
                    let param_type = CodegenRef::new(self.graph, &view);
                    params.push(quote! { request: impl Into<#param_type> });
                }
                RequestView::Multipart => {
                    params.push(quote! { form: crate::util::reqwest::multipart::Form });
                }
            }
        }

        let return_type = match self.op.response() {
            Some(response) => match response {
                ResponseView::Json(view) => CodegenRef::new(self.graph, &view).into_token_stream(),
            },
            None => quote! { () },
        };

        let url = self.url();

        let request = {
            let method = CodegenMethod(self.op.method());
            let builder = match self.op.request() {
                Some(RequestView::Json(_)) => quote! {
                    let request = self.client
                        .#method(url)
                        .headers(self.headers.clone())
                        .json(&request.into());
                },
                Some(RequestView::Multipart) => quote! {
                    let request = self.client
                        .#method(url)
                        .headers(self.headers.clone())
                        .multipart(form);
                },
                None => quote! {
                    let request = self.client
                        .#method(url)
                        .headers(self.headers.clone());
                },
            };
            quote! {
                let request = {
                    #builder
                    #[cfg(feature = "trace-context")]
                    let request = ::ploidy_util::trace::propagate(
                        ::tracing::Span::current(),
                        request,
                    );
                    request
                };
                let response = request.send().await?;
                #[cfg(feature = "tracing")]
                {
                    ::tracing::record_all!(::tracing::Span::current(),
                        http.response.status_code = response.status().as_u16()
                    );
                }
                let response = response.error_for_status()?;
            }
        };

        let response = if self.op.response().is_some() {
            quote! {
                let body = response.bytes().await?;
                let deserializer = &mut ::ploidy_util::serde_json::Deserializer::from_slice(&body);
                let result = ::ploidy_util::serde_path_to_error::deserialize(deserializer)?;
                Ok(result)
            }
        } else {
            quote! {
                let _ = response;
                Ok(())
            }
        };

        let method_name = CodegenIdentUsage::Method(self.graph.ident(self.op.id()));

        let instrument = {
            let name = format!("{} {}", self.op.method().as_str(), self.op.path());
            let template = self.op.path().to_string();
            let method = self.op.method().as_str();
            let mut fields = vec![
                quote!(otel.name = #name),
                quote!(otel.kind = "client"),
                quote!(url.template = #template),
                quote!(http.request.method = #method),
                quote!(server.address, server.port, url.full, http.response.status_code, error.type),
            ];
            fields.extend(paths.iter().map(|param| {
                let param = CodegenIdentUsage::Param(
                    self.graph
                        .ident(IdentMapping::Path(self.op.id(), param.name())),
                );
                quote!(#param = %#param)
            }));
            quote! {
                #[cfg_attr(feature = "tracing", ::tracing::instrument(
                    skip_all,
                    fields(#(#fields),*)
                ))]
            }
        };

        let doc = {
            let url = format!(" {} {}", self.op.method().as_str(), self.op.path());
            match self.op.description() {
                Some(description) => {
                    let attrs = doc_attrs(description);
                    quote! {
                        #attrs
                        #[doc = ""]
                        #[doc = #url]
                    }
                }
                None => {
                    quote!(#[doc = #url])
                }
            }
        };

        tokens.append_all(quote! {
            #doc
            #instrument
            pub async fn #method_name(
                &self,
                #(#params),*
            ) -> Result<#return_type, crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    #url
                    #request
                    #response
                }.await;
                #[cfg(feature = "tracing")]
                if let Err(err) = &result {
                    ::tracing::record_all!(::tracing::Span::current(),
                        error.type = %err.category(),
                    );
                }
                result
            }
        });
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodegenMethod(pub Method);

impl ToTokens for CodegenMethod {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(match self.0 {
            Method::Get => Ident::new("get", Span::call_site()),
            Method::Post => Ident::new("post", Span::call_site()),
            Method::Put => Ident::new("put", Span::call_site()),
            Method::Patch => Ident::new("patch", Span::call_site()),
            Method::Delete => Ident::new("delete", Span::call_site()),
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

    // MARK: With query params

    #[test]
    fn test_operation_with_path_and_query_params() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items/{item_id}:
                get:
                  operationId: getItem
                  description: Gets an item.
                  parameters:
                    - name: item_id
                      in: path
                      required: true
                      schema:
                        type: string
                    - name: expand
                      in: query
                      schema:
                        type: boolean
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().next().unwrap();
        let codegen = CodegenOperation::new(&graph, &op);

        let actual: syn::ImplItemFn = parse_quote!(#codegen);
        let expected: syn::ImplItemFn = parse_quote! {
            #[doc = " Gets an item."]
            #[doc = ""]
            #[doc = " GET /items/{item_id}"]
            #[cfg_attr(
                feature = "tracing",
                ::tracing::instrument(
                    skip_all,
                    fields(
                        otel.name = "GET /items/{item_id}",
                        otel.kind = "client",
                        url.template = "/items/{item_id}",
                        http.request.method = "GET",
                        server.address,
                        server.port,
                        url.full,
                        http.response.status_code,
                        error.type,
                        item_id = %item_id
                    )
                )
            )]
            pub async fn get_item(
                &self,
                item_id: &str,
                query: &parameters::GetItemQuery
            ) -> Result<(), crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    let url = {
                        let mut url = self.base_url.clone();
                        url.path_segments_mut()
                            .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                            .pop_if_empty()
                            .push("items")
                            .push(item_id);
                        let url = ::ploidy_util::serde::Serialize::serialize(
                            query,
                            ::ploidy_util::QuerySerializer::new(
                                url,
                                parameters::GetItemQuery::STYLES,
                            ),
                        )?;
                        #[cfg(feature = "tracing")]
                        {
                            ::tracing::record_all!(::tracing::Span::current(),
                                server.address = url.host_str(),
                                server.port = url.port_or_known_default(),
                                url.full = url.as_str(),
                            );
                        }
                        url
                    };
                    let request = {
                        let request = self
                            .client
                            .get(url)
                            .headers(self.headers.clone());
                        #[cfg(feature = "trace-context")]
                        let request = ::ploidy_util::trace::propagate(
                            ::tracing::Span::current(),
                            request,
                        );
                        request
                    };
                    let response = request
                        .send()
                        .await?;
                    #[cfg(feature = "tracing")]
                    {
                        ::tracing::record_all!(::tracing::Span::current(),
                            http.response.status_code = response.status().as_u16()
                        );
                    }
                    let response = response.error_for_status()?;
                    let _ = response;
                    Ok(())
                }.await;
                #[cfg(feature = "tracing")]
                if let Err(err) = &result {
                    ::tracing::record_all!(::tracing::Span::current(),
                        error.type = %err.category(),
                    );
                }
                result
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_operation_with_query_params_only() {
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
        let codegen = CodegenOperation::new(&graph, &op);

        let actual: syn::ImplItemFn = parse_quote!(#codegen);
        let expected: syn::ImplItemFn = parse_quote! {
            #[doc = " GET /items"]
            #[cfg_attr(
                feature = "tracing",
                ::tracing::instrument(
                    skip_all,
                    fields(
                        otel.name = "GET /items",
                        otel.kind = "client",
                        url.template = "/items",
                        http.request.method = "GET",
                        server.address,
                        server.port,
                        url.full,
                        http.response.status_code,
                        error.type
                    )
                )
            )]
            pub async fn get_items(
                &self,
                query: &parameters::GetItemsQuery
            ) -> Result<(), crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    let url = {
                        let mut url = self.base_url.clone();
                        url.path_segments_mut()
                            .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                            .pop_if_empty()
                            .push("items");
                        let url = ::ploidy_util::serde::Serialize::serialize(
                            query,
                            ::ploidy_util::QuerySerializer::new(
                                url,
                                parameters::GetItemsQuery::STYLES,
                            ),
                        )?;
                        #[cfg(feature = "tracing")]
                        {
                            ::tracing::record_all!(::tracing::Span::current(),
                                server.address = url.host_str(),
                                server.port = url.port_or_known_default(),
                                url.full = url.as_str(),
                            );
                        }
                        url
                    };
                    let request = {
                        let request = self
                            .client
                            .get(url)
                            .headers(self.headers.clone());
                        #[cfg(feature = "trace-context")]
                        let request = ::ploidy_util::trace::propagate(
                            ::tracing::Span::current(),
                            request,
                        );
                        request
                    };
                    let response = request
                        .send()
                        .await?;
                    #[cfg(feature = "tracing")]
                    {
                        ::tracing::record_all!(::tracing::Span::current(),
                            http.response.status_code = response.status().as_u16()
                        );
                    }
                    let response = response.error_for_status()?;
                    let _ = response;
                    Ok(())
                }.await;
                #[cfg(feature = "tracing")]
                if let Err(err) = &result {
                    ::tracing::record_all!(::tracing::Span::current(),
                        error.type = %err.category(),
                    );
                }
                result
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_path_param_named_query_does_not_shadow() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /search/{query}:
                get:
                  operationId: search
                  parameters:
                    - name: query
                      in: path
                      required: true
                      schema:
                        type: string
                    - name: limit
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
        let codegen = CodegenOperation::new(&graph, &op);

        let actual: syn::ImplItemFn = parse_quote!(#codegen);
        let expected: syn::ImplItemFn = parse_quote! {
            #[doc = " GET /search/{query}"]
            #[cfg_attr(
                feature = "tracing",
                ::tracing::instrument(
                    skip_all,
                    fields(
                        otel.name = "GET /search/{query}",
                        otel.kind = "client",
                        url.template = "/search/{query}",
                        http.request.method = "GET",
                        server.address,
                        server.port,
                        url.full,
                        http.response.status_code,
                        error.type,
                        query_2 = %query_2
                    )
                )
            )]
            pub async fn search(
                &self,
                query_2: &str,
                query: &parameters::SearchQuery
            ) -> Result<(), crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    let url = {
                        let mut url = self.base_url.clone();
                        url.path_segments_mut()
                            .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                            .pop_if_empty()
                            .push("search")
                            .push(query_2);
                        let url = ::ploidy_util::serde::Serialize::serialize(
                            query,
                            ::ploidy_util::QuerySerializer::new(
                                url,
                                parameters::SearchQuery::STYLES,
                            ),
                        )?;
                        #[cfg(feature = "tracing")]
                        {
                            ::tracing::record_all!(::tracing::Span::current(),
                                server.address = url.host_str(),
                                server.port = url.port_or_known_default(),
                                url.full = url.as_str(),
                            );
                        }
                        url
                    };
                    let request = {
                        let request = self
                            .client
                            .get(url)
                            .headers(self.headers.clone());
                        #[cfg(feature = "trace-context")]
                        let request = ::ploidy_util::trace::propagate(
                            ::tracing::Span::current(),
                            request,
                        );
                        request
                    };
                    let response = request
                        .send()
                        .await?;
                    #[cfg(feature = "tracing")]
                    {
                        ::tracing::record_all!(::tracing::Span::current(),
                            http.response.status_code = response.status().as_u16()
                        );
                    }
                    let response = response.error_for_status()?;
                    let _ = response;
                    Ok(())
                }.await;
                #[cfg(feature = "tracing")]
                if let Err(err) = &result {
                    ::tracing::record_all!(::tracing::Span::current(),
                        error.type = %err.category(),
                    );
                }
                result
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: With query params and request body

    #[test]
    fn test_operation_with_query_params_and_request_body() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items/{item_id}:
                put:
                  operationId: updateItem
                  parameters:
                    - name: item_id
                      in: path
                      required: true
                      schema:
                        type: string
                    - name: dry_run
                      in: query
                      schema:
                        type: boolean
                  requestBody:
                    content:
                      application/json:
                        schema:
                          $ref: '#/components/schemas/Item'
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
                    name:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().next().unwrap();
        let codegen = CodegenOperation::new(&graph, &op);

        let actual: syn::ImplItemFn = parse_quote!(#codegen);
        let expected: syn::ImplItemFn = parse_quote! {
            #[doc = " PUT /items/{item_id}"]
            #[cfg_attr(
                feature = "tracing",
                ::tracing::instrument(
                    skip_all,
                    fields(
                        otel.name = "PUT /items/{item_id}",
                        otel.kind = "client",
                        url.template = "/items/{item_id}",
                        http.request.method = "PUT",
                        server.address,
                        server.port,
                        url.full,
                        http.response.status_code,
                        error.type,
                        item_id = %item_id
                    )
                )
            )]
            pub async fn update_item(
                &self,
                item_id: &str,
                query: &parameters::UpdateItemQuery,
                request: impl Into<crate::types::Item>
            ) -> Result<crate::types::Item, crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    let url = {
                        let mut url = self.base_url.clone();
                        url.path_segments_mut()
                            .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                            .pop_if_empty()
                            .push("items")
                            .push(item_id);
                        let url = ::ploidy_util::serde::Serialize::serialize(
                            query,
                            ::ploidy_util::QuerySerializer::new(
                                url,
                                parameters::UpdateItemQuery::STYLES,
                            ),
                        )?;
                        #[cfg(feature = "tracing")]
                        {
                            ::tracing::record_all!(::tracing::Span::current(),
                                server.address = url.host_str(),
                                server.port = url.port_or_known_default(),
                                url.full = url.as_str(),
                            );
                        }
                        url
                    };
                    let request = {
                        let request = self
                            .client
                            .put(url)
                            .headers(self.headers.clone())
                            .json(&request.into());
                        #[cfg(feature = "trace-context")]
                        let request = ::ploidy_util::trace::propagate(
                            ::tracing::Span::current(),
                            request,
                        );
                        request
                    };
                    let response = request
                        .send()
                        .await?;
                    #[cfg(feature = "tracing")]
                    {
                        ::tracing::record_all!(::tracing::Span::current(),
                            http.response.status_code = response.status().as_u16()
                        );
                    }
                    let response = response.error_for_status()?;
                    let body = response.bytes().await?;
                    let deserializer = &mut ::ploidy_util::serde_json::Deserializer::from_slice(&body);
                    let result = ::ploidy_util::serde_path_to_error::deserialize(deserializer)?;
                    Ok(result)
                }.await;
                #[cfg(feature = "tracing")]
                if let Err(err) = &result {
                    ::tracing::record_all!(::tracing::Span::current(),
                        error.type = %err.category(),
                    );
                }
                result
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: Without query params

    #[test]
    fn test_operation_without_query_params() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items/{item_id}:
                get:
                  operationId: getItem
                  parameters:
                    - name: item_id
                      in: path
                      required: true
                      schema:
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
        let codegen = CodegenOperation::new(&graph, &op);

        let actual: syn::ImplItemFn = parse_quote!(#codegen);
        let expected: syn::ImplItemFn = parse_quote! {
            #[doc = " GET /items/{item_id}"]
            #[cfg_attr(
                feature = "tracing",
                ::tracing::instrument(
                    skip_all,
                    fields(
                        otel.name = "GET /items/{item_id}",
                        otel.kind = "client",
                        url.template = "/items/{item_id}",
                        http.request.method = "GET",
                        server.address,
                        server.port,
                        url.full,
                        http.response.status_code,
                        error.type,
                        item_id = %item_id
                    )
                )
            )]
            pub async fn get_item(
                &self,
                item_id: &str
            ) -> Result<(), crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    let url = {
                        let mut url = self.base_url.clone();
                        url.path_segments_mut()
                            .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                            .pop_if_empty()
                            .push("items")
                            .push(item_id);
                        #[cfg(feature = "tracing")]
                        {
                            ::tracing::record_all!(::tracing::Span::current(),
                                server.address = url.host_str(),
                                server.port = url.port_or_known_default(),
                                url.full = url.as_str(),
                            );
                        }
                        url
                    };
                    let request = {
                        let request = self
                            .client
                            .get(url)
                            .headers(self.headers.clone());
                        #[cfg(feature = "trace-context")]
                        let request = ::ploidy_util::trace::propagate(
                            ::tracing::Span::current(),
                            request,
                        );
                        request
                    };
                    let response = request
                        .send()
                        .await?;
                    #[cfg(feature = "tracing")]
                    {
                        ::tracing::record_all!(::tracing::Span::current(),
                            http.response.status_code = response.status().as_u16()
                        );
                    }
                    let response = response.error_for_status()?;
                    let _ = response;
                    Ok(())
                }.await;
                #[cfg(feature = "tracing")]
                if let Err(err) = &result {
                    ::tracing::record_all!(::tracing::Span::current(),
                        error.type = %err.category(),
                    );
                }
                result
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: Synthesized path params

    #[test]
    fn test_operation_with_synthesized_path_param() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items/{item_id}:
                get:
                  operationId: getItem
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().next().unwrap();
        let codegen = CodegenOperation::new(&graph, &op);

        let actual: syn::ImplItemFn = parse_quote!(#codegen);
        let expected: syn::ImplItemFn = parse_quote! {
            #[doc = " GET /items/{item_id}"]
            #[cfg_attr(
                feature = "tracing",
                ::tracing::instrument(
                    skip_all,
                    fields(
                        otel.name = "GET /items/{item_id}",
                        otel.kind = "client",
                        url.template = "/items/{item_id}",
                        http.request.method = "GET",
                        server.address,
                        server.port,
                        url.full,
                        http.response.status_code,
                        error.type,
                        item_id = %item_id
                    )
                )
            )]
            pub async fn get_item(
                &self,
                item_id: &str
            ) -> Result<(), crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    let url = {
                        let mut url = self.base_url.clone();
                        url.path_segments_mut()
                            .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                            .pop_if_empty()
                            .push("items")
                            .push(item_id);
                        #[cfg(feature = "tracing")]
                        {
                            ::tracing::record_all!(::tracing::Span::current(),
                                server.address = url.host_str(),
                                server.port = url.port_or_known_default(),
                                url.full = url.as_str(),
                            );
                        }
                        url
                    };
                    let request = {
                        let request = self
                            .client
                            .get(url)
                            .headers(self.headers.clone());
                        #[cfg(feature = "trace-context")]
                        let request = ::ploidy_util::trace::propagate(
                            ::tracing::Span::current(),
                            request,
                        );
                        request
                    };
                    let response = request
                        .send()
                        .await?;
                    #[cfg(feature = "tracing")]
                    {
                        ::tracing::record_all!(::tracing::Span::current(),
                            http.response.status_code = response.status().as_u16()
                        );
                    }
                    let response = response.error_for_status()?;
                    let _ = response;
                    Ok(())
                }.await;
                #[cfg(feature = "tracing")]
                if let Err(err) = &result {
                    ::tracing::record_all!(::tracing::Span::current(),
                        error.type = %err.category(),
                    );
                }
                result
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: Literal query params in path

    #[test]
    fn test_operation_with_literal_query_params() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /v1/messages?beta=true&expand:
                post:
                  operationId: betaCreateMessage
                  requestBody:
                    content:
                      application/json:
                        schema:
                          $ref: '#/components/schemas/Message'
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Message'
            components:
              schemas:
                Message:
                  type: object
                  properties:
                    content:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().next().unwrap();
        let codegen = CodegenOperation::new(&graph, &op);

        let actual: syn::ImplItemFn = parse_quote!(#codegen);
        let expected: syn::ImplItemFn = parse_quote! {
            #[doc = " POST /v1/messages?beta=true&expand="]
            #[cfg_attr(
                feature = "tracing",
                ::tracing::instrument(
                    skip_all,
                    fields(
                        otel.name = "POST /v1/messages?beta=true&expand=",
                        otel.kind = "client",
                        url.template = "/v1/messages?beta=true&expand=",
                        http.request.method = "POST",
                        server.address,
                        server.port,
                        url.full,
                        http.response.status_code,
                        error.type
                    )
                )
            )]
            pub async fn beta_create_message(
                &self,
                request: impl Into<crate::types::Message>
            ) -> Result<crate::types::Message, crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    let url = {
                        let mut url = self.base_url.clone();
                        url.path_segments_mut()
                            .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                            .pop_if_empty()
                            .extend(&["v1", "messages"]);
                        url.query_pairs_mut()
                            .append_pair("beta", "true")
                            .append_pair("expand", "");
                        #[cfg(feature = "tracing")]
                        {
                            ::tracing::record_all!(::tracing::Span::current(),
                                server.address = url.host_str(),
                                server.port = url.port_or_known_default(),
                                url.full = url.as_str(),
                            );
                        }
                        url
                    };
                    let request = {
                        let request = self
                            .client
                            .post(url)
                            .headers(self.headers.clone())
                            .json(&request.into());
                        #[cfg(feature = "trace-context")]
                        let request = ::ploidy_util::trace::propagate(
                            ::tracing::Span::current(),
                            request,
                        );
                        request
                    };
                    let response = request
                        .send()
                        .await?;
                    #[cfg(feature = "tracing")]
                    {
                        ::tracing::record_all!(::tracing::Span::current(),
                            http.response.status_code = response.status().as_u16()
                        );
                    }
                    let response = response.error_for_status()?;
                    let body = response.bytes().await?;
                    let deserializer = &mut ::ploidy_util::serde_json::Deserializer::from_slice(&body);
                    let result = ::ploidy_util::serde_path_to_error::deserialize(deserializer)?;
                    Ok(result)
                }.await;
                #[cfg(feature = "tracing")]
                if let Err(err) = &result {
                    ::tracing::record_all!(::tracing::Span::current(),
                        error.type = %err.category(),
                    );
                }
                result
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_operation_with_literal_and_declared_query_params() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /v1/messages?beta=true:
                post:
                  operationId: betaCreateMessage
                  parameters:
                    - name: limit
                      in: query
                      schema:
                        type: integer
                        format: int32
                  requestBody:
                    content:
                      application/json:
                        schema:
                          $ref: '#/components/schemas/Message'
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Message'
            components:
              schemas:
                Message:
                  type: object
                  properties:
                    content:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().next().unwrap();
        let codegen = CodegenOperation::new(&graph, &op);

        let actual: syn::ImplItemFn = parse_quote!(#codegen);
        let expected: syn::ImplItemFn = parse_quote! {
            #[doc = " POST /v1/messages?beta=true"]
            #[cfg_attr(
                feature = "tracing",
                ::tracing::instrument(
                    skip_all,
                    fields(
                        otel.name = "POST /v1/messages?beta=true",
                        otel.kind = "client",
                        url.template = "/v1/messages?beta=true",
                        http.request.method = "POST",
                        server.address,
                        server.port,
                        url.full,
                        http.response.status_code,
                        error.type
                    )
                )
            )]
            pub async fn beta_create_message(
                &self,
                query: &parameters::BetaCreateMessageQuery,
                request: impl Into<crate::types::Message>
            ) -> Result<crate::types::Message, crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    let url = {
                        let mut url = self.base_url.clone();
                        url.path_segments_mut()
                            .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                            .pop_if_empty()
                            .extend(&["v1", "messages"]);
                        url.query_pairs_mut()
                            .append_pair("beta", "true");
                        let url = ::ploidy_util::serde::Serialize::serialize(
                            query,
                            ::ploidy_util::QuerySerializer::new(
                                url,
                                parameters::BetaCreateMessageQuery::STYLES,
                            ),
                        )?;
                        #[cfg(feature = "tracing")]
                        {
                            ::tracing::record_all!(::tracing::Span::current(),
                                server.address = url.host_str(),
                                server.port = url.port_or_known_default(),
                                url.full = url.as_str(),
                            );
                        }
                        url
                    };
                    let request = {
                        let request = self
                            .client
                            .post(url)
                            .headers(self.headers.clone())
                            .json(&request.into());
                        #[cfg(feature = "trace-context")]
                        let request = ::ploidy_util::trace::propagate(
                            ::tracing::Span::current(),
                            request,
                        );
                        request
                    };
                    let response = request
                        .send()
                        .await?;
                    #[cfg(feature = "tracing")]
                    {
                        ::tracing::record_all!(::tracing::Span::current(),
                            http.response.status_code = response.status().as_u16()
                        );
                    }
                    let response = response.error_for_status()?;
                    let body = response.bytes().await?;
                    let deserializer = &mut ::ploidy_util::serde_json::Deserializer::from_slice(&body);
                    let result = ::ploidy_util::serde_path_to_error::deserialize(deserializer)?;
                    Ok(result)
                }.await;
                #[cfg(feature = "tracing")]
                if let Err(err) = &result {
                    ::tracing::record_all!(::tracing::Span::current(),
                        error.type = %err.category(),
                    );
                }
                result
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_operation_with_path_params_and_literal_and_declared_query_params() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /v1/models/{model_id}?beta=true:
                get:
                  operationId: betaGetModel
                  parameters:
                    - name: model_id
                      in: path
                      required: true
                      schema:
                        type: string
                    - name: expand
                      in: query
                      schema:
                        type: boolean
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Model'
            components:
              schemas:
                Model:
                  type: object
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().next().unwrap();
        let codegen = CodegenOperation::new(&graph, &op);

        let actual: syn::ImplItemFn = parse_quote!(#codegen);
        let expected: syn::ImplItemFn = parse_quote! {
            #[doc = " GET /v1/models/{model_id}?beta=true"]
            #[cfg_attr(
                feature = "tracing",
                ::tracing::instrument(
                    skip_all,
                    fields(
                        otel.name = "GET /v1/models/{model_id}?beta=true",
                        otel.kind = "client",
                        url.template = "/v1/models/{model_id}?beta=true",
                        http.request.method = "GET",
                        server.address,
                        server.port,
                        url.full,
                        http.response.status_code,
                        error.type,
                        model_id = %model_id
                    )
                )
            )]
            pub async fn beta_get_model(
                &self,
                model_id: &str,
                query: &parameters::BetaGetModelQuery
            ) -> Result<crate::types::Model, crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    let url = {
                        let mut url = self.base_url.clone();
                        url.path_segments_mut()
                            .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                            .pop_if_empty()
                            .extend(&["v1", "models"])
                            .push(model_id);
                        url.query_pairs_mut()
                            .append_pair("beta", "true");
                        let url = ::ploidy_util::serde::Serialize::serialize(
                            query,
                            ::ploidy_util::QuerySerializer::new(
                                url,
                                parameters::BetaGetModelQuery::STYLES,
                            ),
                        )?;
                        #[cfg(feature = "tracing")]
                        {
                            ::tracing::record_all!(::tracing::Span::current(),
                                server.address = url.host_str(),
                                server.port = url.port_or_known_default(),
                                url.full = url.as_str(),
                            );
                        }
                        url
                    };
                    let request = {
                        let request = self
                            .client
                            .get(url)
                            .headers(self.headers.clone());
                        #[cfg(feature = "trace-context")]
                        let request = ::ploidy_util::trace::propagate(
                            ::tracing::Span::current(),
                            request,
                        );
                        request
                    };
                    let response = request
                        .send()
                        .await?;
                    #[cfg(feature = "tracing")]
                    {
                        ::tracing::record_all!(::tracing::Span::current(),
                            http.response.status_code = response.status().as_u16()
                        );
                    }
                    let response = response.error_for_status()?;
                    let body = response.bytes().await?;
                    let deserializer = &mut ::ploidy_util::serde_json::Deserializer::from_slice(&body);
                    let result = ::ploidy_util::serde_path_to_error::deserialize(deserializer)?;
                    Ok(result)
                }.await;
                #[cfg(feature = "tracing")]
                if let Err(err) = &result {
                    ::tracing::record_all!(::tracing::Span::current(),
                        error.type = %err.category(),
                    );
                }
                result
            }
        };
        assert_eq!(actual, expected);
    }
}
