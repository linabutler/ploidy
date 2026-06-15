use ploidy_core::{
    codegen::IntoCode,
    ir::{OperationView, View},
};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, format_ident, quote};

use super::{
    cfg::CfgFeature,
    graph::CodegenGraph,
    inlines::CodegenInlines,
    naming::{CodegenIdentUsage, ResourceGroup},
    operation::CodegenOperation,
    query::CodegenQueryParameters,
};

/// Generates an `impl Client` block for a feature-gated resource,
/// with all its operations and inline types.
pub struct CodegenResource<'a> {
    graph: &'a CodegenGraph<'a>,
    resource: ResourceGroup<'a>,
    ops: &'a [OperationView<'a, 'a>],
}

impl<'a> CodegenResource<'a> {
    pub fn new(
        graph: &'a CodegenGraph<'a>,
        resource: ResourceGroup<'a>,
        ops: &'a [OperationView<'a, 'a>],
    ) -> Self {
        Self {
            graph,
            resource,
            ops,
        }
    }
}

impl ToTokens for CodegenResource<'_> {
    #[allow(
        clippy::filter_map_bool_then,
        reason = "`filter_map` + `then` reads cleaner here"
    )]
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let methods = self.ops.iter().map(|op| {
            // Each method gets its own `#[cfg(...)]` attribute.
            let cfg = CfgFeature::for_operation(self.graph, op);
            let method = CodegenOperation::new(self.graph, op);
            quote! {
                #cfg
                #method
            }
        });

        let inlines = CodegenInlines::for_resource_inlines(
            self.graph,
            self.ops.iter().flat_map(|op| op.inlines()).collect(),
        );

        let params = self
            .ops
            .iter()
            .filter_map(|op| {
                // Collect query parameter structs for operations
                // that have at least one query parameter.
                op.query().next().is_some().then(|| {
                    let cfg = CfgFeature::for_operation(self.graph, op);
                    let query = CodegenQueryParameters::new(self.graph, op);
                    let mod_name = format_ident!(
                        "{}_query",
                        CodegenIdentUsage::Module(self.graph.ident(op.id()))
                    );
                    quote! {
                        #cfg
                        mod #mod_name {
                            #query
                        }
                        #cfg
                        pub use #mod_name::*;
                    }
                })
            })
            .reduce(|a, b| quote!(#a #b))
            .map(|params| {
                quote! {
                    pub mod parameters {
                        #params
                    }
                }
            });

        tokens.append_all(quote! {
            impl crate::client::Client {
                #(#methods)*
            }
            #params
            #inlines
        });
    }
}

impl IntoCode for CodegenResource<'_> {
    type Code = (String, TokenStream);

    fn into_code(self) -> Self::Code {
        (
            match self.resource {
                ResourceGroup::Named(name) => format!(
                    "src/client/{}.rs",
                    CodegenIdentUsage::Module(name).display()
                ),
                ResourceGroup::Default => "src/client/default.rs".to_owned(),
            },
            self.into_token_stream(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use itertools::Itertools;
    use ploidy_core::{
        arena::Arena,
        ir::{RawGraph, Spec},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    use crate::graph::CodegenGraph;

    // MARK: Feature gating

    #[test]
    fn test_operation_method_with_only_unnamed_deps_has_no_cfg() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /customers:
                get:
                  operationId: listCustomers
                  x-resource-name: customer
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            type: array
                            items:
                              $ref: '#/components/schemas/Customer'
            components:
              schemas:
                Customer:
                  type: object
                  properties:
                    address:
                      $ref: '#/components/schemas/Address'
                Address:
                  type: object
                  properties:
                    street:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let ops = graph.operations().collect_vec();
        let [op] = &*ops else {
            panic!("expected one operation; got `{ops:?}`");
        };
        let resource =
            CodegenResource::new(&graph, graph.resource_for(op), std::slice::from_ref(op));

        // No `#[cfg(...)]` on the method because none of its
        // dependencies have an `x-resourceId`.
        let actual: syn::File = parse_quote!(#resource);
        let expected: syn::File = parse_quote! {
            impl crate::client::Client {
                #[doc = " GET /customers"]
                #[cfg_attr(
                    feature = "tracing",
                    ::tracing::instrument(
                        skip_all,
                        fields(
                            otel.name = "GET /customers",
                            otel.kind = "client",
                            url.template = "/customers",
                            http.request.method = "GET",
                            server.address,
                            server.port,
                            url.full,
                            http.response.status_code,
                            error.type
                        )
                    )
                )]
                pub async fn list_customers(
                    &self,
                ) -> Result<::std::vec::Vec<crate::types::Customer>, crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    let url = {
                        let mut url = self.base_url.clone();
                        url.path_segments_mut()
                            .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                            .pop_if_empty()
                            .push("customers");
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
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_operation_method_with_named_deps_has_cfg() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /orders:
                get:
                  operationId: listOrders
                  x-resource-name: orders
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            type: array
                            items:
                              $ref: '#/components/schemas/Order'
            components:
              schemas:
                Order:
                  type: object
                  properties:
                    customer:
                      $ref: '#/components/schemas/Customer'
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let ops = graph.operations().collect_vec();
        let [op] = &*ops else {
            panic!("expected one operation; got `{ops:?}`");
        };
        let resource =
            CodegenResource::new(&graph, graph.resource_for(op), std::slice::from_ref(op));

        // `#[cfg(feature = "customer")]` because `Order` depends on
        // `Customer`, which has `x-resourceId: customer`.
        let actual: syn::File = parse_quote!(#resource);
        let expected: syn::File = parse_quote! {
            impl crate::client::Client {
                #[cfg(feature = "customer")]
                #[doc = " GET /orders"]
                #[cfg_attr(
                    feature = "tracing",
                    ::tracing::instrument(
                        skip_all,
                        fields(
                            otel.name = "GET /orders",
                            otel.kind = "client",
                            url.template = "/orders",
                            http.request.method = "GET",
                            server.address,
                            server.port,
                            url.full,
                            http.response.status_code,
                            error.type
                        )
                    )
                )]
                pub async fn list_orders(
                    &self,
                ) -> Result<::std::vec::Vec<crate::types::Order>, crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    let url = {
                        let mut url = self.base_url.clone();
                        url.path_segments_mut()
                            .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                            .pop_if_empty()
                            .push("orders");
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
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: Parameters module

    #[test]
    fn test_resource_emits_parameters_module() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /customers:
                get:
                  operationId: listCustomers
                  x-resource-name: customer
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

        let ops = graph.operations().collect_vec();
        let [op] = &*ops else {
            panic!("expected one operation; got `{ops:?}`");
        };
        let resource =
            CodegenResource::new(&graph, graph.resource_for(op), std::slice::from_ref(op));

        let actual: syn::File = parse_quote!(#resource);
        let expected: syn::File = parse_quote! {
            impl crate::client::Client {
                #[doc = " GET /customers"]
                #[cfg_attr(
                    feature = "tracing",
                    ::tracing::instrument(
                        skip_all,
                        fields(
                            otel.name = "GET /customers",
                            otel.kind = "client",
                            url.template = "/customers",
                            http.request.method = "GET",
                            server.address,
                            server.port,
                            url.full,
                            http.response.status_code,
                            error.type
                        )
                    )
                )]
                pub async fn list_customers(
                    &self,
                    query: &parameters::ListCustomersQuery
                ) -> Result<(), crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    let url = {
                        let mut url = self.base_url.clone();
                        url.path_segments_mut()
                            .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                            .pop_if_empty()
                            .push("customers");
                        let url = ::ploidy_util::serde::Serialize::serialize(
                            query,
                            ::ploidy_util::QuerySerializer::new(
                                url,
                                parameters::ListCustomersQuery::STYLES,
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
            }
            pub mod parameters {
                mod list_customers_query {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
                    #[serde(crate = "::ploidy_util::serde")]
                    pub struct ListCustomersQuery {
                        #[serde(default, skip_serializing_if = "Option::is_none")]
                        pub limit: ::std::option::Option<i32>,
                    }
                    impl ListCustomersQuery {
                        pub const STYLES: &[(&str, ::ploidy_util::QueryStyle)] = &[];
                    }
                }
                pub use list_customers_query::*;
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_resource_with_multiple_query_ops_shares_parameters_module() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /customers:
                get:
                  operationId: listCustomers
                  x-resource-name: customer
                  parameters:
                    - name: limit
                      in: query
                      schema:
                        type: integer
                        format: int32
                  responses:
                    '200':
                      description: OK
              /customers/search:
                get:
                  operationId: searchCustomers
                  x-resource-name: customer
                  parameters:
                    - name: email
                      in: query
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

        let ops = graph.operations().collect_vec();
        let resource = ops
            .iter()
            .map(|op| graph.resource_for(op))
            .all_equal_value()
            .unwrap();
        let resource = CodegenResource::new(&graph, resource, &ops);

        let actual: syn::File = parse_quote!(#resource);
        let expected: syn::File = parse_quote! {
            impl crate::client::Client {
                #[doc = " GET /customers"]
                #[cfg_attr(
                    feature = "tracing",
                    ::tracing::instrument(
                        skip_all,
                        fields(
                            otel.name = "GET /customers",
                            otel.kind = "client",
                            url.template = "/customers",
                            http.request.method = "GET",
                            server.address,
                            server.port,
                            url.full,
                            http.response.status_code,
                            error.type
                        )
                    )
                )]
                pub async fn list_customers(
                    &self,
                    query: &parameters::ListCustomersQuery
                ) -> Result<(), crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    let url = {
                        let mut url = self.base_url.clone();
                        url.path_segments_mut()
                            .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                            .pop_if_empty()
                            .push("customers");
                        let url = ::ploidy_util::serde::Serialize::serialize(
                            query,
                            ::ploidy_util::QuerySerializer::new(
                                url,
                                parameters::ListCustomersQuery::STYLES,
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
                #[doc = " GET /customers/search"]
                #[cfg_attr(
                    feature = "tracing",
                    ::tracing::instrument(
                        skip_all,
                        fields(
                            otel.name = "GET /customers/search",
                            otel.kind = "client",
                            url.template = "/customers/search",
                            http.request.method = "GET",
                            server.address,
                            server.port,
                            url.full,
                            http.response.status_code,
                            error.type
                        )
                    )
                )]
                pub async fn search_customers(
                    &self,
                    query: &parameters::SearchCustomersQuery
                ) -> Result<(), crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    let url = {
                        let mut url = self.base_url.clone();
                        url.path_segments_mut()
                            .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                            .pop_if_empty()
                            .extend(&["customers", "search"]);
                        let url = ::ploidy_util::serde::Serialize::serialize(
                            query,
                            ::ploidy_util::QuerySerializer::new(
                                url,
                                parameters::SearchCustomersQuery::STYLES,
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
            }
            pub mod parameters {
                mod list_customers_query {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
                    #[serde(crate = "::ploidy_util::serde")]
                    pub struct ListCustomersQuery {
                        #[serde(default, skip_serializing_if = "Option::is_none")]
                        pub limit: ::std::option::Option<i32>,
                    }
                    impl ListCustomersQuery {
                        pub const STYLES: &[(&str, ::ploidy_util::QueryStyle)] = &[];
                    }
                }
                pub use list_customers_query::*;
                mod search_customers_query {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
                    #[serde(crate = "::ploidy_util::serde")]
                    pub struct SearchCustomersQuery {
                        pub email: ::std::string::String,
                    }
                    impl SearchCustomersQuery {
                        pub const STYLES: &[(&str, ::ploidy_util::QueryStyle)] = &[];
                    }
                }
                pub use search_customers_query::*;
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_resource_omits_parameters_module_when_no_query_params() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /customers:
                get:
                  operationId: listCustomers
                  x-resource-name: customer
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let ops = graph.operations().collect_vec();
        let [op] = &*ops else {
            panic!("expected one operation; got `{ops:?}`");
        };
        let resource =
            CodegenResource::new(&graph, graph.resource_for(op), std::slice::from_ref(op));

        let actual: syn::File = parse_quote!(#resource);
        let expected: syn::File = parse_quote! {
            impl crate::client::Client {
                #[doc = " GET /customers"]
                #[cfg_attr(
                    feature = "tracing",
                    ::tracing::instrument(
                        skip_all,
                        fields(
                            otel.name = "GET /customers",
                            otel.kind = "client",
                            url.template = "/customers",
                            http.request.method = "GET",
                            server.address,
                            server.port,
                            url.full,
                            http.response.status_code,
                            error.type
                        )
                    )
                )]
                pub async fn list_customers(
                    &self,
                ) -> Result<(), crate::error::Error> {
                let result: Result<_, crate::error::Error> = async move {
                    let url = {
                        let mut url = self.base_url.clone();
                        url.path_segments_mut()
                            .map_err(|()| ::ploidy_util::url::PathAndQueryError::UrlCannotBeABase)?
                            .pop_if_empty()
                            .push("customers");
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
            }
        };
        assert_eq!(actual, expected);
    }
}
