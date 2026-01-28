use ploidy_core::ir::{ExtendableView, IrPrimitiveView, PrimitiveIrType};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::config::DateTimeFormat;

#[derive(Clone, Copy, Debug)]
pub struct CodegenPrimitive<'a> {
    ty: &'a IrPrimitiveView<'a>,
}

impl<'a> CodegenPrimitive<'a> {
    pub fn new(ty: &'a IrPrimitiveView<'a>) -> Self {
        Self { ty }
    }
}

impl<'a> ToTokens for CodegenPrimitive<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(match self.ty.ty() {
            PrimitiveIrType::String => quote! { ::std::string::String },
            PrimitiveIrType::I8 => quote! { i8 },
            PrimitiveIrType::U8 => quote! { u8 },
            PrimitiveIrType::I16 => quote! { i16 },
            PrimitiveIrType::U16 => quote! { u16 },
            PrimitiveIrType::I32 => quote! { i32 },
            PrimitiveIrType::U32 => quote! { u32 },
            PrimitiveIrType::I64 => quote! { i64 },
            PrimitiveIrType::U64 => quote! { u64 },
            PrimitiveIrType::F32 => quote! { f32 },
            PrimitiveIrType::F64 => quote! { f64 },
            PrimitiveIrType::Bool => quote! { bool },
            PrimitiveIrType::DateTime => {
                let format = self
                    .ty
                    .extensions()
                    .get::<DateTimeFormat>()
                    .as_deref()
                    .copied()
                    .unwrap_or_default();
                match format {
                    DateTimeFormat::Rfc3339 => {
                        quote! { ::chrono::DateTime<::chrono::Utc> }
                    }
                    DateTimeFormat::UnixSeconds => {
                        quote! { ::ploidy_util::date_time::UnixSeconds }
                    }
                    DateTimeFormat::UnixMilliseconds => {
                        quote! { ::ploidy_util::date_time::UnixMilliseconds }
                    }
                    DateTimeFormat::UnixMicroseconds => {
                        quote! { ::ploidy_util::date_time::UnixMicroseconds }
                    }
                    DateTimeFormat::UnixNanoseconds => {
                        quote! { ::ploidy_util::date_time::UnixNanoseconds }
                    }
                }
            }
            PrimitiveIrType::UnixTime => quote! { ::ploidy_util::date_time::UnixSeconds },
            PrimitiveIrType::Date => quote! { ::chrono::NaiveDate },
            PrimitiveIrType::Url => quote! { ::url::Url },
            PrimitiveIrType::Uuid => quote! { ::uuid::Uuid },
            PrimitiveIrType::Bytes => quote! { ::ploidy_util::binary::Base64 },
            PrimitiveIrType::Binary => quote! { ::serde_bytes::ByteBuf },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use itertools::Itertools;
    use ploidy_core::{
        ir::{IrGraph, IrSpec},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    use crate::{CodegenConfig, CodegenGraph, DateTimeFormat};

    #[test]
    fn test_codegen_primitive_string() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: string
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected string; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(::std::string::String);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_i8() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: integer
                      format: int8
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected i8; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(i8);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_u8() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: integer
                      format: uint8
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected u8; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(u8);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_i16() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: integer
                      format: int16
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected i16; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(i16);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_u16() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: integer
                      format: uint16
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected u16; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(u16);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_i32() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: integer
                      format: int32
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected string; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(i32);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_u32() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: integer
                      format: uint32
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected u32; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(u32);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_i64() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: integer
                      format: int64
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected i64; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(i64);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_u64() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: integer
                      format: uint64
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected u64; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(u64);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_f32() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: number
                      format: float
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected f32; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(f32);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_f64() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: number
                      format: double
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected f64; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(f64);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_bool() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: boolean
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected bool; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(bool);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_datetime_default_rfc3339() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: string
                      format: date-time
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        // Default config uses RFC 3339.
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected datetime; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(::chrono::DateTime<::chrono::Utc>);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_datetime_unix_milliseconds() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: string
                      format: date-time
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::with_config(
            IrGraph::new(&spec),
            &CodegenConfig {
                date_time_format: DateTimeFormat::UnixMilliseconds,
            },
        );
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected datetime; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(::ploidy_util::date_time::UnixMilliseconds);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_datetime_unix_seconds() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: string
                      format: date-time
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::with_config(
            IrGraph::new(&spec),
            &CodegenConfig {
                date_time_format: DateTimeFormat::UnixSeconds,
            },
        );
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected datetime; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(::ploidy_util::date_time::UnixSeconds);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_datetime_unix_microseconds() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: string
                      format: date-time
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::with_config(
            IrGraph::new(&spec),
            &CodegenConfig {
                date_time_format: DateTimeFormat::UnixMicroseconds,
            },
        );
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected datetime; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(::ploidy_util::date_time::UnixMicroseconds);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_datetime_unix_nanoseconds() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: string
                      format: date-time
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::with_config(
            IrGraph::new(&spec),
            &CodegenConfig {
                date_time_format: DateTimeFormat::UnixNanoseconds,
            },
        );
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected datetime; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(::ploidy_util::date_time::UnixNanoseconds);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_date() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: string
                      format: date
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected date; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(::chrono::NaiveDate);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_url() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: string
                      format: uri
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected url; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(::url::Url);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_uuid() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: string
                      format: uuid
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected uuid; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(::uuid::Uuid);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_bytes() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: string
                      format: byte
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected bytes; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(::ploidy_util::binary::Base64);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_primitive_binary() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Test:
                  type: object
                  required: [value]
                  properties:
                    value:
                      type: string
                      format: binary
        "})
        .unwrap();
        let spec = IrSpec::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(IrGraph::new(&spec));
        let primitives = graph.primitives().collect_vec();
        let [ty] = &*primitives else {
            panic!("expected binary; got `{primitives:?}`");
        };
        let p = CodegenPrimitive::new(ty);
        let actual: syn::Type = parse_quote!(#p);
        let expected: syn::Type = parse_quote!(::serde_bytes::ByteBuf);
        assert_eq!(actual, expected);
    }
}
