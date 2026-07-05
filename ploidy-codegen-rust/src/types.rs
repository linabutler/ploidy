use itertools::Itertools;
use ploidy_core::{
    codegen::IntoCode,
    ir::{HasTypeId, OperationView, ResponseView, TypeId, TypeView},
};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, format_ident, quote};

use super::{
    cfg::CfgFeature,
    graph::CodegenGraph,
    naming::{CodegenIdentUsage, status_variant_name},
    ref_::CodegenRef,
};

/// Generates the `types/mod.rs` module.
pub struct CodegenTypesModule<'a> {
    graph: &'a CodegenGraph<'a>,
}

impl<'a> CodegenTypesModule<'a> {
    pub fn new(graph: &'a CodegenGraph<'a>) -> Self {
        Self { graph }
    }
}

impl ToTokens for CodegenTypesModule<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut tys = self.graph.schemas().collect_vec();
        tys.sort_by_key(|s| self.graph.ident(s.id()));
        let mut result_ops = self
            .graph
            .operations()
            .filter(|op| {
                let mut shapes: Vec<Option<TypeId>> = vec![];
                for case in op.response_cases() {
                    let shape = case.body().map(|body| match body {
                        ResponseView::Json(view) | ResponseView::Headers(view) => match view {
                            TypeView::Schema(view) => view.id(),
                            TypeView::Inline(view) => view.id(),
                        },
                    });
                    if !shapes.contains(&shape) {
                        shapes.push(shape);
                    }
                }
                shapes.len() > 1
            })
            .collect_vec();
        result_ops.sort_by_key(|op| self.graph.ident(op.id()));

        let mods = tys.iter().map(|schema| {
            let cfg = CfgFeature::for_schema_type(self.graph, schema);
            let mod_name = CodegenIdentUsage::Module(self.graph.ident(schema.id()));
            quote! {
                #cfg
                pub mod #mod_name;
            }
        });
        let result_mods = result_ops.iter().map(|op| {
            let cfg = CfgFeature::for_operation(self.graph, op);
            let mod_name = format_ident!(
                "{}_result",
                CodegenIdentUsage::Module(self.graph.ident(op.id()))
            );
            quote! {
                #cfg
                pub mod #mod_name;
            }
        });
        let uses = tys.iter().map(|schema| {
            let cfg = CfgFeature::for_schema_type(self.graph, schema);
            let ident = self.graph.ident(schema.id());
            let ty_name = CodegenIdentUsage::Type(ident);
            let mod_name = CodegenIdentUsage::Module(ident);
            quote! {
                #cfg
                pub use #mod_name::#ty_name;
            }
        });
        let result_uses = result_ops.iter().map(|op| {
            let cfg = CfgFeature::for_operation(self.graph, op);
            let type_name = format_ident!(
                "{}Result",
                CodegenIdentUsage::Type(self.graph.ident(op.id()))
            );
            let mod_name = format_ident!(
                "{}_result",
                CodegenIdentUsage::Module(self.graph.ident(op.id()))
            );
            quote! {
                #cfg
                pub use #mod_name::#type_name;
            }
        });

        tokens.append_all(quote! {
            #(#mods)*
            #(#result_mods)*
            #(#uses)*
            #(#result_uses)*
        });
    }
}

impl IntoCode for CodegenTypesModule<'_> {
    type Code = (&'static str, TokenStream);

    fn into_code(self) -> Self::Code {
        ("src/types/mod.rs", self.into_token_stream())
    }
}

/// Generates an operation result enum for multiple successful response shapes.
#[derive(Debug)]
pub struct CodegenOperationResult<'a> {
    graph: &'a CodegenGraph<'a>,
    op: &'a OperationView<'a, 'a>,
}

impl<'a> CodegenOperationResult<'a> {
    pub fn new(graph: &'a CodegenGraph<'a>, op: &'a OperationView<'a, 'a>) -> Self {
        Self { graph, op }
    }
}

impl ToTokens for CodegenOperationResult<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let type_name = format_ident!(
            "{}Result",
            CodegenIdentUsage::Type(self.graph.ident(self.op.id()))
        );
        let variants = self.op.response_cases().map(|case| {
            let status = case.status();
            let variant_name = format_ident!("{}", status_variant_name(status));
            match case.body() {
                Some(ResponseView::Json(view) | ResponseView::Headers(view)) => {
                    let ty = CodegenRef::new(self.graph, &view);
                    quote! { #variant_name(#ty) }
                }
                None => quote! { #variant_name },
            }
        });
        tokens.append_all(quote! {
            #[derive(Clone, Debug, PartialEq)]
            pub enum #type_name {
                #(#variants),*
            }
        });
    }
}

impl IntoCode for CodegenOperationResult<'_> {
    type Code = (String, TokenStream);

    fn into_code(self) -> Self::Code {
        let mod_name = format_ident!(
            "{}_result",
            CodegenIdentUsage::Module(self.graph.ident(self.op.id()))
        );
        (
            format!("src/types/{}.rs", mod_name),
            self.into_token_stream(),
        )
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

    use crate::graph::CodegenGraph;

    #[test]
    fn test_operation_result_file_for_distinct_success_bodies() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /jobs:
                post:
                  operationId: startJob
                  responses:
                    '200':
                      description: Complete
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Job'
                    '202':
                      description: Accepted
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/PendingJob'
            components:
              schemas:
                Job:
                  type: object
                PendingJob:
                  type: object
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().next().unwrap();
        let (path, tokens) = CodegenOperationResult::new(&graph, &op).into_code();

        assert_eq!(path, "src/types/start_job_result.rs");
        let actual: syn::File = parse_quote!(#tokens);
        let expected: syn::File = parse_quote! {
            #[derive(Clone, Debug, PartialEq)]
            pub enum StartJobResult {
                Ok(crate::types::Job),
                Accepted(crate::types::PendingJob)
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_types_module_exports_operation_result() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /jobs:
                post:
                  operationId: startJob
                  responses:
                    '200':
                      description: Complete
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Job'
                    '202':
                      description: Accepted
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/PendingJob'
            components:
              schemas:
                Job:
                  type: object
                PendingJob:
                  type: object
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let codegen = CodegenTypesModule::new(&graph);
        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub mod job;
            pub mod pending_job;
            pub mod start_job_result;
            pub use job::Job;
            pub use pending_job::PendingJob;
            pub use start_job_result::StartJobResult;
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_operation_result_variant_for_header_only_response() {
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
                    '200':
                      description: Complete
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Job'
                    '304':
                      description: Not modified.
                      headers:
                        etag:
                          required: true
                          schema:
                            type: string
            components:
              schemas:
                Job:
                  type: object
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().next().unwrap();
        let (path, tokens) = CodegenOperationResult::new(&graph, &op).into_code();

        assert_eq!(path, "src/types/get_job_result.rs");
        let actual: syn::File = parse_quote!(#tokens);
        let expected: syn::File = parse_quote! {
            #[derive(Clone, Debug, PartialEq)]
            pub enum GetJobResult {
                Ok(crate::types::Job),
                NotModified(crate::client::default::types::GetJobResponseNotModified)
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_types_module_exports_result_for_body_and_header_shapes() {
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
                    '200':
                      description: Complete
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Job'
                    '304':
                      description: Not modified.
                      headers:
                        etag:
                          required: true
                          schema:
                            type: string
            components:
              schemas:
                Job:
                  type: object
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let codegen = CodegenTypesModule::new(&graph);
        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub mod job;
            pub mod get_job_result;
            pub use job::Job;
            pub use get_job_result::GetJobResult;
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_operation_result_file_for_bodyless_success() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /jobs:
                post:
                  operationId: startJob
                  responses:
                    '200':
                      description: Complete
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Job'
                    '204':
                      description: No content
            components:
              schemas:
                Job:
                  type: object
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().next().unwrap();
        let codegen = CodegenOperationResult::new(&graph, &op);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Clone, Debug, PartialEq)]
            pub enum StartJobResult {
                Ok(crate::types::Job),
                NoContent
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_types_module_omits_result_for_same_success_body() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /jobs:
                post:
                  operationId: startJob
                  responses:
                    '200':
                      description: Complete
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Job'
                    '202':
                      description: Accepted
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Job'
            components:
              schemas:
                Job:
                  type: object
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let codegen = CodegenTypesModule::new(&graph);
        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub mod job;
            pub use job::Job;
        };
        assert_eq!(actual, expected);
    }
}
