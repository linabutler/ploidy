use either::Either;
use ploidy_core::{
    codegen::UniqueNameSpace,
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
    naming::CodegenTypeName,
    naming::{CodegenIdent, CodegenStructFieldName},
    ref_::CodegenRef,
};

#[derive(Clone, Copy, Debug)]
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
        let mut space = UniqueNameSpace::new();
        let fields = self
            .ty
            .fields()
            .filter(|field| !field.discriminator())
            .map(|field| {
                let field_name: Ident = match field.name() {
                    IrStructFieldName::Name(n) => {
                        let name = CodegenIdent::Field(&space.uniquify(n));
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
