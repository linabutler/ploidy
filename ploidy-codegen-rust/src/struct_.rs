use itertools::Itertools;
use ploidy_core::{
    codegen::UniqueNames,
    ir::{Required, StructFieldName, StructFieldView, StructView, View},
};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    derives::ExtraDerive,
    doc_attrs,
    ext::FieldViewExt,
    naming::{CodegenIdentRef, CodegenIdentScope, CodegenIdentUsage, CodegenTypeName},
    ref_::CodegenRef,
};

#[derive(Clone, Debug)]
pub struct CodegenStruct<'a> {
    name: CodegenTypeName<'a>,
    ty: &'a StructView<'a, 'a>,
}

impl<'a> CodegenStruct<'a> {
    pub fn new(name: CodegenTypeName<'a>, ty: &'a StructView<'a, 'a>) -> Self {
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
            .filter(|field| !field.tag())
            .map(|field| {
                let doc_attrs = field.description().map(doc_attrs);

                let name = match field.name() {
                    StructFieldName::Name(n) => scope.uniquify(n),
                    StructFieldName::Hint(hint) => {
                        scope.uniquify_ident(&CodegenIdentRef::from_field_name_hint(hint))
                    }
                };
                let field_name = CodegenIdentUsage::Field(&name);
                let field_attrs = StructFieldAttrs::new(field_name, &field);
                let ty = CodegenField::new(&field);

                quote! {
                    #doc_attrs
                    #field_attrs
                    pub #field_name: #ty,
                }
            })
            .collect_vec();

        let mut extra_derives = vec![];

        // Derive `Eq` and `Hash` if all fields are transitively hashable.
        let all_hashable = self.ty.hashable();
        if all_hashable {
            extra_derives.push(ExtraDerive::Eq);
            extra_derives.push(ExtraDerive::Hash);
        }

        // Derive `Default` if all fields are transitively defaultable.
        if self.ty.defaultable() {
            extra_derives.push(ExtraDerive::Default);
        }

        let type_name = &self.name;
        let doc_attrs = self.ty.description().map(doc_attrs);

        tokens.append_all(quote! {
            #doc_attrs
            #[derive(Debug, Clone, PartialEq, #(#extra_derives,)* ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct #type_name {
                #(#fields)*
            }
        });
    }
}

/// A field in a struct, ready for code generation.
#[derive(Debug)]
struct CodegenField<'view, 'a> {
    field: &'a StructFieldView<'view, 'a, 'a>,
}

impl<'view, 'a> CodegenField<'view, 'a> {
    fn new(field: &'a StructFieldView<'view, 'a, 'a>) -> Self {
        Self { field }
    }
}

impl ToTokens for CodegenField<'_, '_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let ty = self.field.inner();
        let ref_ = CodegenRef::new(&ty);
        let boxed = if self.field.needs_box() {
            quote! { ::std::boxed::Box<#ref_> }
        } else {
            quote! { #ref_ }
        };

        tokens.append_all(match self.field.required() {
            Required::Optional => quote! { ::ploidy_util::absent::AbsentOr<#boxed> },
            Required::Required { nullable: true } => quote! { ::std::option::Option<#boxed> },
            Required::Required { nullable: false } => boxed,
        });
    }
}

/// Generates `#[serde(...)]` and `#[ploidy(pointer(...))]` attributes
/// for a struct field.
#[derive(Debug)]
struct StructFieldAttrs<'view, 'a> {
    field_name: CodegenIdentUsage<'a>,
    field: &'a StructFieldView<'view, 'a, 'a>,
}

impl<'view, 'a> StructFieldAttrs<'view, 'a> {
    fn new(field_name: CodegenIdentUsage<'a>, field: &'a StructFieldView<'view, 'a, 'a>) -> Self {
        Self { field_name, field }
    }
}

impl ToTokens for StructFieldAttrs<'_, '_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let serde = {
            let mut meta = vec![];

            // Add `flatten` xor `rename` (specifying both on the same field
            // isn't meaningful).
            if self.field.flattened() {
                meta.push(quote! { flatten });
            } else if let &StructFieldName::Name(name) = &self.field.name() {
                // `rename` if the OpenAPI field name doesn't match
                // the Rust identifier.
                if self.field_name.display().to_string() != name {
                    meta.push(quote! { rename = #name });
                }
            }

            if matches!(self.field.required(), Required::Optional) {
                meta.push(quote! { default });
                meta.push(
                    quote! { skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent" },
                );
            }

            if meta.is_empty() {
                quote! {}
            } else {
                quote! { #[serde(#(#meta),*)] }
            }
        };

        let pointer = {
            let mut meta = vec![];

            if self.field.flattened() {
                meta.push(quote! { flatten });
            } else if let &StructFieldName::Name(name) = &self.field.name()
                && self.field_name.display().to_string() != name
            {
                meta.push(quote! { rename = #name });
            }

            if meta.is_empty() {
                quote! {}
            } else {
                quote! { #[ploidy(pointer(#(#meta),*))] }
            }
        };

        tokens.append_all(serde);
        tokens.append_all(pointer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        arena::Arena,
        ir::{RawGraph, SchemaTypeView, Spec},
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Pet").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `name` is a required string field, which implements `Default`,
        // so the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Pet {
                pub name: ::std::string::String,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub age: ::ploidy_util::absent::AbsentOr<i32>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_excludes_tag_fields() {
        // `Animal` is only used inside the `Pet` tagged union, so it's
        // not inlined and the tag field (`type`) is excluded.
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
                Pet:
                  oneOf:
                    - $ref: '#/components/schemas/Animal'
                  discriminator:
                    propertyName: type
                    mapping:
                      animal: '#/components/schemas/Animal'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Animal").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Animal`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `name` is a required string field, which implements `Default`,
        // so the struct can derive `Default`. The tag field is excluded.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Record").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Record`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // Required nullable field uses `Option<T>`, not `AbsentOr<T>`,
        // and without `#[serde(...)]` attributes. Since both `String` and
        // `Option<T>` implement `Default`, the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Record {
                pub id: ::std::string::String,
                pub deleted_at: ::std::option::Option<::ploidy_util::chrono::DateTime<::ploidy_util::chrono::Utc>>,
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Record").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
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
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Record {
                pub id: ::std::string::String,
                pub deleted_at: ::std::option::Option<::ploidy_util::chrono::DateTime<::ploidy_util::chrono::Utc>>,
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Record").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Record`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // Optional nullable field uses `AbsentOr<T>` with `#[serde(...)]` attributes.
        // Since `String` implements `Default`, the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Record {
                pub id: ::std::string::String,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub deleted_at: ::ploidy_util::absent::AbsentOr<::ploidy_util::chrono::DateTime<::ploidy_util::chrono::Utc>>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_optional_field_referencing_nullable_schema_unwraps() {
        // A field that references a named nullable schema (like `NullableString`)
        // should unwrap the inner type to avoid `AbsentOr<Option<T>>`. The
        // `AbsentOr` type already has `Absent`, `Null`, and `Present(T)` variants,
        // so wrapping `Option<T>` would create redundant representations for null.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.1.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                NullableString:
                  type:
                    - string
                    - 'null'
                Record:
                  type: object
                  properties:
                    nickname:
                      $ref: '#/components/schemas/NullableString'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Record").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Record`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // The field should be `AbsentOr<String>`, not `AbsentOr<NullableString>`
        // (which would be `AbsentOr<Option<String>>`).
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Record {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub nickname: ::ploidy_util::absent::AbsentOr<::std::string::String>,
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("User").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `User`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `id` is a required string field, which implements `Default`,
        // so the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct User {
                pub id: ::std::string::String,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Measurement").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Measurement`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `value` and `unit` are required primitive fields. `f64` prevents `Eq`
        // and `Hash`, but both implement `Default`, so the struct can derive it.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Measurement {
                pub value: f64,
                pub unit: ::std::string::String,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_derives_hash_eq_despite_inheriting_from_unhashable_tagged_union() {
        // `TextAction` inherits from tagged union `Action`, and has
        // all hashable fields; it should still derive `Eq` and `Hash`
        // despite its sibling `MetricAction` having unhashable (`f64`) fields.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                TextAction:
                  type: object
                  allOf:
                    - $ref: '#/components/schemas/Action'
                  properties:
                    label:
                      type: string
                  required:
                    - label
                MetricAction:
                  type: object
                  properties:
                    score:
                      type: number
                      format: double
                  required:
                    - score
                Action:
                  oneOf:
                    - $ref: '#/components/schemas/TextAction'
                    - $ref: '#/components/schemas/MetricAction'
                  discriminator:
                    propertyName: type
                    mapping:
                      text: '#/components/schemas/TextAction'
                      metric: '#/components/schemas/MetricAction'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("TextAction").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `TextAction`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct TextAction {
                pub label: ::std::string::String,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_derives_hash_eq_despite_inheriting_from_unhashable_untagged_union() {
        // `TextAction` inherits from untagged union `Action`, and has
        // all hashable fields; it should still derive `Eq` and `Hash`
        // despite its sibling `MetricAction` having unhashable (`f64`) fields.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                TextAction:
                  type: object
                  allOf:
                    - $ref: '#/components/schemas/Action'
                  properties:
                    label:
                      type: string
                  required:
                    - label
                MetricAction:
                  type: object
                  properties:
                    score:
                      type: number
                      format: double
                  required:
                    - score
                Action:
                  oneOf:
                    - $ref: '#/components/schemas/TextAction'
                    - $ref: '#/components/schemas/MetricAction'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("TextAction").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `TextAction`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct TextAction {
                pub label: ::std::string::String,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_omits_hash_eq_when_inheriting_unhashable_field_from_tagged_union() {
        // `TextAction` inherits from tagged union `Action`, which declares
        // common field `score: f64`, so neither `Action` nor `TextAction`
        // can derive `Eq` or `Hash`.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                TextAction:
                  type: object
                  allOf:
                    - $ref: '#/components/schemas/Action'
                  properties:
                    label:
                      type: string
                  required:
                    - label
                MetricAction:
                  type: object
                  properties:
                    value:
                      type: string
                  required:
                    - value
                Action:
                  oneOf:
                    - $ref: '#/components/schemas/TextAction'
                    - $ref: '#/components/schemas/MetricAction'
                  discriminator:
                    propertyName: type
                    mapping:
                      text: '#/components/schemas/TextAction'
                      metric: '#/components/schemas/MetricAction'
                  properties:
                    type:
                      type: string
                    score:
                      type: number
                      format: double
                  required:
                    - type
                    - score
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("TextAction").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `TextAction`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct TextAction {
                pub score: f64,
                pub label: ::std::string::String,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_omits_hash_eq_when_inheriting_unhashable_field_from_untagged_union() {
        // `TextAction` inherits from untagged union `Action`, which declares
        // common field `score: f64`, so neither `Action` nor `TextAction`
        // can derive `Eq` or `Hash`.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                TextAction:
                  type: object
                  allOf:
                    - $ref: '#/components/schemas/Action'
                  properties:
                    label:
                      type: string
                  required:
                    - label
                MetricAction:
                  type: object
                  properties:
                    value:
                      type: string
                  required:
                    - value
                Action:
                  oneOf:
                    - $ref: '#/components/schemas/TextAction'
                    - $ref: '#/components/schemas/MetricAction'
                  properties:
                    score:
                      type: number
                      format: double
                  required:
                    - score
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("TextAction").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `TextAction`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct TextAction {
                pub score: f64,
                pub label: ::std::string::String,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_derives_hash_eq_when_union_common_field_inherits_from_unhashable_union() {
        // `TextAction` inherits from tagged union `Action`, which declares
        // common field `metadata: ActionMetadata`. `ActionMetadata`
        // inherits from a different tagged union `MetadataKind`, whose
        // variant `NumericMetadata` has `f64`.
        //
        // `TextAction` can derive `Eq` and `Hash` because neither it nor
        // `ActionMetadata` directly contains `f64`. `NumericMetadata` is
        // a sibling variant of `MetadataKind`, not an ancestor.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                TextAction:
                  type: object
                  allOf:
                    - $ref: '#/components/schemas/Action'
                  properties:
                    label:
                      type: string
                  required:
                    - label
                MetricAction:
                  type: object
                  properties:
                    value:
                      type: string
                  required:
                    - value
                Action:
                  oneOf:
                    - $ref: '#/components/schemas/TextAction'
                    - $ref: '#/components/schemas/MetricAction'
                  discriminator:
                    propertyName: type
                    mapping:
                      text: '#/components/schemas/TextAction'
                      metric: '#/components/schemas/MetricAction'
                  properties:
                    metadata:
                      $ref: '#/components/schemas/ActionMetadata'
                  required:
                    - type
                    - metadata
                ActionMetadata:
                  type: object
                  allOf:
                    - $ref: '#/components/schemas/MetadataKind'
                  properties:
                    timestamp:
                      type: string
                  required:
                    - timestamp
                NumericMetadata:
                  type: object
                  properties:
                    score:
                      type: number
                      format: double
                  required:
                    - score
                MetadataKind:
                  oneOf:
                    - $ref: '#/components/schemas/ActionMetadata'
                    - $ref: '#/components/schemas/NumericMetadata'
                  discriminator:
                    propertyName: kind
                    mapping:
                      action: '#/components/schemas/ActionMetadata'
                      numeric: '#/components/schemas/NumericMetadata'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("TextAction").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `TextAction`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct TextAction {
                pub metadata: crate::types::ActionMetadata,
                pub label: ::std::string::String,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_omits_hash_eq_when_inheriting_unhashable_field_through_cycle() {
        // Struct `N` inherits from struct `A`, which declares field `t: T`.
        // `T` has `val: f64` (unhashable) and `ns: Vec<N>`, closing the cycle.
        //
        //   N --Inherits--> A --Field--> T --Field--> [N] --Contains--> N
        //                                 \--Field--> f64
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                A:
                  type: object
                  properties:
                    t:
                      $ref: '#/components/schemas/T'
                  required:
                    - t
                N:
                  type: object
                  allOf:
                    - $ref: '#/components/schemas/A'
                  properties:
                    name:
                      type: string
                  required:
                    - name
                T:
                  type: object
                  properties:
                    ns:
                      type: array
                      items:
                        $ref: '#/components/schemas/N'
                    val:
                      type: number
                      format: double
                  required:
                    - ns
                    - val
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("N").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `N`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct N {
                pub t: crate::types::T,
                pub name: ::std::string::String,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_omits_hash_eq_for_scc_sibling_of_inherited_unhashable() {
        // `X` has field `y: Y`. `Y` inherits `P`, which has fields
        // `v: f64`. Two SCCs: `{X, Y}` and `{P}`.
        //
        //   X --Field--> Y --Inherits--> P --Field--> f64
        //   Y --Field--> X
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                X:
                  type: object
                  properties:
                    y:
                      $ref: '#/components/schemas/Y'
                  required:
                    - y
                Y:
                  type: object
                  allOf:
                    - $ref: '#/components/schemas/P'
                  properties:
                    x:
                      $ref: '#/components/schemas/X'
                  required:
                    - x
                P:
                  type: object
                  properties:
                    v:
                      type: number
                      format: double
                  required:
                    - v
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("X").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `X`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct X {
                pub y: ::std::boxed::Box<crate::types::Y>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_omits_hash_eq_for_non_inheriting_scc_member() {
        // `A` inherits `B`. `B` has field `d: D`. `D` inherits `E`
        // with fields `f: f64` and `as: [A]`. One SCC: `{A, B, D}`.
        //
        //   A --Inherits--> B --Field--> D --Inherits--> E --Field--> f64
        //                                D --Field--> [A]
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                A:
                  type: object
                  allOf:
                    - $ref: '#/components/schemas/B'
                B:
                  type: object
                  properties:
                    d:
                      $ref: '#/components/schemas/D'
                  required:
                    - d
                D:
                  type: object
                  allOf:
                    - $ref: '#/components/schemas/E'
                  properties:
                    as:
                      type: array
                      items:
                        $ref: '#/components/schemas/A'
                  required:
                    - as
                E:
                  type: object
                  properties:
                    f:
                      type: number
                      format: double
                  required:
                    - f
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("B").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `B`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct B {
                pub d: crate::types::D,
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: `Default`

    #[test]
    fn test_struct_omits_default_when_inheriting_undefaultable_field_through_cycle() {
        // `N` inherits `A`, which has required field `link: Url`
        // (undefaultable). `N` has required field `t: T`, and `T` has
        // required field `n: N`, closing the cycle. Two SCCs:
        // `{N, T}` and `{A}`.
        //
        //   N --Inherits--> A --Field--> Url
        //   N --Field--> T --Field--> N
        //
        // None of `A`, `N`, `T` should derive `Default`.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                A:
                  type: object
                  properties:
                    link:
                      type: string
                      format: uri
                  required:
                    - link
                N:
                  type: object
                  allOf:
                    - $ref: '#/components/schemas/A'
                  properties:
                    t:
                      $ref: '#/components/schemas/T'
                  required:
                    - t
                T:
                  type: object
                  properties:
                    n:
                      $ref: '#/components/schemas/N'
                  required:
                    - n
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let actual: syn::File = syn::parse2(
            graph
                .schemas()
                .filter(|s| matches!(s.name(), "A" | "N" | "T"))
                .map(|schema| {
                    let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
                        panic!("expected struct; got `{schema:?}`");
                    };
                    let name = CodegenTypeName::Schema(schema);
                    let codegen = CodegenStruct::new(name, struct_view);
                    quote!(#codegen)
                })
                .reduce(|a, b| quote! { #a #b })
                .unwrap(),
        )
        .unwrap();
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct A {
                pub link: ::ploidy_util::url::Url,
            }

            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct N {
                pub link: ::ploidy_util::url::Url,
                pub t: ::std::boxed::Box<crate::types::T>,
            }

            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct T {
                pub n: ::std::boxed::Box<crate::types::N>,
            }
        };
        assert_eq!(actual, expected);
    }

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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Options").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Options`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Options {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub verbose: ::ploidy_util::absent::AbsentOr<bool>,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Outer").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Outer`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // Both `Outer` and `Inner` have all optional fields,
        // so `Default` should be derived for both.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Outer {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Outer").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Outer`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `Outer.inner` is required, and `Inner` has a required field (`id`),
        // but `id` is a string which implements `Default`. Since all reachable
        // required fields are defaultable, `Outer` can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Owner").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Owner`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `Pet` is a tagged union, but `Owner.pet` is optional (`AbsentOr<Pet>`),
        // which always implements `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Owner {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `StringOrInt` is an untagged union, but `Container.value` is optional
        // (`AbsentOr<StringOrInt>`), which always implements `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Container {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Owner").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Owner`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `Pet` is a required field, so `Owner` can't derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Outer").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Outer`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `Outer.inner` is optional, so `Outer` can derive `Default` even though
        // `Inner` has a required field.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Outer {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `data` is a required `Any` field. Since `serde_json::Value` implements
        // `Default`, the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Container {
                pub data: ::ploidy_util::serde_json::Value,
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Defaults").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Defaults`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // Primitives like `String`, `i32`, and `bool` implement `Default`,
        // so the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Resource").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Resource`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `Url` doesn't implement `Default`, so the struct can't derive it.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Resource {
                pub link: ::ploidy_util::url::Url,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_derives_default_with_optional_url_field() {
        // Even though `Url` doesn't implement `Default`, an optional `Url`
        // field becomes `AbsentOr<Url>`, which is unconditionally `Default`.
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
                  properties:
                    link:
                      type: string
                      format: uri
                    name:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Resource").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Resource`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Resource {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub link: ::ploidy_util::absent::AbsentOr<::ploidy_util::url::Url>,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub name: ::ploidy_util::absent::AbsentOr<::std::string::String>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_derives_default_with_required_container_schema_field() {
        // A required field that references a container schema should still allow
        // the struct to derive `Default`, since `Vec<T>` implements `Default`.
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
                Container:
                  type: object
                  required:
                    - tags
                  properties:
                    tags:
                      $ref: '#/components/schemas/Tags'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `Tags` is a type alias for `Vec<String>`, which implements `Default`,
        // so the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Container {
                pub tags: crate::types::Tags,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_derives_default_when_inheriting_from_tagged_union() {
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
                Animal:
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                    - $ref: '#/components/schemas/Cat'
                  discriminator:
                    propertyName: type
                    mapping:
                      dog: '#/components/schemas/Dog'
                      cat: '#/components/schemas/Cat'
                  properties:
                    type:
                      type: string
                  required:
                    - type
                Corgi:
                  allOf:
                    - $ref: '#/components/schemas/Animal'
                    - type: object
                      properties:
                        name:
                          type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Corgi").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Corgi`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `Corgi` inherits from the tagged union `Animal` via `allOf`.
        // `Animal` has `type` as a required common field alongside its
        // `oneOf` discriminator; `Corgi` inherits it. Both `String` and
        // `AbsentOr` implement `Default`, so `Default` is still derived.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Corgi {
                pub r#type: ::std::string::String,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub name: ::ploidy_util::absent::AbsentOr<::std::string::String>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_excludes_default_when_inheriting_non_defaultable_from_tagged_union() {
        // `Base` is a tagged union with a non-defaultable `source` field.
        // `Child` inherits from `Base` and has only optional own fields,
        // but can't derive `Default` thanks to the inherited `source`.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                TypeA:
                  type: object
                  properties:
                    value:
                      type: string
                TypeB:
                  type: object
                  properties:
                    count:
                      type: integer
                Base:
                  oneOf:
                    - $ref: '#/components/schemas/TypeA'
                    - $ref: '#/components/schemas/TypeB'
                  discriminator:
                    propertyName: kind
                    mapping:
                      a: '#/components/schemas/TypeA'
                      b: '#/components/schemas/TypeB'
                  properties:
                    kind:
                      type: string
                    source:
                      type: string
                      format: uri
                  required:
                    - kind
                    - source
                Child:
                  allOf:
                    - $ref: '#/components/schemas/Base'
                    - type: object
                      properties:
                        name:
                          type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Child").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Child`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `Child` inherits non-defaultable `source` from `Base`.
        // `Url` doesn't implement `Default`, so `Child` can't derive it,
        // even though `Child`'s own `name` field is optional.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Child {
                pub kind: ::std::string::String,
                pub source: ::ploidy_util::url::Url,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub name: ::ploidy_util::absent::AbsentOr<::std::string::String>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_derives_default_despite_inheriting_from_non_defaultable_tagged_union() {
        // `TextAction` inherits from tagged union `Action`, and has
        // all defaultable fields; it should still derive `Default`
        // despite its sibling `LinkAction` having non-defaultable
        // (`Url`) fields.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                TextAction:
                  type: object
                  allOf:
                    - $ref: '#/components/schemas/Action'
                  properties:
                    label:
                      type: string
                LinkAction:
                  type: object
                  properties:
                    url:
                      type: string
                      format: uri
                  required:
                    - url
                Action:
                  oneOf:
                    - $ref: '#/components/schemas/TextAction'
                    - $ref: '#/components/schemas/LinkAction'
                  discriminator:
                    propertyName: type
                    mapping:
                      text: '#/components/schemas/TextAction'
                      link: '#/components/schemas/LinkAction'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("TextAction").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `TextAction`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct TextAction {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub label: ::ploidy_util::absent::AbsentOr<::std::string::String>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_derives_default_despite_inheriting_from_non_defaultable_untagged_union() {
        // `TextAction` inherits from untagged union `Action`, and has
        // all defaultable fields; it should still derive `Default`
        // despite its sibling `LinkAction` having non-defaultable
        // (`Url`) fields.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                TextAction:
                  type: object
                  allOf:
                    - $ref: '#/components/schemas/Action'
                  properties:
                    label:
                      type: string
                LinkAction:
                  type: object
                  properties:
                    url:
                      type: string
                      format: uri
                  required:
                    - url
                Action:
                  oneOf:
                    - $ref: '#/components/schemas/TextAction'
                    - $ref: '#/components/schemas/LinkAction'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("TextAction").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `TextAction`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct TextAction {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub label: ::ploidy_util::absent::AbsentOr<::std::string::String>,
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Node").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Node`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `next` is required and recursive, so it should be boxed.
        // `value` is a string which implements `Default`, so the struct can
        // derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Node").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Node`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `next` is optional and recursive. The box should be inside `AbsentOr`,
        // giving `AbsentOr<Box<Node>>`, not `Box<AbsentOr<Node>>`. `value` is a
        // string which implements `Default`, so the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Node {
                pub value: ::std::string::String,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Node").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Node`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // `children` is an array of recursive elements, but arrays (`Vec`)
        // provide their own indirection, so no boxing is needed. `value` is a
        // string which implements `Default`, so the struct can derive `Default`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Node").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
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
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Node {
                pub value: ::std::string::String,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub children: ::ploidy_util::absent::AbsentOr<::std::vec::Vec<crate::types::Node>>,
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: Inheritance

    #[test]
    fn test_struct_linearizes_inline_all_of_parent_fields() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Person:
                  allOf:
                    - type: object
                      properties:
                        name:
                          type: string
                      required:
                        - name
                    - type: object
                      properties:
                        age:
                          type: integer
                          format: int32
                      required:
                        - age
                  properties:
                    email:
                      type: string
                  required:
                    - email
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Person").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Person`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // Inherited fields from inline `allOf` parents should appear first
        // in declaration order, followed by the struct's own fields.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Person {
                pub name: ::std::string::String,
                pub age: i32,
                pub email: ::std::string::String,
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: Additional properties

    #[test]
    fn test_struct_with_additional_properties() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Config:
                  type: object
                  properties:
                    name:
                      type: string
                  required:
                    - name
                  additionalProperties:
                    type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Config").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Config`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Config {
                pub name: ::std::string::String,
                #[serde(flatten)]
                #[ploidy(pointer(flatten))]
                pub additional_properties: ::std::collections::BTreeMap<::std::string::String, ::std::string::String>,
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: Inlined struct variants of tagged unions

    #[test]
    fn test_inlined_struct_includes_tag() {
        // `Dog` is both a variant of `Pet` tagged union _and_ referenced by
        // `Owner.dog`, making it inlinable. The tag field `kind` should
        // be included as a regular field on the struct.
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
                    kind:
                      type: string
                    bark:
                      type: string
                  required:
                    - kind
                    - bark
                Pet:
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                  discriminator:
                    propertyName: kind
                    mapping:
                      dog: '#/components/schemas/Dog'
                Owner:
                  type: object
                  properties:
                    dog:
                      $ref: '#/components/schemas/Dog'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let mut raw = RawGraph::new(&arena, &spec);
        raw.inline_tagged_variants();
        let graph = CodegenGraph::new(raw.cook());

        let schema = graph.schema("Dog").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Dog`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        // Both `kind` and `bark` should be present. After inlining, the
        // tagged union no longer references `Dog` directly, so `kind`
        // is not treated as a tag.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Dog {
                pub kind: ::std::string::String,
                pub bark: ::std::string::String,
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: Enum fields

    #[test]
    fn test_struct_required_enum_field() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Status:
                  type: string
                  enum:
                    - active
                    - inactive
                Pet:
                  type: object
                  required:
                    - name
                    - status
                  properties:
                    name:
                      type: string
                    status:
                      $ref: '#/components/schemas/Status'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Pet").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Pet {
                pub name: ::std::string::String,
                pub status: crate::types::Status,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_optional_enum_field_uses_absent() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Status:
                  type: string
                  enum:
                    - active
                    - inactive
                Pet:
                  type: object
                  required:
                    - name
                  properties:
                    name:
                      type: string
                    status:
                      $ref: '#/components/schemas/Status'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Pet").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Pet {
                pub name: ::std::string::String,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub status: ::ploidy_util::absent::AbsentOr<crate::types::Status>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_required_inline_enum_field() {
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
                  required:
                    - name
                    - status
                  properties:
                    name:
                      type: string
                    status:
                      type: string
                      enum:
                        - active
                        - inactive
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Pet").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Pet {
                pub name: ::std::string::String,
                pub status: crate::types::pet::types::Status,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_required_nullable_enum_field_uses_option() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Status:
                  type: string
                  enum:
                    - active
                    - inactive
                  nullable: true
                Pet:
                  type: object
                  required:
                    - name
                    - status
                  properties:
                    name:
                      type: string
                    status:
                      $ref: '#/components/schemas/Status'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Pet").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        // Required nullable enum fields become `Option<T>` without
        // `skip_serializing_if`, since their type is
        // `Container::Optional`, not `Enum`.
        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Pet {
                pub name: ::std::string::String,
                pub status: ::std::option::Option<crate::types::Status>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_deduplicates_additional_properties_collision() {
        // When a struct has both an own property named `additionalProperties`
        // _and_ an `additionalProperties` schema, the hint field should be
        // uniquified to avoid collision. The named property claims the
        // unsuffixed name, since own properties take precedence over
        // additional properties in the IR.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Config:
                  type: object
                  additionalProperties:
                    type: string
                  properties:
                    additionalProperties:
                      type: boolean
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Config").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Config`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Config {
                #[serde(rename = "additionalProperties", default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                #[ploidy(pointer(rename = "additionalProperties"))]
                pub additional_properties: ::ploidy_util::absent::AbsentOr<bool>,
                #[serde(flatten)]
                #[ploidy(pointer(flatten))]
                pub additional_properties2: ::std::collections::BTreeMap<::std::string::String, ::std::string::String>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_required_unrepresentable_enum_field_no_skip() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Priority:
                  type: integer
                  enum:
                    - 1
                    - 2
                    - 3
                Pet:
                  type: object
                  required:
                    - name
                    - priority
                  properties:
                    name:
                      type: string
                    priority:
                      $ref: '#/components/schemas/Priority'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Pet").unwrap();
        let schema @ SchemaTypeView::Struct(_, struct_view) = &schema else {
            panic!("expected struct `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenStruct::new(name, struct_view);

        // Unrepresentable enums become `String` type aliases,
        // so no `skip_serializing_if` is added.
        let actual: syn::ItemStruct = parse_quote!(#codegen);
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Pet {
                pub name: ::std::string::String,
                pub priority: crate::types::Priority,
            }
        };
        assert_eq!(actual, expected);
    }
}
