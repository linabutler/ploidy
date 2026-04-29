use ploidy_core::ir::{
    ContainerView, ExtendableView, InlineTypePathRoot, InlineTypeView, SchemaTypeView, TypeView,
};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};
use syn::parse_quote;

use super::{
    naming::{CargoFeature, CodegenIdent, CodegenIdentUsage, CodegenTypeName},
    primitive::CodegenPrimitive,
};

#[derive(Clone, Copy, Debug)]
pub struct CodegenRef<'a> {
    ty: &'a TypeView<'a, 'a>,
}

impl<'a> CodegenRef<'a> {
    pub fn new(ty: &'a TypeView<'a, 'a>) -> Self {
        Self { ty }
    }
}

impl ToTokens for CodegenRef<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(match self.ty {
            // Emit inline container types, primitive types, and
            // untyped values directly. Note that we only do this for inlines;
            // named schema containers are always emitted as references.
            TypeView::Inline(InlineTypeView::Container(_, ContainerView::Array(inner))) => {
                let inner_ty = inner.ty();
                let inner_ref = CodegenRef::new(&inner_ty);
                quote! { ::std::vec::Vec<#inner_ref> }
            }
            TypeView::Inline(InlineTypeView::Container(_, ContainerView::Map(inner))) => {
                let inner_ty = inner.ty();
                let inner_ref = CodegenRef::new(&inner_ty);
                quote! { ::std::collections::BTreeMap<::std::string::String, #inner_ref> }
            }
            TypeView::Inline(InlineTypeView::Container(_, ContainerView::Optional(inner))) => {
                let inner_ty = inner.ty();
                let inner_ref = CodegenRef::new(&inner_ty);
                quote! { ::std::option::Option<#inner_ref> }
            }
            TypeView::Inline(InlineTypeView::Primitive(_, view)) => {
                let ty = CodegenPrimitive::new(view);
                quote!(#ty)
            }
            TypeView::Inline(InlineTypeView::Any(_, _)) => {
                quote! { ::ploidy_util::serde_json::Value }
            }
            TypeView::Inline(InlineTypeView::Composition(_, view)) => {
                let mut all_of = view.all_of().filter(|ty| !is_any(ty));
                let Some(ty) = all_of.next() else {
                    return tokens.append_all(quote! { ::ploidy_util::serde_json::Value });
                };
                assert!(
                    all_of.next().is_none(),
                    "Rust codegen only supports single-parent inline compositions"
                );
                let ref_ = CodegenRef::new(&ty);
                quote! { #ref_ }
            }
            TypeView::Inline(ty) => {
                let path = ty.path();
                let root: syn::Path = match path.root {
                    InlineTypePathRoot::Resource(resource) => {
                        let feature = resource.map(CargoFeature::from_name).unwrap_or_default();
                        let mod_name = CodegenIdentUsage::Module(feature.as_ident());
                        parse_quote!(crate::client::#mod_name::types)
                    }
                    InlineTypePathRoot::Type(name) => {
                        let mod_ident = CodegenIdent::new(name);
                        let mod_name = CodegenIdentUsage::Module(&mod_ident);
                        parse_quote!(crate::types::#mod_name::types)
                    }
                };
                let ty_name = CodegenTypeName::Inline(ty);
                parse_quote!(#root::#ty_name)
            }
            TypeView::Schema(ty) => {
                let ext = ty.extensions();
                let ty_ident = ext.get::<CodegenIdent>().unwrap();
                let ty_name = CodegenIdentUsage::Type(&ty_ident);
                quote! { crate::types::#ty_name }
            }
        })
    }
}

fn is_any(ty: &TypeView<'_, '_>) -> bool {
    matches!(
        ty,
        TypeView::Schema(SchemaTypeView::Any(..)) | TypeView::Inline(InlineTypeView::Any(..))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        arena::Arena,
        ir::{
            ContainerView, InlineTypeView, RawGraph, SchemaTypeView, Spec, StructFieldName,
            TypeView,
        },
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    use crate::{CodegenGraph, tests::assert_matches};

    #[test]
    fn test_codegen_ref_any() {
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
                    - data
                  properties:
                    data: {}
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), StructFieldName::Name("data")))
            .unwrap();
        let ty = field.ty();
        assert_matches!(ty, TypeView::Inline(InlineTypeView::Any(_, _)));

        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote!(::ploidy_util::serde_json::Value);
        assert_eq!(actual, expected);
    }

    // MARK: Containers

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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), StructFieldName::Name("items")))
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), StructFieldName::Name("numbers")))
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), StructFieldName::Name("metadata")))
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), StructFieldName::Name("counters")))
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), StructFieldName::Name("value")))
            .unwrap();
        let ty = field.ty();
        assert_matches!(
            ty,
            TypeView::Inline(InlineTypeView::Container(_, ContainerView::Optional(_)))
        );

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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), StructFieldName::Name("count")))
            .unwrap();
        let ty = field.ty();
        assert_matches!(
            ty,
            TypeView::Inline(InlineTypeView::Container(_, ContainerView::Optional(_)))
        );

        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote!(::std::option::Option<i32>);
        assert_eq!(actual, expected);
    }

    // MARK: Nested containers

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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), StructFieldName::Name("matrix")))
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), StructFieldName::Name("items")))
            .unwrap();
        let ty = field.ty();
        assert_matches!(
            ty,
            TypeView::Inline(InlineTypeView::Container(_, ContainerView::Optional(_)))
        );

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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), StructFieldName::Name("data")))
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Pet").expect("expected schema `Pet`");
        let ty = TypeView::Schema(schema);
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), ploidy_core::ir::StructFieldName::Name("users")))
            .unwrap();
        let ty = field.ty();

        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote! {
            ::std::vec::Vec<crate::types::User>
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ref_container_schema() {
        // A reference to a named container schema should generate
        // `crate::types::Name`, the same as any other schema.
        // The actual schema is emitted as a type alias.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Tags:
                  type: array
                  items:
                    type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Tags").expect("expected schema `Tags`");
        let ty = TypeView::Schema(schema);
        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote!(crate::types::Tags);
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), StructFieldName::Name("nested")))
            .unwrap();
        let ty = field.ty();

        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote!(crate::types::container::types::Nested);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ref_inline_type_from_resource_operation() {
        use ploidy_core::ir::RequestView;

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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph
            .operations()
            .find(|op| op.id() == "createPet")
            .unwrap();
        let Some(RequestView::Json(ty)) = op.request() else {
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
        use ploidy_core::ir::RequestView;

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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph
            .operations()
            .find(|op| op.id() == "doSomething")
            .unwrap();
        let Some(RequestView::Json(ty)) = op.request() else {
            panic!(
                "expected JSON request body for operation `doSomething`; got {:?}",
                op.request()
            );
        };

        // Operations without a declared resource name
        // should use `default`.
        let ref_ = CodegenRef::new(&ty);
        let actual: syn::Type = parse_quote!(#ref_);
        let expected: syn::Type = parse_quote!(crate::client::default::types::DoSomethingRequest);
        assert_eq!(actual, expected);
    }
}
