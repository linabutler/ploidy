use either::Either;
use ploidy_core::{
    codegen::UniqueNames,
    ir::{
        InlineIrTypeView, IrStructFieldName, IrStructFieldView, IrStructView, IrTypeView,
        PrimitiveIrType, SchemaIrTypeView, View,
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
            .collect::<Vec<_>>();

        let mut extra_derives = vec![];
        let is_hashable = self.ty.reachable().all(|view| {
            // If this struct doesn't reach any floating-point types, then it can
            // derive `Eq` and `Hash`. (Rust doesn't define equivalence for floats).
            !matches!(
                view,
                IrTypeView::Primitive(PrimitiveIrType::F32 | PrimitiveIrType::F64)
            )
        });
        if is_hashable {
            extra_derives.push(ExtraDerive::Eq);
            extra_derives.push(ExtraDerive::Hash);
        }
        let is_defaultable = self.ty.reachable().all(|view| match view {
            IrTypeView::Schema(SchemaIrTypeView::Struct(_, ref view))
            | IrTypeView::Inline(InlineIrTypeView::Struct(_, ref view)) => {
                // If all non-discriminator fields of all reachable structs are optional,
                // then this struct can derive `Default`.
                view.fields()
                    .filter(|f| !f.discriminator())
                    .all(|f| !f.required())
            }
            // Other schema and inline types don't derive `Default`,
            // so structs that contain them can't, either.
            IrTypeView::Schema(_) | IrTypeView::Inline(_) => false,
            // All primitives implement `Default`, and wrappers
            // implement it if their containing type does, which
            // `reachable()` will also visit.
            _ => true,
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
        if matches!(
            self.field.ty(),
            IrTypeView::Array(_) | IrTypeView::Map(_) | IrTypeView::Primitive(_) | IrTypeView::Any
        ) {
            // Leaf types like primitives and `Any` don't contain any references,
            // and arrays (`Vec`) and maps (`BTreeMap`) are heap-allocated,
            // so we never need to box them.
            return false;
        }
        self.field.needs_indirection()
    }
}

impl ToTokens for CodegenField<'_, '_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        // For a nullable `T`, we emit either `Option<T>` or `AbsentOr<T>`,
        // depending on whether the field is required, while `CodegenRef`
        // always emits `Option<T>`, so we extract the inner T to avoid
        // double-wrapping.
        let inner_view = match self.field.ty() {
            IrTypeView::Nullable(nullable) => Either::Left(nullable.inner()),
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
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
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
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
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
        // and without `#[serde(...)]` attributes.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Record {
                pub id: ::std::string::String,
                pub deleted_at: ::std::option::Option<::ploidy_util::date_time::UnixMilliseconds>,
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
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Record {
                pub id: ::std::string::String,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub deleted_at: ::ploidy_util::absent::AbsentOr<::ploidy_util::date_time::UnixMilliseconds>,
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
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
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
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
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
    fn test_struct_omits_default_with_nested_required_field() {
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
        // `Inner` has a required field, so `Default` shouldn't be derived for `Outer`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Outer {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub inner: ::ploidy_util::absent::AbsentOr<crate::types::Inner>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_omits_default_with_nested_tagged_union() {
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
        // `Pet` is a tagged union that doesn't implement `Default`,
        // so neither should `Owner`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Owner {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub pet: ::ploidy_util::absent::AbsentOr<crate::types::Pet>,
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_omits_default_with_nested_untagged_union() {
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
        // `StringOrInt` is an untagged union that doesn't implement `Default`,
        // so neither should `Container`.
        let expected: syn::ItemStruct = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Container {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub value: ::ploidy_util::absent::AbsentOr<crate::types::StringOrInt>,
            }
        };
        assert_eq!(actual, expected);
    }
}
