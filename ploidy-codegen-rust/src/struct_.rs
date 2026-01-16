use either::Either;
use itertools::Itertools;
use ploidy_core::{
    codegen::UniqueNames,
    ir::{
        InlineIrTypeView, IrStructFieldName, IrStructFieldView, IrStructView, IrTypeView,
        PrimitiveIrType, SchemaIrTypeView, Traversal, View,
    },
};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};
use syn::{Ident, parse_quote};

use super::{
    derives::ExtraDerive,
    doc_attrs,
    naming::{CodegenIdentScope, CodegenIdentUsage, CodegenStructFieldName, CodegenTypeName},
    ref_::CodegenRef,
};

#[derive(Clone, Debug)]
pub struct CodegenStruct<'a> {
    name: CodegenTypeName<'a>,
    ty: &'a IrStructView<'a>,
}

impl<'a> CodegenStruct<'a> {
    pub fn new(name: CodegenTypeName<'a>, ty: &'a IrStructView<'a>) -> Self {
        Self { name, ty }
    }
}

impl ToTokens for CodegenStruct<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let unique = UniqueNames::new();
        let mut scope = CodegenIdentScope::new(&unique);
        let fields = self
            .ty
            .fields()
            .filter(|field| !field.discriminator())
            .map(|field| {
                let field_name: Ident = match field.name() {
                    IrStructFieldName::Name(n) => {
                        let name = CodegenIdentUsage::Field(&scope.uniquify(n));
                        parse_quote!(#name)
                    }
                    IrStructFieldName::Hint(hint) => {
                        let name = CodegenStructFieldName(hint);
                        parse_quote!(#name)
                    }
                };

                let codegen_field = CodegenField::new(&field);
                let final_type = codegen_field.to_token_stream();

                let serde_attrs = SerdeFieldAttr::new(&field_name, &field);
                let doc_attrs = field.description().map(doc_attrs);

                quote! {
                    #doc_attrs
                    #serde_attrs
                    pub #field_name: #final_type,
                }
            })
            .collect_vec();

        let mut extra_derives = vec![];
        // Structs that don't contain any floating-point types
        // can derive `Eq` and `Hash`.
        let is_hashable = self.ty.reachable().all(|view| {
            if let IrTypeView::Primitive(p) = &view
                && let PrimitiveIrType::F32 | PrimitiveIrType::F64 = p.ty()
            {
                false
            } else {
                true
            }
        });
        if is_hashable {
            extra_derives.push(ExtraDerive::Eq);
            extra_derives.push(ExtraDerive::Hash);
        }
        let is_defaultable = self
            .ty
            .reachable_if(|view| {
                match view {
                    IrTypeView::Optional(_) | IrTypeView::Array(_) | IrTypeView::Map(_) => {
                        // All wrappers implement `Default`: optional fields become `AbsentOr<T>`,
                        // nullable fields become `Option<T>`, and arrays and maps don't require
                        // `T: Default`.
                        Traversal::Ignore
                    }
                    IrTypeView::Schema(SchemaIrTypeView::Struct(_, view))
                    | IrTypeView::Inline(InlineIrTypeView::Struct(_, view)) => {
                        if view
                            .fields()
                            .filter(|f| !f.discriminator())
                            .all(|f| !f.required())
                        {
                            // If all non-discriminator fields of all reachable structs
                            // are optional, then this struct can derive `Default`.
                            Traversal::Ignore
                        } else {
                            // Otherwise, skip the struct itself, but visit all its fields
                            // to determine which ones are defaultable.
                            Traversal::Skip
                        }
                    }
                    _ => Traversal::Visit,
                }
            })
            .all(|ty| {
                match ty {
                    // `serde_json::Value` implements `Default`.
                    IrTypeView::Any => true,
                    // `Url` doesn't implement `Default`, but other primitives do.
                    IrTypeView::Primitive(p) if p.ty() == PrimitiveIrType::Url => false,
                    IrTypeView::Primitive(_) => true,
                    // Other types aren't defaultable.
                    _ => false,
                }
            });
        if is_defaultable {
            extra_derives.push(ExtraDerive::Default);
        }

        let type_name = &self.name;
        let doc_attrs = self.ty.description().map(doc_attrs);

        tokens.append_all(quote! {
            #doc_attrs
            #[derive(Debug, Clone, PartialEq, #(#extra_derives,)* ::serde::Serialize, ::serde::Deserialize)]
            pub struct #type_name {
                #(#fields)*
            }
        });
    }
}

/// A field in a struct, ready for code generation.
#[derive(Debug)]
struct CodegenField<'view, 'a> {
    field: &'a IrStructFieldView<'view, 'a>,
}

impl<'view, 'a> CodegenField<'view, 'a> {
    fn new(field: &'a IrStructFieldView<'view, 'a>) -> Self {
        Self { field }
    }

    fn needs_box(&self) -> bool {
        let mut ty = self.field.ty();
        loop {
            match ty {
                IrTypeView::Optional(optional) => {
                    // Unwrap nested optionals, since `Optional(T)`
                    // doesn't determine whether T needs to be boxed.
                    ty = optional.inner();
                }
                IrTypeView::Array(_)
                | IrTypeView::Map(_)
                | IrTypeView::Primitive(_)
                | IrTypeView::Any => {
                    // Arrays and maps are heap-allocated, and so already
                    // provide their own indirection. Leaf types like primitives
                    // and `Any` don't contain references, and don't need
                    // boxing, either.
                    return false;
                }
                _ => {
                    // For other types, consult whether the struct field has
                    // an edge back to the struct.
                    return self.field.needs_indirection();
                }
            }
        }
    }
}

impl ToTokens for CodegenField<'_, '_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        // For `Optional(T)`, we emit either `Option<T>` or `AbsentOr<T>`,
        // depending on whether the field is required; but `CodegenRef`
        // always emits `Option<T>`, so we extract the inner T to avoid
        // double-wrapping.
        let inner_view = match self.field.ty() {
            IrTypeView::Optional(nullable) => Either::Left(nullable.inner()),
            other => Either::Right(other),
        };

        let inner_ty = CodegenRef::new(inner_view.as_ref().into_inner());
        let inner = if self.needs_box() {
            quote! { ::std::boxed::Box<#inner_ty> }
        } else {
            quote! { #inner_ty }
        };

        tokens.append_all(match (inner_view, self.field.required()) {
            // Since `AbsentOr` can represent `null`,
            // always emit it for optional fields.
            (_, false) => quote! { ::ploidy_util::absent::AbsentOr<#inner> },
            // For required fields, use `Option` if it's nullable,
            // or the original type if not.
            (Either::Left(_), true) => quote! { ::std::option::Option<#inner> },
            (Either::Right(_), true) => inner,
        });
    }
}

/// Generates a `#[serde(...)]` attribute for a struct field.
#[derive(Debug)]
struct SerdeFieldAttr<'view, 'a> {
    ident: &'a Ident,
    field: &'a IrStructFieldView<'view, 'a>,
}

impl<'view, 'a> SerdeFieldAttr<'view, 'a> {
    fn new(ident: &'a Ident, field: &'a IrStructFieldView<'view, 'a>) -> Self {
        Self { ident, field }
    }
}

impl ToTokens for SerdeFieldAttr<'_, '_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut attrs = Vec::new();

        // Add `flatten` xor `rename` (specifying both on the same field
        // isn't meaningful).
        if self.field.flattened() {
            attrs.push(quote! { flatten });
        } else if let &IrStructFieldName::Name(name) = &self.field.name() {
            // `rename` if the OpenAPI field name doesn't match
            // the Rust identifier.
            let f = self.ident.to_string();
            if f.strip_prefix("r#").unwrap_or(&f) != name {
                attrs.push(quote! { rename = #name });
            }
        }

        if !self.field.required() {
            // `CodegenField` always emits `AbsentOr` for optional fields.
            attrs.push(quote! { default });
            attrs.push(
                quote! { skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent" },
            );
        }

        if !attrs.is_empty() {
            tokens.append_all(quote! { #[serde(#(#attrs,)*)] });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        ir::{IrGraph, IrSpec, SchemaIrTypeView},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    use crate::CodegenGraph;

    #[test]
    fn test_struct() {
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
                    age:
                      type: integer
                      format: int32
                  required:
                    - name
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `name` is a required string field, which implements `Default`,
        // so the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Pet {
                pub name: ::std::string::String,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub age: ::ploidy_util::absent::AbsentOr<i32>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_excludes_discriminator_fields() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Animal:
                  type: object
                  properties:
                    type:
                      type: string
                    name:
                      type: string
                  required:
                    - type
                    - name
                  discriminator:
                    propertyName: type
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Animal");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Animal`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `name` is a required string field, which implements `Default`,
        // so the struct can derive `Default`. The discriminator field is excluded.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Animal {
                pub name: ::std::string::String,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_required_nullable_field_uses_option() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Record:
                  type: object
                  properties:
                    id:
                      type: string
                    deleted_at:
                      type: string
                      format: date-time
                      nullable: true
                  required:
                    - id
                    - deleted_at
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Record");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Record`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // Required nullable field uses `Option<T>`, not `AbsentOr<T>`,
        // and without `#[serde(...)]` attributes. Since both `String` and
        // `Option<T>` implement `Default`, the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Record {
                pub id: ::std::string::String,
                pub deleted_at: ::std::option::Option<::chrono::DateTime<::chrono::Utc>>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_required_nullable_field_openapi_31_syntax() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.1.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Record:
                  type: object
                  properties:
                    id:
                      type: string
                    deleted_at:
                      type: [string, 'null']
                      format: date-time
                  required:
                    - id
                    - deleted_at
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Record");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Record`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // OpenAPI 3.1 `type: [T, 'null']` syntax should behave identically to
        // OpenAPI 3.0 `nullable: true`: required nullable fields become `Option<T>`.
        // Since both `String` and `Option<T>` implement `Default`, the struct can
        // derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Record {
                pub id: ::std::string::String,
                pub deleted_at: ::std::option::Option<::chrono::DateTime<::chrono::Utc>>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_optional_nullable_field_uses_absent_or() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Record:
                  type: object
                  properties:
                    id:
                      type: string
                    deleted_at:
                      type: string
                      format: date-time
                      nullable: true
                  required:
                    - id
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Record");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Record`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // Optional nullable field uses `AbsentOr<T>` with `#[serde(...)]` attributes.
        // Since `String` implements `Default`, the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Record {
                pub id: ::std::string::String,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub deleted_at: ::ploidy_util::absent::AbsentOr<::chrono::DateTime<::chrono::Utc>>,
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: `Hash` and `Eq`

    #[test]
    fn test_struct_derives_hash_eq_when_hashable() {
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
                    active:
                      type: boolean
                  required:
                    - id
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "User");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `User`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `id` is a required string field, which implements `Default`,
        // so the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct User {
                pub id: ::std::string::String,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub active: ::ploidy_util::absent::AbsentOr<bool>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_omits_hash_eq_with_floats() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Measurement:
                  type: object
                  properties:
                    value:
                      type: number
                      format: double
                    unit:
                      type: string
                  required:
                    - value
                    - unit
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Measurement");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Measurement`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `value` and `unit` are required primitive fields. `f64` prevents `Eq`
        // and `Hash`, but both implement `Default`, so the struct can derive it.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Measurement {
                pub value: f64,
                pub unit: ::std::string::String,
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: `Default`

    #[test]
    fn test_struct_derives_default_when_all_optional() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Options:
                  type: object
                  properties:
                    verbose:
                      type: boolean
                    count:
                      type: integer
                      format: int32
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Options");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Options`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Options {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub verbose: ::ploidy_util::absent::AbsentOr<bool>,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub count: ::ploidy_util::absent::AbsentOr<i32>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_derives_default_with_nested_optional_struct() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Inner:
                  type: object
                  properties:
                    value:
                      type: string
                Outer:
                  type: object
                  properties:
                    inner:
                      $ref: '#/components/schemas/Inner'
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Outer");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Outer`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // Both `Outer` and `Inner` have all optional fields,
        // so `Default` should be derived for both.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Outer {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub inner: ::ploidy_util::absent::AbsentOr<crate::types::Inner>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_omits_default_with_required_nested_required_field() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Inner:
                  type: object
                  properties:
                    id:
                      type: string
                  required:
                    - id
                Outer:
                  type: object
                  required:
                    - inner
                  properties:
                    inner:
                      $ref: '#/components/schemas/Inner'
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Outer");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Outer`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `Outer.inner` is required, and `Inner` has a required field (`id`),
        // but `id` is a string which implements `Default`. Since all reachable
        // required fields are defaultable, `Outer` can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Outer {
                pub inner: crate::types::Inner,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_derives_default_with_optional_tagged_union() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Dog:
                  type: object
                  properties:
                    bark:
                      type: string
                Cat:
                  type: object
                  properties:
                    meow:
                      type: string
                Pet:
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                    - $ref: '#/components/schemas/Cat'
                  discriminator:
                    propertyName: type
                    mapping:
                      dog: '#/components/schemas/Dog'
                      cat: '#/components/schemas/Cat'
                Owner:
                  type: object
                  properties:
                    pet:
                      $ref: '#/components/schemas/Pet'
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Owner");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Owner`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `Pet` is a tagged union, but `Owner.pet` is optional (`AbsentOr<Pet>`),
        // which always implements `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Owner {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub pet: ::ploidy_util::absent::AbsentOr<crate::types::Pet>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_derives_default_with_optional_untagged_union() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                StringOrInt:
                  oneOf:
                    - type: string
                    - type: integer
                      format: int32
                Container:
                  type: object
                  properties:
                    value:
                      $ref: '#/components/schemas/StringOrInt'
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `StringOrInt` is an untagged union, but `Container.value` is optional
        // (`AbsentOr<StringOrInt>`), which always implements `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Container {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub value: ::ploidy_util::absent::AbsentOr<crate::types::StringOrInt>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_omits_default_with_required_tagged_union() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Dog:
                  type: object
                  properties:
                    bark:
                      type: string
                Cat:
                  type: object
                  properties:
                    meow:
                      type: string
                Pet:
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                    - $ref: '#/components/schemas/Cat'
                  discriminator:
                    propertyName: type
                    mapping:
                      dog: '#/components/schemas/Dog'
                      cat: '#/components/schemas/Cat'
                Owner:
                  type: object
                  required:
                    - pet
                  properties:
                    pet:
                      $ref: '#/components/schemas/Pet'
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Owner");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Owner`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `Pet` is a required field, so `Owner` can't derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Owner {
                pub pet: crate::types::Pet,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_derives_default_with_optional_field_to_struct_with_required() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Inner:
                  type: object
                  properties:
                    id:
                      type: string
                  required:
                    - id
                Outer:
                  type: object
                  properties:
                    inner:
                      $ref: '#/components/schemas/Inner'
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Outer");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Outer`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `Outer.inner` is optional, so `Outer` can derive `Default` even though
        // `Inner` has a required field.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Outer {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub inner: ::ploidy_util::absent::AbsentOr<crate::types::Inner>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_derives_default_with_required_any_field() {
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `data` is a required `Any` field. Since `serde_json::Value` implements
        // `Default`, the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Container {
                pub data: ::serde_json::Value,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_derives_default_with_required_primitive_fields() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Defaults:
                  type: object
                  required:
                    - text
                    - count
                    - flag
                  properties:
                    text:
                      type: string
                    count:
                      type: integer
                      format: int32
                    flag:
                      type: boolean
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Defaults");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Defaults`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // Primitives like `String`, `i32`, and `bool` implement `Default`,
        // so the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Defaults {
                pub text: ::std::string::String,
                pub count: i32,
                pub flag: bool,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_omits_default_with_required_url_field() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Resource:
                  type: object
                  required:
                    - link
                  properties:
                    link:
                      type: string
                      format: uri
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Resource");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Resource`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `Url` doesn't implement `Default`, so the struct can't derive it.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Resource {
                pub link: ::url::Url,
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: Boxing

    #[test]
    fn test_struct_boxes_recursive_required_field() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Node:
                  type: object
                  properties:
                    value:
                      type: string
                    next:
                      $ref: '#/components/schemas/Node'
                  required:
                    - value
                    - next
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Node");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Node`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `next` is required and recursive, so it should be boxed. `value` is a
        // string which implements `Default`, so the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Node {
                pub value: ::std::string::String,
                pub next: ::std::boxed::Box<crate::types::Node>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_boxes_recursive_optional_field() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Node:
                  type: object
                  properties:
                    value:
                      type: string
                    next:
                      $ref: '#/components/schemas/Node'
                  required:
                    - value
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Node");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Node`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `next` is optional and recursive. The box should be inside `AbsentOr`,
        // giving `AbsentOr<Box<Node>>`, not `Box<AbsentOr<Node>>`. `value` is a
        // string which implements `Default`, so the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Node {
                pub value: ::std::string::String,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub next: ::ploidy_util::absent::AbsentOr<::std::boxed::Box<crate::types::Node>>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_does_not_box_recursive_array_field() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Node:
                  type: object
                  properties:
                    value:
                      type: string
                    children:
                      type: array
                      items:
                        $ref: '#/components/schemas/Node'
                  required:
                    - value
                    - children
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Node");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Node`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `children` is an array of recursive elements, but arrays (`Vec`)
        // provide their own indirection, so no boxing is needed. `value` is a
        // string which implements `Default`, so the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Node {
                pub value: ::std::string::String,
                pub children: ::std::vec::Vec<crate::types::Node>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_does_not_box_optional_recursive_array_field() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Node:
                  type: object
                  properties:
                    value:
                      type: string
                    children:
                      type: array
                      items:
                        $ref: '#/components/schemas/Node'
                  required:
                    - value
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Node");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Node`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `children` is an optional array of recursive elements. Arrays provide
        // their own indirection, so no boxing is needed, despite the field
        // being optional (`AbsentOr`). `value` is a string which implements
        // `Default`, so the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Node {
                pub value: ::std::string::String,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub children: ::ploidy_util::absent::AbsentOr<::std::vec::Vec<crate::types::Node>>,
            }
        };
        assert_eq!(actual, expected);
    }
}
