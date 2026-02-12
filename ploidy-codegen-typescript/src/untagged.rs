use itertools::Itertools;
use ploidy_core::ir::IrUntaggedView;
use quasiquodo_ts::{Comments, swc::ecma_ast::TsType, ts_quote};

use super::ref_::ts_type_ref;

/// Resolves an untagged union to a TypeScript union type expression.
pub fn ts_untagged_type(ty: &IrUntaggedView<'_>, comments: &Comments) -> TsType {
    let mut types = ty.variants().map(|variant| match variant.ty() {
        Some(variant) => ts_type_ref(&variant.view, comments),
        None => ts_quote!("null" as TsType),
    });
    let Some(first) = types.next() else {
        return ts_quote!("never" as TsType);
    };
    ts_quote!(
        "#{first} | #{rest}" as TsType,
        first: TsType = first,
        rest: Vec<Box<TsType>> = types.map(Box::new).collect_vec(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        codegen::Code,
        ir::{Ir, SchemaIrTypeView},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use quasiquodo_ts::{Comments, swc::ecma_ast::Module};

    use crate::{CodegenGraph, TsSource, naming::CodegenTypeName};

    #[test]
    fn test_untagged_union_primitives() {
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
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "StringOrInt");
        let Some(schema @ SchemaIrTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `StringOrInt`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).display().to_string();
        let comments = Comments::new();
        let ty = ts_untagged_type(untagged_view, &comments);
        let items = vec![ts_quote!(
            "export type #{n} = #{t}" as ModuleItem,
            n: Ident = name,
            t: TsType = ty
        )];
        assert_eq!(
            TsSource::new(
                String::new(),
                comments,
                Module {
                    body: items,
                    ..Module::default()
                }
            )
            .into_string()
            .unwrap(),
            "export type StringOrInt = string | number;\n"
        );
    }

    #[test]
    fn test_untagged_union_with_refs() {
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
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Animal");
        let Some(schema @ SchemaIrTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `Animal`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).display().to_string();
        let comments = Comments::new();
        let ty = ts_untagged_type(untagged_view, &comments);
        let items = vec![ts_quote!(
            "export type #{n} = #{t}" as ModuleItem,
            n: Ident = name,
            t: TsType = ty
        )];
        assert_eq!(
            TsSource::new(
                String::new(),
                comments,
                Module {
                    body: items,
                    ..Module::default()
                }
            )
            .into_string()
            .unwrap(),
            "export type Animal = Dog | Cat;\n"
        );
    }

    #[test]
    fn test_untagged_union_with_description() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                StringOrInt:
                  description: A union that can be either a string or an integer.
                  oneOf:
                    - type: string
                    - type: integer
                      format: int32
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "StringOrInt");
        let Some(schema @ SchemaIrTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `StringOrInt`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).display().to_string();

        // Description is handled by the caller (schema.rs) via Comments.
        let comments = Comments::new();
        let ty = ts_untagged_type(untagged_view, &comments);
        let items = vec![ts_quote!(
            "export type #{n} = #{t}" as ModuleItem,
            n: Ident = name,
            t: TsType = ty
        )];
        assert_eq!(
            TsSource::new(
                String::new(),
                comments,
                Module {
                    body: items,
                    ..Module::default()
                }
            )
            .into_string()
            .unwrap(),
            "export type StringOrInt = string | number;\n"
        );
    }
}
