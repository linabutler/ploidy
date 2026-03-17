use ploidy_core::ir::{EnumVariant, EnumView};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, format_ident, quote};
use syn::{Ident, parse_quote};

use super::{
    doc_attrs,
    ext::EnumViewExt,
    naming::{CodegenIdent, CodegenIdentUsage, CodegenTypeName},
};

#[derive(Clone, Debug)]
pub struct CodegenEnum<'a> {
    name: CodegenTypeName<'a>,
    ty: &'a EnumView<'a>,
}

impl<'a> CodegenEnum<'a> {
    pub fn new(name: CodegenTypeName<'a>, ty: &'a EnumView<'a>) -> Self {
        Self { name, ty }
    }
}

impl ToTokens for CodegenEnum<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        if !self.ty.representable() {
            // If any variant can't be represented as a Rust enum variant,
            // emit a type alias for the enum instead.
            let type_name: Ident = {
                let name = &self.name;
                parse_quote!(#name)
            };
            let doc_attrs = self.ty.description().map(doc_attrs);
            tokens.append_all(quote! {
                #doc_attrs
                pub type #type_name = ::std::string::String;
            });
        } else {
            // Otherwise, emit a Rust enum.
            let mut variants = vec![];
            let mut display_arms = vec![];
            let mut from_str_arms = vec![];

            for variant in self.ty.variants() {
                match variant {
                    EnumVariant::String(name) => {
                        let name_ident = CodegenIdent::new(name);
                        let variant_name = CodegenIdentUsage::Variant(&name_ident);
                        variants.push(quote! { #variant_name });
                        display_arms.push(quote! { Self::#variant_name => #name });
                        from_str_arms.push(quote! { #name => Self::#variant_name });
                    }
                    _ => continue,
                }
            }

            // The catch-all `Other` variant comes last.
            let type_name: Ident = {
                let name = &self.name;
                parse_quote!(#name)
            };
            let other_name = format_ident!("Other{}", type_name);
            variants.push(quote! { #other_name(String) });
            display_arms.push(quote! { Self::#other_name(s) => s.as_str() });
            from_str_arms.push(quote! { _ => Self::#other_name(s.to_owned()) });

            let expecting = format!("a variant of `{type_name}`");

            let doc_attrs = self.ty.description().map(doc_attrs);

            tokens.append_all(quote! {
                #doc_attrs
                #[derive(Clone, Debug, Eq, Hash, PartialEq)]
                pub enum #type_name {
                    #(#variants),*
                }

                impl ::std::default::Default for #type_name {
                    fn default() -> Self {
                        Self::#other_name(::std::string::String::default())
                    }
                }

                impl ::std::fmt::Display for #type_name {
                    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        f.write_str(match self {
                            #(#display_arms),*
                        })
                    }
                }

                impl ::std::str::FromStr for #type_name {
                    type Err = ::std::convert::Infallible;

                    fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                        ::std::result::Result::Ok(match s {
                            #(#from_str_arms),*
                        })
                    }
                }

                impl<'de> ::ploidy_util::serde::Deserialize<'de> for #type_name {
                    fn deserialize<D: ::ploidy_util::serde::Deserializer<'de>>(
                        deserializer: D,
                    ) -> ::std::result::Result<Self, D::Error> {
                        struct Visitor;
                        impl<'de> ::ploidy_util::serde::de::Visitor<'de> for Visitor {
                            type Value = #type_name;

                            fn expecting(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                                f.write_str(#expecting)
                            }

                            fn visit_str<E: ::ploidy_util::serde::de::Error>(
                                self,
                                s: &str,
                            ) -> ::std::result::Result<Self::Value, E> {
                                let ::std::result::Result::Ok(v) = ::std::str::FromStr::from_str(s);
                                Ok(v)
                            }
                        }
                        ::ploidy_util::serde::Deserializer::deserialize_str(deserializer, Visitor)
                    }
                }

                impl ::ploidy_util::serde::Serialize for #type_name {
                    fn serialize<S: ::ploidy_util::serde::Serializer>(
                        &self,
                        serializer: S,
                    ) -> ::std::result::Result<S::Ok, S::Error> {
                        serializer.collect_str(self)
                    }
                }
            });
        }
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

    // MARK: String variants

    #[test]
    fn test_enum_string_variants() {
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
                    - pending
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schemas().find(|s| s.name() == "Status");
        let Some(schema @ SchemaTypeView::Enum(_, enum_view)) = &schema else {
            panic!("expected enum `Status`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenEnum::new(name, enum_view);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Clone, Debug, Eq, Hash, PartialEq)]
            pub enum Status {
                Active,
                Inactive,
                Pending,
                OtherStatus(String)
            }
            impl ::std::default::Default for Status {
                fn default() -> Self {
                    Self::OtherStatus(::std::string::String::default())
                }
            }
            impl ::std::fmt::Display for Status {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    f.write_str(
                        match self {
                            Self::Active => "active",
                            Self::Inactive => "inactive",
                            Self::Pending => "pending",
                            Self::OtherStatus(s) => s.as_str()
                        }
                    )
                }
            }
            impl ::std::str::FromStr for Status {
                type Err = ::std::convert::Infallible;
                fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                    ::std::result::Result::Ok(
                        match s {
                            "active" => Self::Active,
                            "inactive" => Self::Inactive,
                            "pending" => Self::Pending,
                            _ => Self::OtherStatus(s.to_owned())
                        }
                    )
                }
            }
            impl<'de> ::ploidy_util::serde::Deserialize<'de> for Status {
                fn deserialize<D: ::ploidy_util::serde::Deserializer<'de>>(
                    deserializer: D,
                ) -> ::std::result::Result<Self, D::Error> {
                    struct Visitor;
                    impl<'de> ::ploidy_util::serde::de::Visitor<'de> for Visitor {
                        type Value = Status;
                        fn expecting(
                            &self,
                            f: &mut ::std::fmt::Formatter<'_>
                        ) -> ::std::fmt::Result {
                            f.write_str("a variant of `Status`")
                        }
                        fn visit_str<E: ::ploidy_util::serde::de::Error>(
                            self,
                            s: &str,
                        ) -> ::std::result::Result<Self::Value, E> {
                            let ::std::result::Result::Ok(v) = ::std::str::FromStr::from_str(s);
                            Ok(v)
                        }
                    }
                    ::ploidy_util::serde::Deserializer::deserialize_str(deserializer, Visitor)
                }
            }
            impl ::ploidy_util::serde::Serialize for Status {
                fn serialize<S: ::ploidy_util::serde::Serializer>(
                    &self,
                    serializer: S,
                ) -> ::std::result::Result<S::Ok, S::Error> {
                    serializer.collect_str(self)
                }
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: Unrepresentable variants

    #[test]
    fn test_enum_unrepresentable_becomes_type_alias() {
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
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schemas().find(|s| s.name() == "Priority");
        let Some(schema @ SchemaTypeView::Enum(_, view)) = &schema else {
            panic!("expected enum `Priority`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenEnum::new(name, view);

        let actual: syn::Item = parse_quote!(#codegen);
        let expected: syn::Item = parse_quote! {
            pub type Priority = ::std::string::String;
        };
        assert_eq!(actual, expected);
    }
}
