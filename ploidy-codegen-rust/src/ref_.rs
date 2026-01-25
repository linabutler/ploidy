use ploidy_core::ir::{ExtendableView, InlineIrTypePathRoot, IrTypeView};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};
use syn::parse_quote;

use super::{
    naming::{CargoFeature, CodegenIdent, CodegenIdentUsage, CodegenTypeName},
    primitive::CodegenPrimitive,
};

#[derive(Clone, Copy, Debug)]
pub struct CodegenRef<'a> {
    ty: &'a IrTypeView<'a>,
}

impl<'a> CodegenRef<'a> {
    pub fn new(ty: &'a IrTypeView<'a>) -> Self {
        Self { ty }
    }
}

impl ToTokens for CodegenRef<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(match self.ty {
            IrTypeView::Primitive(ty) => {
                let ty = CodegenPrimitive::new(ty);
                quote!(#ty)
            }
            IrTypeView::Array(ty) => {
                let inner = ty.inner();
                let ty = CodegenRef::new(&inner);
                quote! { ::std::vec::Vec<#ty> }
            }
            IrTypeView::Map(ty) => {
                let inner = ty.inner();
                let ty = CodegenRef::new(&inner);
                quote! { ::std::collections::BTreeMap<::std::string::String, #ty> }
            }
            IrTypeView::Optional(ty) => {
                let inner = ty.inner();
                let ty = CodegenRef::new(&inner);
                quote! { ::std::option::Option<#ty> }
            }
            IrTypeView::Any => quote! { ::serde_json::Value },
            IrTypeView::Inline(ty) => {
                let path = ty.path();
                let root: syn::Path = match path.root {
                    InlineIrTypePathRoot::Resource(resource) => {
                        let feature = resource.map(CargoFeature::from_name).unwrap_or_default();
                        let module = CodegenIdentUsage::Module(feature.as_ident());
                        parse_quote!(crate::client::#module::types)
                    }
                    InlineIrTypePathRoot::Type(name) => {
                        let ident = CodegenIdent::new(name);
                        let module = CodegenIdentUsage::Module(&ident);
                        parse_quote!(crate::types::#module::types)
                    }
                };
                let name = CodegenTypeName::Inline(ty);
                parse_quote!(#root::#name)
            }
            IrTypeView::Schema(view) => {
                let ext = view.extensions();
                let ident = ext.get::<CodegenIdent>().unwrap();
                let usage = CodegenIdentUsage::Type(&ident);
                quote! { crate::types::#usage }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        ir::{IrGraph, IrSpec, IrStructFieldName, IrTypeView, SchemaIrTypeView},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    use crate::{CodegenGraph, tests::assert_matches};

    #[test]
    fn test_codegen_ref_any() {
        let ty = IrTypeView::Any;
        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote!(::serde_json::Value);
        assert_eq!(actual, expected);
    }

    // MARK: Wrappers

    #[test]
    fn test_codegen_ref_array_of_strings() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Container:
                  type: object
                  required:
                    - items
                  properties:
                    items:
                      type: array
                      items:
                        type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), IrStructFieldName::Name("items")))
            .unwrap();
        let ty = field.ty();
        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote!(::std::vec::Vec<::std::string::String>);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ref_array_of_i32() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Container:
                  type: object
                  required:
                    - numbers
                  properties:
                    numbers:
                      type: array
                      items:
                        type: integer
                        format: int32
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), IrStructFieldName::Name("numbers")))
            .unwrap();
        let ty = field.ty();
        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote!(::std::vec::Vec<i32>);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ref_map_of_strings() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Container:
                  type: object
                  required:
                    - metadata
                  properties:
                    metadata:
                      type: object
                      additionalProperties:
                        type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), IrStructFieldName::Name("metadata")))
            .unwrap();
        let ty = field.ty();
        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote! {
            ::std::collections::BTreeMap<::std::string::String, ::std::string::String>
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ref_map_of_i64() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Container:
                  type: object
                  required:
                    - counters
                  properties:
                    counters:
                      type: object
                      additionalProperties:
                        type: integer
                        format: int64
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), IrStructFieldName::Name("counters")))
            .unwrap();
        let ty = field.ty();
        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote! {
            ::std::collections::BTreeMap<::std::string::String, i64>
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ref_nullable_string() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Container:
                  type: object
                  properties:
                    value:
                      type: string
                      nullable: true
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), IrStructFieldName::Name("value")))
            .unwrap();
        let ty = field.ty();
        assert_matches!(ty, IrTypeView::Optional(_));

        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote!(::std::option::Option<::std::string::String>);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ref_nullable_i32() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Container:
                  type: object
                  properties:
                    count:
                      type: integer
                      format: int32
                      nullable: true
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), IrStructFieldName::Name("count")))
            .unwrap();
        let ty = field.ty();
        assert_matches!(ty, IrTypeView::Optional(_));

        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote!(::std::option::Option<i32>);
        assert_eq!(actual, expected);
    }

    // MARK: Nested wrappers

    #[test]
    fn test_codegen_ref_array_of_arrays() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Container:
                  type: object
                  required: [matrix]
                  properties:
                    matrix:
                      type: array
                      items:
                        type: array
                        items:
                          type: integer
                          format: int32
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), IrStructFieldName::Name("matrix")))
            .unwrap();
        let ty = field.ty();

        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote! {
            ::std::vec::Vec<::std::vec::Vec<i32>>
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ref_nullable_array() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Container:
                  type: object
                  properties:
                    items:
                      type: array
                      items:
                        type: string
                      nullable: true
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), IrStructFieldName::Name("items")))
            .unwrap();
        let ty = field.ty();
        assert_matches!(ty, IrTypeView::Optional(_));

        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote! {
            ::std::option::Option<::std::vec::Vec<::std::string::String>>
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ref_map_of_arrays() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Container:
                  type: object
                  required: [data]
                  properties:
                    data:
                      type: object
                      additionalProperties:
                        type: array
                        items:
                          type: boolean
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), IrStructFieldName::Name("data")))
            .unwrap();
        let ty = field.ty();

        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote! {
            ::std::collections::BTreeMap<::std::string::String, ::std::vec::Vec<bool>>
        };
        assert_eq!(actual, expected);
    }

    // MARK: Schema references

    #[test]
    fn test_codegen_ref_schema_reference() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Pet:
                  type: object
                  properties:
                    name:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph
            .schemas()
            .find(|s| s.name() == "Pet")
            .expect("expected schema `Pet`");
        let ty = IrTypeView::Schema(schema);
        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote!(crate::types::Pet);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ref_array_of_schema_references() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                User:
                  type: object
                  properties:
                    id:
                      type: string
                Container:
                  type: object
                  required: [users]
                  properties:
                    users:
                      type: array
                      items:
                        $ref: '#/components/schemas/User'
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), ploidy_core::ir::IrStructFieldName::Name("users")))
            .unwrap();
        let ty = field.ty();

        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote! {
            ::std::vec::Vec<crate::types::User>
        };
        assert_eq!(actual, expected);
    }

    // MARK: Inline references

    #[test]
    fn test_codegen_ref_inline_type_from_schema() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Container:
                  type: object
                  required:
                    - nested
                  properties:
                    nested:
                      type: object
                      properties:
                        value:
                          type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), IrStructFieldName::Name("nested")))
            .unwrap();
        let ty = field.ty();

        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote!(crate::types::container::types::Nested);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ref_inline_type_from_resource_operation() {
        use ploidy_core::ir::IrRequestView;

        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /pets:
                post:
                  operationId: createPet
                  x-resource-name: pets
                  requestBody:
                    content:
                      application/json:
                        schema:
                          type: object
                          properties:
                            name:
                              type: string
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let op = graph
            .operations()
            .find(|op| op.id() == "createPet")
            .unwrap();
        let Some(IrRequestView::Json(ty)) = op.request() else {
            panic!(
                "expected JSON request body for operation `createPet`; got {:?}",
                op.request()
            );
        };

        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote!(crate::client::pets::types::CreatePetRequest);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ref_inline_type_from_operation_without_resource() {
        use ploidy_core::ir::IrRequestView;

        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /misc:
                post:
                  operationId: doSomething
                  requestBody:
                    content:
                      application/json:
                        schema:
                          type: object
                          properties:
                            data:
                              type: string
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let op = graph
            .operations()
            .find(|op| op.id() == "doSomething")
            .unwrap();
        let Some(IrRequestView::Json(ty)) = op.request() else {
            panic!(
                "expected JSON request body for operation `doSomething`; got {:?}",
                op.request()
            );
        };

        // Operations without a declared resource name
        // should use `full`.
        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote!(crate::client::full::types::DoSomethingRequest);
        assert_eq!(actual, expected);
    }
}
