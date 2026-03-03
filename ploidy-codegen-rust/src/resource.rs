use ploidy_core::{codegen::IntoCode, ir::IrOperationView};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    cfg::CfgFeature,
    inlines::CodegenInlines,
    naming::{CargoFeature, CodegenIdentUsage},
    operation::CodegenOperation,
};

/// Generates an `impl Client` block for a feature-gated resource,
/// with all its operations and inline types.
pub struct CodegenResource<'a> {
    feature: &'a CargoFeature,
    ops: &'a [IrOperationView<'a>],
}

impl<'a> CodegenResource<'a> {
    pub fn new(feature: &'a CargoFeature, ops: &'a [IrOperationView<'a>]) -> Self {
        Self { feature, ops }
    }
}

impl ToTokens for CodegenResource<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        // Each method gets its own `#[cfg(...)]` attribute.
        let methods = self.ops.iter().map(|view| {
            let cfg = CfgFeature::for_operation(view);
            let method = CodegenOperation::new(view).into_token_stream();
            quote! {
                #cfg
                #method
            }
        });
        let inlines = CodegenInlines::Resource(self.ops);
        tokens.append_all(quote! {
            impl crate::client::Client {
                #(#methods)*
            }
            #inlines
        });
    }
}

impl IntoCode for CodegenResource<'_> {
    type Code = (String, TokenStream);

    fn into_code(self) -> Self::Code {
        (
            format!(
                "src/client/{}.rs",
                CodegenIdentUsage::Module(self.feature.as_ident()).display()
            ),
            self.into_token_stream(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use itertools::Itertools;
    use ploidy_core::{ir::Ir, parse::Document};
    use pretty_assertions::assert_eq;

    use syn::parse_quote;

    use crate::{graph::CodegenGraph, naming::CargoFeature};

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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let ops = graph.operations().collect_vec();
        let feature = CargoFeature::from_name("customer");
        let resource = CodegenResource::new(&feature, &ops);

        // Parse the generated tokens as a file, then
        // extract the `impl` block containing the methods.
        let actual: syn::File = parse_quote!(#resource);
        let block = actual
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Impl(block) => Some(block),
                _ => None,
            })
            .unwrap();

        // The method should not have a `#[cfg(...)]` attribute,
        // since none of its dependencies have an `x-resourceId`.
        let methods = block
            .items
            .iter()
            .filter_map(|item| match item {
                syn::ImplItem::Fn(method) => Some(method),
                _ => None,
            })
            .collect_vec();
        assert_eq!(methods.len(), 1);
        assert!(
            !methods[0]
                .attrs
                .iter()
                .any(|attr| attr.path().is_ident("cfg"))
        );
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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let ops = graph.operations().collect_vec();
        let feature = CargoFeature::from_name("orders");
        let resource = CodegenResource::new(&feature, &ops);

        let actual: syn::File = parse_quote!(#resource);
        let block = actual
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Impl(block) => Some(block),
                _ => None,
            })
            .unwrap();

        // The method should have a `#[cfg(feature = "customer")]` attribute,
        // since `Order` (no resource) depends on `Customer` (has `x-resourceId`).
        let methods = block
            .items
            .iter()
            .filter_map(|item| match item {
                syn::ImplItem::Fn(method) => Some(method),
                _ => None,
            })
            .collect_vec();
        assert_eq!(methods.len(), 1);
        let cfg = methods[0]
            .attrs
            .iter()
            .find(|attr| attr.path().is_ident("cfg"));
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "customer")]);
        assert_eq!(cfg, Some(&expected));
    }
}
