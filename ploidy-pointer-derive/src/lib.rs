//! Derive macro for the [`JsonPointee`] trait.
//!
//! This crate provides a derive macro to generate a [`JsonPointee`]
//! implementation for a Rust data structure. The macro can generate implementations for
//! structs and enums, and supports [Serde][serde]-like attributes.
//!
//! # Container attributes
//!
//! Container-level attributes apply to structs and enums:
//!
//! * `#[ploidy(tag = "field")]` - Use the internally tagged enum representation,
//!   with the given field name for the tag. Supported on enums only.
//! * `#[ploidy(tag = "t", content = "c")]` - Use the adjacently tagged enum representation,
//!   with the given field names for the tag and contents. Supported on enums only.
//! * `#[ploidy(untagged)]` - Use the untagged enum representation. Supported on enums only.
//! * `#[ploidy(rename_all = "case")]` - Rename all struct fields or enum variants
//!   according to the given case. The supported cases are `lowercase`, `UPPERCASE`,
//!   `PascalCase`, `camelCase`, `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case`, and
//!   `SCREAMING-KEBAB-CASE`.
//!
//! # Variant Attributes
//!
//! Variant-level attributes apply to enum variants:
//!
//! * `#[ploidy(rename = "name")]` - Access this variant using the given name,
//!   instead of its Rust name.
//! * `#[ploidy(skip)]` - Make this variant inaccessible, except for the tag field
//!   if using the internally or adjacently tagged enum representation.
//!
//! # Field Attributes
//!
//! Field-level attributes apply to struct and enum variant fields:
//!
//! * `#[ploidy(rename = "name")]` - Access this variant using the given name,
//!   instead of its Rust name.
//! * `#[ploidy(flatten)]` - Remove one layer of structure between the container
//!   and field. Supported on named fields only.
//! * `#[ploidy(skip)]` - Exclude the field from pointer access.
//!
//! # Examples
//!
//! ## Struct flattening
//!
//! ```ignore
//! # use ploidy_pointer::{BadJsonPointer, JsonPointee, JsonPointer};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! #[derive(JsonPointee)]
//! struct User {
//!     name: String,
//!     #[ploidy(flatten)]
//!     contact: ContactInfo,
//! }
//!
//! #[derive(JsonPointee)]
//! struct ContactInfo {
//!     email: String,
//!     phone: String,
//! }
//!
//! let user = User {
//!     name: "Alice".to_owned(),
//!     contact: ContactInfo {
//!         email: "a@example.com".to_owned(),
//!         phone: "555-1234".to_owned(),
//!     },
//! };
//! assert_eq!(
//!     user.resolve(JsonPointer::parse("/name")?)?.downcast_ref::<String>(),
//!     Some(&"Alice".to_owned()),
//! );
//! assert_eq!(
//!     user.resolve(JsonPointer::parse("/email")?)?.downcast_ref::<String>(),
//!     Some(&"a@example.com".to_owned()),
//! );
//! assert_eq!(
//!     user.resolve(JsonPointer::parse("/phone")?)?.downcast_ref::<String>(),
//!     Some(&"555-1234".to_owned()),
//! );
//! # Ok(())
//! # }
//! ```
//!
//! ## Renaming fields
//!
//! ```ignore
//! # use ploidy_pointer::{BadJsonPointer, JsonPointee, JsonPointer};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! #[derive(JsonPointee)]
//! #[ploidy(rename_all = "snake_case")]
//! enum ApiResponse {
//!     SuccessResponse { data: String },
//!     #[ploidy(rename = "error")]
//!     ErrorResponse { message: String },
//! }
//!
//! let success = ApiResponse::SuccessResponse {
//!     data: "ok".to_owned(),
//! };
//! assert_eq!(
//!     success.resolve(JsonPointer::parse("/success_response/data")?)?.downcast_ref::<String>(),
//!     Some(&"ok".to_owned()),
//! );
//!
//! let error = ApiResponse::ErrorResponse {
//!     message: "failed".to_owned(),
//! };
//! assert_eq!(
//!     error.resolve(JsonPointer::parse("/error/message")?)?.downcast_ref::<String>(),
//!     Some(&"failed".to_owned()),
//! );
//! # Ok(())
//! # }
//! ```
//!
//! # Enum representations
//!
//! Like Serde, `#[derive(JsonPointee)]` supports externally tagged,
//! internally tagged, adjacently tagged, and untagged enum representations.
//!
//! ## Externally tagged
//!
//! This is the default enum representation. The variant's tag wraps the contents.
//!
//! ```ignore
//! # use ploidy_pointer::{BadJsonPointer, JsonPointee, JsonPointer};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! #[derive(JsonPointee)]
//! enum Message {
//!     Text { content: String },
//!     Image { url: String },
//! }
//!
//! let message = Message::Text {
//!     content: "hello".to_owned(),
//! };
//! assert_eq!(
//!     message.resolve(JsonPointer::parse("/Text/content")?)?.downcast_ref::<String>(),
//!     Some(&"hello".to_owned()),
//! );
//! # Ok(())
//! # }
//! ```
//!
//! ## Internally tagged
//!
//! In this representation, the tag that specifies the variant name
//! is next to the variant's fields.
//!
//! ```ignore
//! # use ploidy_pointer::{BadJsonPointer, JsonPointee, JsonPointer};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! #[derive(JsonPointee)]
//! #[ploidy(tag = "type")]
//! enum Message {
//!     Text { content: String },
//!     Image { url: String },
//! }
//!
//! let message = Message::Text {
//!     content: "hello".to_owned(),
//! };
//! assert_eq!(
//!     message.resolve(JsonPointer::parse("/type")?)?.downcast_ref::<&str>(),
//!     Some(&"Text"),
//! );
//! assert_eq!(
//!     message.resolve(JsonPointer::parse("/content")?)?.downcast_ref::<String>(),
//!     Some(&"hello".to_owned()),
//! );
//! # Ok(())
//! # }
//! ```
//!
//! ## Adjacently tagged
//!
//! In this representation, the variant's tag and contents are adjacent
//! to each other, as two fields in the same object.
//!
//! ```ignore
//! # use ploidy_pointer::{BadJsonPointer, JsonPointee, JsonPointer};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! #[derive(JsonPointee)]
//! #[ploidy(tag = "type", content = "value")]
//! enum Message {
//!     Text { content: String },
//!     Image { url: String },
//! }
//!
//! let message = Message::Text {
//!     content: "hello".to_owned(),
//! };
//! assert_eq!(
//!     message.resolve(JsonPointer::parse("/type")?)?.downcast_ref::<&str>(),
//!     Some(&"Text"),
//! );
//! assert_eq!(
//!     message.resolve(JsonPointer::parse("/value/content")?)?.downcast_ref::<String>(),
//!     Some(&"hello".to_owned()),
//! );
//! # Ok(())
//! # }
//! ```
//!
//! ## Untagged
//!
//! In this representation, the variant's name is completely ignored,
//! and pointers are resolved against the variant's contents.
//!
//! ```ignore
//! # use ploidy_pointer::{BadJsonPointer, JsonPointee, JsonPointer};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! #[derive(JsonPointee)]
//! #[ploidy(untagged)]
//! enum Message {
//!     Text { content: String },
//!     Image { url: String },
//! }
//!
//! let message = Message::Text {
//!     content: "hello".to_owned(),
//! };
//! assert_eq!(
//!     message.resolve(JsonPointer::parse("/content")?)?.downcast_ref::<String>(),
//!     Some(&"hello".to_owned()),
//! );
//! # Ok(())
//! # }
//! ```
//!
//! [serde]: https://serde.rs

use std::fmt::Display;

use heck::{
    ToKebabCase, ToLowerCamelCase, ToPascalCase, ToShoutyKebabCase, ToShoutySnakeCase, ToSnakeCase,
};
use itertools::Itertools;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, format_ident, quote};
use syn::{
    Attribute, Data, DataEnum, DataStruct, DeriveInput, Field, Fields, GenericParam, Ident,
    parse_macro_input,
};

/// Derives the `JsonPointee` trait for JSON Pointer (RFC 6901) traversal.
///
/// See the [module documentation][crate] for detailed usage and examples.
#[proc_macro_derive(JsonPointee, attributes(ploidy))]
pub fn derive_pointee(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_for(&input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

fn derive_for(input: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let attrs: Vec<_> = input
        .attrs
        .iter()
        .map(ContainerAttr::parse_all)
        .flatten_ok()
        .try_collect()?;
    let container =
        ContainerInfo::new(name, &attrs).map_err(|err| syn::Error::new_spanned(input, err))?;

    // Hygienic parameter for the generated `resolve` method.
    let pointer = Ident::new("pointer", Span::mixed_site());

    let body = match &input.data {
        Data::Struct(data) => {
            if container.tag.is_some() {
                return Err(syn::Error::new_spanned(input, DeriveError::TagOnNonEnum));
            }
            derive_for_struct(&pointer, container, data)?
        }
        Data::Enum(data) => derive_for_enum(&pointer, container, data)?,
        Data::Union(_) => return Err(syn::Error::new_spanned(input, DeriveError::Union)),
    };

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let where_clause = {
        // Add or extend the `where` clause with `T: JsonPointee` bounds
        // for all generic type parameters.
        let bounds = input
            .generics
            .params
            .iter()
            .filter_map(|param| match param {
                GenericParam::Type(param) => {
                    let ident = &param.ident;
                    Some(quote! { #ident: ::ploidy_pointer::JsonPointee })
                }
                _ => None,
            })
            .collect_vec();
        if bounds.is_empty() {
            quote! { #where_clause }
        } else if let Some(where_clause) = where_clause {
            quote! { #where_clause #(#bounds),* }
        } else {
            quote! { where #(#bounds),* }
        }
    };

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::ploidy_pointer::JsonPointee for #name #ty_generics #where_clause {
            fn resolve(&self, #pointer: ::ploidy_pointer::JsonPointer<'_>)
                -> ::std::result::Result<&dyn ::ploidy_pointer::JsonPointee, ::ploidy_pointer::BadJsonPointer> {
                #body
            }
        }
    })
}

fn derive_for_struct(
    pointer: &Ident,
    container: ContainerInfo<'_>,
    data: &DataStruct,
) -> syn::Result<TokenStream> {
    let body = match &data.fields {
        Fields::Named(fields) => {
            let fields: Vec<_> = fields
                .named
                .iter()
                .map(|f| NamedFieldInfo::new(container, f))
                .try_collect()?;
            let bindings = fields.iter().map(|f| {
                let binding = f.binding;
                quote! { #binding }
            });
            let body = NamedPointeeBody::new(NamedPointeeTy::Struct(container), pointer, &fields);
            quote! {
                let Self { #(#bindings),* } = self;
                #body
            }
        }
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            // For newtype structs, resolve the pointer against the inner value.
            quote! {
                <_ as ::ploidy_pointer::JsonPointee>::resolve(&self.0, #pointer)
            }
        }
        Fields::Unnamed(fields) => {
            let fields: Vec<_> = fields
                .unnamed
                .iter()
                .enumerate()
                .map(|(index, f)| TupleFieldInfo::new(index, f))
                .try_collect()?;
            let bindings = fields.iter().map(|f| {
                let binding = &f.binding;
                quote! { #binding }
            });
            let body = TuplePointeeBody::new(TuplePointeeTy::Struct(container), pointer, &fields);
            quote! {
                let Self(#(#bindings),*) = self;
                #body
            }
        }
        Fields::Unit => {
            let body = UnitPointeeBody::new(UnitPointeeTy::Struct(container), pointer);
            quote!(#body)
        }
    };
    Ok(body)
}

fn derive_for_enum(
    pointer: &Ident,
    container: ContainerInfo<'_>,
    data: &DataEnum,
) -> syn::Result<TokenStream> {
    // Default to the externally tagged representation
    // if a tag isn't explicitly specified.
    let tag = container.tag.unwrap_or(VariantTag::External);

    let arms: Vec<_> = data
        .variants
        .iter()
        .map(|variant| {
            let name = &variant.ident;
            let attrs: Vec<_> = variant
                .attrs
                .iter()
                .map(VariantAttr::parse_all)
                .flatten_ok()
                .try_collect()?;
            let info = VariantInfo::new(container, name, &attrs);

            // For skipped variants, derive an implementation
            // that always returns an error.
            if info.is_skipped() {
                let ty = match &variant.fields {
                    Fields::Named(_) => VariantTy::Named(info, tag),
                    Fields::Unnamed(_) => VariantTy::Tuple(info, tag),
                    Fields::Unit => VariantTy::Unit(info, tag),
                };
                let body = SkippedVariantBody::new(ty, pointer);
                return syn::Result::Ok(quote!(#body));
            }

            let arm = match &variant.fields {
                Fields::Named(fields) => {
                    let fields: Vec<_> = fields
                        .named
                        .iter()
                        .map(|f| NamedFieldInfo::new(container, f))
                        .try_collect()?;
                    let bindings = fields.iter().map(|f| {
                        let binding = f.binding;
                        quote! { #binding }
                    });
                    let body = NamedPointeeBody::new(
                        NamedPointeeTy::Variant(info, tag),
                        pointer,
                        &fields,
                    );
                    quote! {
                        Self::#name { #(#bindings),* } => {
                            #body
                        }
                    }
                }
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    match tag {
                        VariantTag::Internal(tag_field) => {
                            // For internally tagged newtype variants, check the tag field
                            // before delegating to the inner value.
                            let key = Ident::new("key", Span::mixed_site());
                            let effective_name = info.effective_name();
                            quote! {
                                Self::#name(inner) => {
                                    let Some(#key) = #pointer.head() else {
                                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                                    };
                                    if #key.as_str() == #tag_field {
                                        return Ok(&#effective_name as &dyn ::ploidy_pointer::JsonPointee);
                                    }
                                    <_ as ::ploidy_pointer::JsonPointee>::resolve(inner, #pointer)
                                }
                            }
                        }
                        VariantTag::External => {
                            // For externally tagged newtype variants, the first segment
                            // must match the variant name; then the tail should resolve
                            // against the inner value.
                            let key = Ident::new("key", Span::mixed_site());
                            let effective_name = info.effective_name();
                            let pointee_ty = TuplePointeeTy::Variant(info, tag);
                            let key_err = if cfg!(feature = "did-you-mean") {
                                quote!(::ploidy_pointer::BadJsonPointerKey::with_ty(#key, #pointee_ty))
                            } else {
                                quote!(::ploidy_pointer::BadJsonPointerKey::new(#key))
                            };
                            quote! {
                                Self::#name(inner) => {
                                    let Some(#key) = #pointer.head() else {
                                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                                    };
                                    if #key.as_str() != #effective_name {
                                        return Err(#key_err)?;
                                    }
                                    <_ as ::ploidy_pointer::JsonPointee>::resolve(inner, #pointer.tail())
                                }
                            }
                        }
                        VariantTag::Adjacent { tag: tag_field, content: content_field } => {
                            // For adjacently tagged newtype variants, the first segment
                            // must match either the tag or content field.
                            let key = Ident::new("key", Span::mixed_site());
                            let effective_name = info.effective_name();
                            let pointee_ty = TuplePointeeTy::Variant(info, tag);
                            let key_err = if cfg!(feature = "did-you-mean") {
                                quote!(::ploidy_pointer::BadJsonPointerKey::with_suggestions(
                                    #key,
                                    #pointee_ty,
                                    [#tag_field, #content_field],
                                ))
                            } else {
                                quote!(::ploidy_pointer::BadJsonPointerKey::new(#key))
                            };
                            quote! {
                                Self::#name(inner) => {
                                    let Some(#key) = #pointer.head() else {
                                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                                    };
                                    match #key.as_str() {
                                        #tag_field => Ok(&#effective_name as &dyn ::ploidy_pointer::JsonPointee),
                                        #content_field => <_ as ::ploidy_pointer::JsonPointee>::resolve(inner, #pointer.tail()),
                                        _ => Err(#key_err)?,
                                    }
                                }
                            }
                        }
                        VariantTag::Untagged => {
                            // For untagged newtype variants, transparently resolve
                            // against the inner value.
                            quote! {
                                Self::#name(inner) => {
                                    <_ as ::ploidy_pointer::JsonPointee>::resolve(
                                        inner,
                                        #pointer,
                                    )
                                }
                            }
                        }
                    }
                }
                Fields::Unnamed(fields) => {
                    let fields: Vec<_> = fields
                        .unnamed
                        .iter()
                        .enumerate()
                        .map(|(index, f)| TupleFieldInfo::new(index, f))
                        .try_collect()?;
                    let bindings = fields.iter().map(|f| {
                        let binding = &f.binding;
                        quote! { #binding }
                    });
                    let body = TuplePointeeBody::new(
                        TuplePointeeTy::Variant(info, tag),
                        pointer,
                        &fields,
                    );
                    quote! {
                        Self::#name(#(#bindings),*) => {
                            #body
                        }
                    }
                }
                Fields::Unit => {
                    let body = UnitPointeeBody::new(
                        UnitPointeeTy::Variant(info, tag),
                        pointer,
                    );
                    quote! {
                        Self::#name => {
                            #body
                        }
                    }
                }
            };
            syn::Result::Ok(arm)
        })
        .try_collect()?;

    Ok(quote! {
        match self {
            #(#arms,)*
        }
    })
}

#[derive(Clone, Copy, Debug)]
struct ContainerInfo<'a> {
    name: &'a Ident,
    rename_all: Option<RenameAll>,
    tag: Option<VariantTag<'a>>,
}

impl<'a> ContainerInfo<'a> {
    fn new(name: &'a Ident, attrs: &'a [ContainerAttr]) -> Result<Self, DeriveError> {
        let rename_all = attrs.iter().find_map(|attr| match attr {
            &ContainerAttr::RenameAll(rename_all) => Some(rename_all),
            _ => None,
        });

        let tag = attrs
            .iter()
            .filter_map(|attr| match attr {
                ContainerAttr::Tag(t) => Some(t.as_str()),
                _ => None,
            })
            .at_most_one()
            .map_err(|_| DeriveError::ConflictingTagAttributes)?;
        let content = attrs
            .iter()
            .filter_map(|attr| match attr {
                ContainerAttr::Content(c) => Some(c.as_str()),
                _ => None,
            })
            .at_most_one()
            .map_err(|_| DeriveError::ConflictingTagAttributes)?;
        let untagged = attrs
            .iter()
            .filter(|attr| matches!(attr, ContainerAttr::Untagged))
            .at_most_one()
            .map_err(|_| DeriveError::ConflictingTagAttributes)?;
        let tag = match (tag, content, untagged) {
            // No explicit tag.
            (None, None, None) => None,
            // Internally tagged: only `tag`.
            (Some(tag), None, None) => Some(VariantTag::Internal(tag)),
            // Untagged: only `untagged`.
            (None, None, Some(_)) => Some(VariantTag::Untagged),
            (Some(tag), Some(content), None) if tag == content => {
                return Err(DeriveError::SameTagAndContent);
            }
            // Adjacently tagged: both `tag` and `content`.
            (Some(tag), Some(content), None) => Some(VariantTag::Adjacent { tag, content }),
            (None, Some(_), _) => return Err(DeriveError::ContentWithoutTag),
            _ => return Err(DeriveError::ConflictingTagAttributes),
        };

        Ok(Self {
            name,
            rename_all,
            tag,
        })
    }
}

#[derive(Debug)]
struct NamedFieldInfo<'a> {
    binding: &'a Ident,
    key: String,
    is_flattened: bool,
    is_skipped: bool,
}

impl<'a> NamedFieldInfo<'a> {
    fn new(container: ContainerInfo<'a>, f: &'a Field) -> syn::Result<Self> {
        let name = f.ident.as_ref().unwrap();
        let attrs: Vec<_> = f
            .attrs
            .iter()
            .map(FieldAttr::parse_all)
            .flatten_ok()
            .try_collect()?;

        let is_flattened = attrs.iter().any(|attr| matches!(attr, FieldAttr::Flatten));
        let is_skipped = attrs.iter().any(|attr| matches!(attr, FieldAttr::Skip));

        if is_flattened && is_skipped {
            return Err(syn::Error::new_spanned(f, DeriveError::FlattenWithSkip));
        }

        let key = attrs
            .iter()
            .find_map(|attr| match attr {
                FieldAttr::Rename(name) => Some(name.clone()),
                _ => None,
            })
            .or_else(|| {
                container
                    .rename_all
                    .map(|rename_all| rename_all.apply(&name.to_string()))
            })
            .unwrap_or_else(|| name.to_string());

        Ok(NamedFieldInfo {
            binding: name,
            key,
            is_flattened,
            is_skipped,
        })
    }
}

#[derive(Debug)]
struct TupleFieldInfo {
    index: usize,
    binding: Ident,
    is_skipped: bool,
}

impl TupleFieldInfo {
    fn new(index: usize, f: &Field) -> syn::Result<Self> {
        let attrs: Vec<_> = f
            .attrs
            .iter()
            .map(FieldAttr::parse_all)
            .flatten_ok()
            .try_collect()?;

        let _: () = attrs
            .iter()
            .map(|attr| match attr {
                FieldAttr::Flatten => {
                    Err(syn::Error::new_spanned(f, DeriveError::FlattenOnNonNamed))
                }
                FieldAttr::Rename(_) => {
                    Err(syn::Error::new_spanned(f, DeriveError::RenameOnNonNamed))
                }
                _ => Ok(()),
            })
            .try_collect()?;

        let is_skipped = attrs.iter().any(|attr| matches!(attr, FieldAttr::Skip));

        Ok(Self {
            index,
            binding: format_ident!("f{}", index, span = Span::mixed_site()),
            is_skipped,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct VariantInfo<'a> {
    container: ContainerInfo<'a>,
    name: &'a Ident,
    attrs: &'a [VariantAttr],
}

impl<'a> VariantInfo<'a> {
    fn new(container: ContainerInfo<'a>, name: &'a Ident, attrs: &'a [VariantAttr]) -> Self {
        Self {
            container,
            name,
            attrs,
        }
    }

    fn effective_name(&self) -> String {
        self.attrs
            .iter()
            .find_map(|attr| match attr {
                VariantAttr::Rename(name) => Some(name.clone()),
                _ => None,
            })
            .or_else(|| {
                self.container
                    .rename_all
                    .map(|rename_all| rename_all.apply(&self.name.to_string()))
            })
            .unwrap_or_else(|| self.name.to_string())
    }

    fn is_skipped(&self) -> bool {
        self.attrs
            .iter()
            .any(|attr| matches!(attr, VariantAttr::Skip))
    }
}

#[derive(Clone, Copy, Debug)]
struct NamedPointeeBody<'a> {
    ty: NamedPointeeTy<'a>,
    pointer: &'a Ident,
    fields: &'a [NamedFieldInfo<'a>],
}

impl<'a> NamedPointeeBody<'a> {
    fn new(ty: NamedPointeeTy<'a>, pointer: &'a Ident, fields: &'a [NamedFieldInfo]) -> Self {
        Self {
            ty,
            pointer,
            fields,
        }
    }
}

impl ToTokens for NamedPointeeBody<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let pointer = self.pointer;
        let key = Ident::new("key", Span::mixed_site());
        let pointee_ty = self.ty;

        // Build match arms for fields.
        let arms = self
            .fields
            .iter()
            .filter(|f| !f.is_flattened && !f.is_skipped)
            .map(|f| {
                let field_key = &f.key;
                let binding = f.binding;
                quote! {
                    #field_key => <_ as ::ploidy_pointer::JsonPointee>::resolve(
                        #binding,
                        #pointer.tail(),
                    )
                }
            });

        // Build field suggestions for error messages.
        let mut suggestions: Vec<_> = self
            .fields
            .iter()
            .filter(|f| !f.is_flattened && !f.is_skipped)
            .map(|f| {
                let key = &f.key;
                quote! { #key }
            })
            .collect();
        if let NamedPointeeTy::Variant(_, VariantTag::Internal(tag)) = self.ty {
            suggestions.push(quote! { #tag });
        }

        let wildcard = {
            // For flattened fields, we build an `.or_else()` chain bottom-up
            // using a right fold.
            let rest = if cfg!(feature = "did-you-mean") {
                quote!(Err(::ploidy_pointer::BadJsonPointerKey::with_suggestions(
                    #key,
                    #pointee_ty,
                    [#(#suggestions),*],
                ))?)
            } else {
                quote!(Err(::ploidy_pointer::BadJsonPointerKey::new(#key))?)
            };
            self.fields
                .iter()
                .filter(|f| f.is_flattened)
                .rfold(rest, |rest, f| {
                    let binding = f.binding;
                    quote! {
                        <_ as ::ploidy_pointer::JsonPointee>
                            ::resolve(
                                #binding,
                                #pointer.clone()
                            )
                            .or_else(|_| #rest)
                    }
                })
        };

        let body = match self.ty {
            NamedPointeeTy::Variant(info, VariantTag::Internal(tag_field)) => {
                // For internally tagged struct-like variants, check the tag field
                // before resolving against the named fields.
                let variant_name = info.effective_name();
                quote! {
                    let Some(#key) = #pointer.head() else {
                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                    };
                    if #key.as_str() == #tag_field {
                        return Ok(&#variant_name as &dyn ::ploidy_pointer::JsonPointee);
                    }
                    match #key.as_str() {
                        #(#arms,)*
                        _ => #wildcard,
                    }
                }
            }
            NamedPointeeTy::Variant(info, VariantTag::External) => {
                // For externally tagged struct-like variants, the first segment
                // must match the variant name; then the tail should resolve
                // against the named fields.
                let variant_name = info.effective_name();
                let ty_err = if cfg!(feature = "did-you-mean") {
                    quote!(::ploidy_pointer::BadJsonPointerTy::with_ty(&#pointer, #pointee_ty))
                } else {
                    quote!(::ploidy_pointer::BadJsonPointerTy::new(&#pointer))
                };
                quote! {
                    let Some(#key) = #pointer.head() else {
                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                    };
                    if #key.as_str() != #variant_name {
                        return Err(#ty_err)?;
                    }
                    let #pointer = #pointer.tail();
                    let Some(#key) = #pointer.head() else {
                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                    };
                    match #key.as_str() {
                        #(#arms,)*
                        _ => #wildcard,
                    }
                }
            }
            NamedPointeeTy::Variant(
                info,
                VariantTag::Adjacent {
                    tag: tag_field,
                    content: content_field,
                },
            ) => {
                // For adjacently tagged struct-like variants, the first segment
                // must match either the tag or content field.
                let variant_name = info.effective_name();
                let key_err = if cfg!(feature = "did-you-mean") {
                    quote!(::ploidy_pointer::BadJsonPointerKey::with_suggestions(
                        #key,
                        #pointee_ty,
                        [#tag_field, #content_field],
                    ))
                } else {
                    quote!(::ploidy_pointer::BadJsonPointerKey::new(#key))
                };
                quote! {
                    let Some(#key) = #pointer.head() else {
                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                    };
                    match #key.as_str() {
                        #tag_field => {
                            return Ok(&#variant_name as &dyn ::ploidy_pointer::JsonPointee);
                        }
                        #content_field => {
                            let #pointer = #pointer.tail();
                            let Some(#key) = #pointer.head() else {
                                return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                            };
                            match #key.as_str() {
                                #(#arms,)*
                                _ => #wildcard,
                            }
                        }
                        _ => {
                            return Err(#key_err)?;
                        }
                    }
                }
            }
            NamedPointeeTy::Struct(_) | NamedPointeeTy::Variant(_, VariantTag::Untagged) => {
                // For structs and untagged struct-like variants,
                // access the fields directly.
                quote! {
                    let Some(#key) = #pointer.head() else {
                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                    };
                    match #key.as_str() {
                        #(#arms,)*
                        _ => #wildcard,
                    }
                }
            }
        };

        tokens.append_all(body);
    }
}

#[derive(Clone, Copy, Debug)]
struct TuplePointeeBody<'a> {
    ty: TuplePointeeTy<'a>,
    pointer: &'a Ident,
    fields: &'a [TupleFieldInfo],
}

impl<'a> TuplePointeeBody<'a> {
    fn new(ty: TuplePointeeTy<'a>, pointer: &'a Ident, fields: &'a [TupleFieldInfo]) -> Self {
        Self {
            ty,
            pointer,
            fields,
        }
    }
}

impl ToTokens for TuplePointeeBody<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let pointer = self.pointer;
        let idx = Ident::new("idx", Span::mixed_site());
        let key = Ident::new("key", Span::mixed_site());

        // Build match arms for tuple indices.
        let arms = self.fields.iter().filter(|f| !f.is_skipped).map(|f| {
            let index = f.index;
            let binding = &f.binding;
            quote! {
                #index => <_ as ::ploidy_pointer::JsonPointee>::resolve(
                    #binding,
                    #pointer.tail(),
                )
            }
        });

        // Build common tail.
        let ty = self.ty;
        let len = self.fields.len();
        let ty_err = if cfg!(feature = "did-you-mean") {
            quote!(::ploidy_pointer::BadJsonPointerTy::with_ty(&#pointer, #ty))
        } else {
            quote!(::ploidy_pointer::BadJsonPointerTy::new(&#pointer))
        };
        let tail = quote! {
            let Some(#idx) = #key.to_index() else {
                return Err(#ty_err)?;
            };
            match #idx {
                #(#arms,)*
                _ => Err(::ploidy_pointer::BadJsonPointer::Index(#idx, 0..#len))
            }
        };

        let body = match self.ty {
            TuplePointeeTy::Variant(info, VariantTag::Internal(tag_field)) => {
                // For internally tagged tuple variants, check the tag field
                // before resolving against the tuple indices.
                let variant_name = info.effective_name();
                quote! {
                    let Some(#key) = #pointer.head() else {
                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                    };
                    if #key.as_str() == #tag_field {
                        return Ok(&#variant_name as &dyn ::ploidy_pointer::JsonPointee);
                    }
                    #tail
                }
            }
            TuplePointeeTy::Variant(info, VariantTag::External) => {
                // For externally tagged tuple variants, the first segment
                // must match the variant name; then the tail should resolve
                // against the tuple indices.
                let variant_name = info.effective_name();
                let ty_err = if cfg!(feature = "did-you-mean") {
                    quote!(::ploidy_pointer::BadJsonPointerTy::with_ty(&#pointer, #ty))
                } else {
                    quote!(::ploidy_pointer::BadJsonPointerTy::new(&#pointer))
                };
                quote! {
                    let Some(#key) = #pointer.head() else {
                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                    };
                    if #key.as_str() != #variant_name {
                        return Err(#ty_err)?;
                    }
                    let #pointer = #pointer.tail();
                    let Some(#key) = #pointer.head() else {
                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                    };
                    #tail
                }
            }
            TuplePointeeTy::Variant(
                info,
                VariantTag::Adjacent {
                    tag: tag_field,
                    content: content_field,
                },
            ) => {
                // For adjacently tagged tuple variants, the first segment
                // must match either the tag or content field.
                let variant_name = info.effective_name();
                let key_err = if cfg!(feature = "did-you-mean") {
                    quote!(::ploidy_pointer::BadJsonPointerKey::with_suggestions(
                        #key,
                        #ty,
                        [#tag_field, #content_field],
                    ))
                } else {
                    quote!(::ploidy_pointer::BadJsonPointerKey::new(#key))
                };
                quote! {
                    let Some(#key) = #pointer.head() else {
                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                    };
                    match #key.as_str() {
                        #tag_field => {
                            return Ok(&#variant_name as &dyn ::ploidy_pointer::JsonPointee);
                        }
                        #content_field => {
                            let #pointer = #pointer.tail();
                            let Some(#key) = #pointer.head() else {
                                return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                            };
                            #tail
                        }
                        _ => {
                            return Err(#key_err)?;
                        }
                    }
                }
            }
            TuplePointeeTy::Struct(_) | TuplePointeeTy::Variant(_, VariantTag::Untagged) => {
                // For structs and untagged tuple variants,
                // access the tuple indices directly.
                quote! {
                    let Some(#key) = #pointer.head() else {
                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                    };
                    #tail
                }
            }
        };

        tokens.append_all(body);
    }
}

#[derive(Clone, Copy, Debug)]
struct UnitPointeeBody<'a> {
    ty: UnitPointeeTy<'a>,
    pointer: &'a Ident,
}

impl<'a> UnitPointeeBody<'a> {
    fn new(ty: UnitPointeeTy<'a>, pointer: &'a Ident) -> Self {
        Self { ty, pointer }
    }
}

impl ToTokens for UnitPointeeBody<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let pointer = self.pointer;
        let body = match self.ty {
            ty @ UnitPointeeTy::Variant(info, VariantTag::Internal(tag_field)) => {
                // For internally tagged unit variants, only the tag field is accessible.
                let key = Ident::new("key", Span::mixed_site());
                let variant_name = info.effective_name();
                let key_err = if cfg!(feature = "did-you-mean") {
                    quote!(::ploidy_pointer::BadJsonPointerKey::with_suggestions(
                        #key,
                        #ty,
                        [#tag_field],
                    ))
                } else {
                    quote!(::ploidy_pointer::BadJsonPointerKey::new(#key))
                };
                quote! {
                    let Some(#key) = #pointer.head() else {
                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                    };
                    if #key.as_str() == #tag_field {
                        return Ok(&#variant_name as &dyn ::ploidy_pointer::JsonPointee);
                    }
                    Err(#key_err)?
                }
            }
            ty @ UnitPointeeTy::Variant(info, VariantTag::External) => {
                // For externally tagged unit variants, allow just the tag field.
                let key = Ident::new("key", Span::mixed_site());
                let variant_name = info.effective_name();
                let key_err = if cfg!(feature = "did-you-mean") {
                    quote!(::ploidy_pointer::BadJsonPointerKey::with_ty(#key, #ty))
                } else {
                    quote!(::ploidy_pointer::BadJsonPointerKey::new(#key))
                };
                let ty_err = if cfg!(feature = "did-you-mean") {
                    quote!(::ploidy_pointer::BadJsonPointerTy::with_ty(&#pointer.tail(), #ty))
                } else {
                    quote!(::ploidy_pointer::BadJsonPointerTy::new(&#pointer.tail()))
                };
                quote! {
                    let Some(#key) = #pointer.head() else {
                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                    };
                    if #key.as_str() != #variant_name {
                        return Err(#key_err)?;
                    }
                    if !#pointer.tail().is_empty() {
                        return Err(#ty_err)?;
                    }
                    Ok(self as &dyn ::ploidy_pointer::JsonPointee)
                }
            }
            ty @ UnitPointeeTy::Variant(info, VariantTag::Adjacent { tag: tag_field, .. }) => {
                // For adjacently tagged unit variants, allow just the tag field.
                let key = Ident::new("key", Span::mixed_site());
                let variant_name = info.effective_name();
                let key_err = if cfg!(feature = "did-you-mean") {
                    quote!(::ploidy_pointer::BadJsonPointerKey::with_suggestions(
                        #key,
                        #ty,
                        [#tag_field],
                    ))
                } else {
                    quote!(::ploidy_pointer::BadJsonPointerKey::new(#key))
                };
                quote! {
                    let Some(#key) = #pointer.head() else {
                        return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                    };
                    match #key.as_str() {
                        #tag_field => {
                            return Ok(&#variant_name as &dyn ::ploidy_pointer::JsonPointee);
                        }
                        _ => {
                            return Err(#key_err)?;
                        }
                    }
                }
            }
            ty @ (UnitPointeeTy::Struct(_) | UnitPointeeTy::Variant(_, VariantTag::Untagged)) => {
                // For unit structs and untagged unit variants, deny all fields.
                let ty_err = if cfg!(feature = "did-you-mean") {
                    quote!(::ploidy_pointer::BadJsonPointerTy::with_ty(&#pointer, #ty))
                } else {
                    quote!(::ploidy_pointer::BadJsonPointerTy::new(&#pointer))
                };
                quote! {
                    if #pointer.is_empty() {
                        Ok(self as &dyn ::ploidy_pointer::JsonPointee)
                    } else {
                        Err(#ty_err)?
                    }
                }
            }
        };
        tokens.append_all(body);
    }
}

#[derive(Clone, Copy, Debug)]
enum NamedPointeeTy<'a> {
    Struct(ContainerInfo<'a>),
    Variant(VariantInfo<'a>, VariantTag<'a>),
}

impl ToTokens for NamedPointeeTy<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(match self {
            Self::Struct(info) => {
                let ty = info.name;
                quote! {
                    ::ploidy_pointer::JsonPointeeTy::struct_named(
                        stringify!(#ty)
                    )
                }
            }
            Self::Variant(info, ..) => {
                let ty = info.container.name;
                let variant = info.name;
                quote! {
                    ::ploidy_pointer::JsonPointeeTy::struct_variant_named(
                        stringify!(#ty),
                        stringify!(#variant),
                    )
                }
            }
        });
    }
}

#[derive(Clone, Copy, Debug)]
enum TuplePointeeTy<'a> {
    Struct(ContainerInfo<'a>),
    Variant(VariantInfo<'a>, VariantTag<'a>),
}

impl ToTokens for TuplePointeeTy<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(match self {
            Self::Struct(info) => {
                let ty = info.name;
                quote! {
                    ::ploidy_pointer::JsonPointeeTy::tuple_struct_named(
                        stringify!(#ty)
                    )
                }
            }
            Self::Variant(info, ..) => {
                let ty = info.container.name;
                let variant = info.name;
                quote! {
                    ::ploidy_pointer::JsonPointeeTy::tuple_variant_named(
                        stringify!(#ty),
                        stringify!(#variant),
                    )
                }
            }
        });
    }
}

#[derive(Clone, Copy, Debug)]
enum UnitPointeeTy<'a> {
    Struct(ContainerInfo<'a>),
    Variant(VariantInfo<'a>, VariantTag<'a>),
}

impl ToTokens for UnitPointeeTy<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(match self {
            Self::Struct(info) => {
                let ty = info.name;
                quote! {
                    ::ploidy_pointer::JsonPointeeTy::unit_struct_named(
                        stringify!(#ty)
                    )
                }
            }
            Self::Variant(info, ..) => {
                let ty = info.container.name;
                let variant = info.name;
                quote! {
                    ::ploidy_pointer::JsonPointeeTy::unit_variant_named(
                        stringify!(#ty),
                        stringify!(#variant),
                    )
                }
            }
        });
    }
}

#[derive(Clone, Copy, Debug)]
enum VariantTy<'a> {
    Named(VariantInfo<'a>, VariantTag<'a>),
    Tuple(VariantInfo<'a>, VariantTag<'a>),
    Unit(VariantInfo<'a>, VariantTag<'a>),
}

impl<'a> VariantTy<'a> {
    fn info(self) -> VariantInfo<'a> {
        let (Self::Named(info, _) | Self::Tuple(info, _) | Self::Unit(info, _)) = self;
        info
    }

    fn tag(self) -> VariantTag<'a> {
        let (Self::Named(_, tag) | Self::Tuple(_, tag) | Self::Unit(_, tag)) = self;
        tag
    }
}

impl ToTokens for VariantTy<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(match self {
            Self::Named(info, _) => {
                let ty = info.container.name;
                let variant = info.name;
                quote! {
                    ::ploidy_pointer::JsonPointeeTy::struct_variant_named(
                        stringify!(#ty),
                        stringify!(#variant),
                    )
                }
            }
            Self::Tuple(info, _) => {
                let ty = info.container.name;
                let variant = info.name;
                quote! {
                    ::ploidy_pointer::JsonPointeeTy::tuple_variant_named(
                        stringify!(#ty),
                        stringify!(#variant),
                    )
                }
            }
            Self::Unit(info, _) => {
                let ty = info.container.name;
                let variant = info.name;
                quote! {
                    ::ploidy_pointer::JsonPointeeTy::unit_variant_named(
                        stringify!(#ty),
                        stringify!(#variant),
                    )
                }
            }
        });
    }
}

#[derive(Clone, Copy, Debug)]
struct SkippedVariantBody<'a> {
    ty: VariantTy<'a>,
    pointer: &'a Ident,
}

impl<'a> SkippedVariantBody<'a> {
    fn new(ty: VariantTy<'a>, pointer: &'a Ident) -> Self {
        Self { ty, pointer }
    }
}

impl ToTokens for SkippedVariantBody<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let pointer = self.pointer;
        let ty = self.ty;

        let pattern = match ty {
            VariantTy::Named(info, _) => {
                let variant_name = info.name;
                quote!(Self::#variant_name { .. })
            }
            VariantTy::Tuple(info, _) => {
                let variant_name = info.name;
                quote!(Self::#variant_name(..))
            }
            VariantTy::Unit(info, _) => {
                let variant_name = info.name;
                quote!(Self::#variant_name)
            }
        };

        match ty.tag() {
            VariantTag::Internal(tag_field) => {
                // Internally tagged skipped variants allow access to the tag field only.
                let key = Ident::new("key", Span::mixed_site());
                let effective_name = ty.info().effective_name();
                let ty_err = if cfg!(feature = "did-you-mean") {
                    quote!(::ploidy_pointer::BadJsonPointerTy::with_ty(&#pointer, #ty))
                } else {
                    quote!(::ploidy_pointer::BadJsonPointerTy::new(&#pointer))
                };
                tokens.append_all(quote! {
                    #pattern => {
                        let Some(#key) = #pointer.head() else {
                            return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                        };
                        if #key.as_str() == #tag_field {
                            return Ok(&#effective_name as &dyn ::ploidy_pointer::JsonPointee);
                        }
                        Err(#ty_err)?
                    }
                });
            }
            VariantTag::External => {
                // Externally tagged skipped variants are completely inaccessible.
                let ty_err = if cfg!(feature = "did-you-mean") {
                    quote!(::ploidy_pointer::BadJsonPointerTy::with_ty(&#pointer, #ty))
                } else {
                    quote!(::ploidy_pointer::BadJsonPointerTy::new(&#pointer))
                };
                tokens.append_all(quote! {
                    #pattern => Err(#ty_err)?
                });
            }
            VariantTag::Adjacent { tag: tag_field, .. } => {
                // Adjacently tagged skipped variants allow tag field access,
                // but content field access errors.
                let key = Ident::new("key", Span::mixed_site());
                let effective_name = ty.info().effective_name();
                let key_err = if cfg!(feature = "did-you-mean") {
                    quote!(::ploidy_pointer::BadJsonPointerKey::with_suggestions(
                        #key,
                        #ty,
                        [#tag_field],
                    ))
                } else {
                    quote!(::ploidy_pointer::BadJsonPointerKey::new(#key))
                };
                tokens.append_all(quote! {
                    #pattern => {
                        let Some(#key) = #pointer.head() else {
                            return Ok(self as &dyn ::ploidy_pointer::JsonPointee);
                        };
                        match #key.as_str() {
                            #tag_field => {
                                return Ok(&#effective_name as &dyn ::ploidy_pointer::JsonPointee);
                            }
                            _ => {
                                return Err(#key_err)?;
                            }
                        }
                    }
                });
            }
            VariantTag::Untagged => {
                // Untagged skipped variants are completely inaccessible.
                let ty_err = if cfg!(feature = "did-you-mean") {
                    quote!(::ploidy_pointer::BadJsonPointerTy::with_ty(&#pointer, #ty))
                } else {
                    quote!(::ploidy_pointer::BadJsonPointerTy::new(&#pointer))
                };
                tokens.append_all(quote! {
                    #pattern => Err(#ty_err)?
                });
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum VariantTag<'a> {
    Internal(&'a str),
    External,
    Adjacent { tag: &'a str, content: &'a str },
    Untagged,
}

#[derive(Clone, Debug)]
enum ContainerAttr {
    RenameAll(RenameAll),
    Tag(String),
    Content(String),
    Untagged,
}

impl ContainerAttr {
    fn parse_all(attr: &Attribute) -> syn::Result<Vec<Self>> {
        if !attr.path().is_ident("ploidy") {
            return Ok(vec![]);
        }
        let mut attrs = vec![];
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename_all") {
                let value = meta.value()?;
                let s: syn::LitStr = value.parse()?;
                let Some(rename) = RenameAll::from_str(&s.value()) else {
                    return Err(meta.error(DeriveError::BadRenameAll));
                };
                attrs.push(Self::RenameAll(rename));
            } else if meta.path.is_ident("tag") {
                let value = meta.value()?;
                let s: syn::LitStr = value.parse()?;
                attrs.push(Self::Tag(s.value()));
            } else if meta.path.is_ident("content") {
                let value = meta.value()?;
                let s: syn::LitStr = value.parse()?;
                attrs.push(Self::Content(s.value()));
            } else if meta.path.is_ident("untagged") {
                attrs.push(Self::Untagged);
            }
            Ok(())
        })?;
        Ok(attrs)
    }
}

#[derive(Clone, Debug)]
enum FieldAttr {
    Rename(String),
    Flatten,
    Skip,
}

impl FieldAttr {
    fn parse_all(attr: &Attribute) -> syn::Result<Vec<Self>> {
        if !attr.path().is_ident("ploidy") {
            return Ok(vec![]);
        }
        let mut attrs = vec![];
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                let value = meta.value()?;
                let s: syn::LitStr = value.parse()?;
                attrs.push(Self::Rename(s.value()));
            } else if meta.path.is_ident("flatten") {
                attrs.push(Self::Flatten);
            } else if meta.path.is_ident("skip") {
                attrs.push(Self::Skip);
            }
            Ok(())
        })?;
        Ok(attrs)
    }
}

#[derive(Clone, Debug)]
enum VariantAttr {
    Skip,
    Rename(String),
}

impl VariantAttr {
    fn parse_all(attr: &Attribute) -> syn::Result<Vec<Self>> {
        if !attr.path().is_ident("ploidy") {
            return Ok(vec![]);
        }
        let mut attrs = vec![];
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                attrs.push(Self::Skip);
            } else if meta.path.is_ident("rename") {
                let value = meta.value()?;
                let s: syn::LitStr = value.parse()?;
                attrs.push(Self::Rename(s.value()));
            }
            Ok(())
        })?;
        Ok(attrs)
    }
}

/// Supported `rename_all` transforms, matching Serde.
#[derive(Clone, Copy, Debug)]
enum RenameAll {
    Lowercase,
    Uppercase,
    PascalCase,
    CamelCase,
    SnakeCase,
    ScreamingSnakeCase,
    KebabCase,
    ScreamingKebabCase,
}

impl RenameAll {
    const fn all() -> &'static [Self] {
        &[
            Self::Lowercase,
            Self::Uppercase,
            Self::PascalCase,
            Self::CamelCase,
            Self::SnakeCase,
            Self::ScreamingSnakeCase,
            Self::KebabCase,
            Self::ScreamingKebabCase,
        ]
    }

    fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "lowercase" => RenameAll::Lowercase,
            "UPPERCASE" => RenameAll::Uppercase,
            "PascalCase" => RenameAll::PascalCase,
            "camelCase" => RenameAll::CamelCase,
            "snake_case" => RenameAll::SnakeCase,
            "SCREAMING_SNAKE_CASE" => RenameAll::ScreamingSnakeCase,
            "kebab-case" => RenameAll::KebabCase,
            "SCREAMING-KEBAB-CASE" => RenameAll::ScreamingKebabCase,
            _ => return None,
        })
    }

    fn apply(&self, s: &str) -> String {
        match self {
            RenameAll::Lowercase => s.to_lowercase(),
            RenameAll::Uppercase => s.to_uppercase(),
            RenameAll::PascalCase => s.to_pascal_case(),
            RenameAll::CamelCase => s.to_lower_camel_case(),
            RenameAll::SnakeCase => s.to_snake_case(),
            RenameAll::ScreamingSnakeCase => s.to_shouty_snake_case(),
            RenameAll::KebabCase => s.to_kebab_case(),
            RenameAll::ScreamingKebabCase => s.to_shouty_kebab_case(),
        }
    }
}

impl Display for RenameAll {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Lowercase => "lowercase",
            Self::Uppercase => "UPPERCASE",
            Self::PascalCase => "PascalCase",
            Self::CamelCase => "camelCase",
            Self::SnakeCase => "snake_case",
            Self::ScreamingSnakeCase => "SCREAMING_SNAKE_CASE",
            Self::KebabCase => "kebab-case",
            Self::ScreamingKebabCase => "SCREAMING-KEBAB-CASE",
        })
    }
}

#[derive(Debug, thiserror::Error)]
enum DeriveError {
    #[error("`JsonPointee` can't be derived for unions")]
    Union,
    #[error("`rename` is only supported on struct and struct-like enum variant fields")]
    RenameOnNonNamed,
    #[error("`flatten` is only supported on struct and struct-like enum variant fields")]
    FlattenOnNonNamed,
    #[error("`flatten` and `skip` are mutually exclusive")]
    FlattenWithSkip,
    #[error("`tag` is only supported on enums")]
    TagOnNonEnum,
    #[error("`content` requires `tag`")]
    ContentWithoutTag,
    #[error("`tag` and `content` must have different field names")]
    SameTagAndContent,
    #[error("only one of: `tag`, `tag` and `content`, `untagged` allowed")]
    ConflictingTagAttributes,
    #[error("`rename_all` must be one of: {}", RenameAll::all().iter().join(","))]
    BadRenameAll,
}
