use itertools::Itertools;
use ploidy_core::ir::IrTaggedView;
use quasiquodo_ts::{Comments, swc::ecma_ast::TsType, ts_quote};

use super::ref_::ts_type_ref;

/// Resolves a tagged union to a TypeScript type expression (a union
/// of `{ tag: 'value' } & VariantRef` intersections).
pub fn ts_tagged_type(ty: &IrTaggedView<'_>, comments: &Comments) -> TsType {
    let tag = ty.tag();
    let mut types = ty.variants().map(|variant| {
        let view = variant.ty();
        let variant_ref = ts_type_ref(&view, comments);

        let discriminator_value = variant.aliases().first().copied().unwrap_or(variant.name());

        let tag_object = ts_quote!(
            "{ #{tag}: #{v}; }" as TsType,
            tag: &str = tag,
            v: &str = discriminator_value
        );

        ts_quote!(
            "#{a} & #{b}" as TsType,
            a: TsType = tag_object,
            b: TsType = variant_ref,
        )
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
    fn test_tagged_union() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
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
                    propertyName: petType
                    mapping:
                      dog: '#/components/schemas/Dog'
                      cat: '#/components/schemas/Cat'
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Tagged(_, tagged)) = &schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).display().to_string();
        let comments = Comments::new();
        let ty = ts_tagged_type(tagged, &comments);
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
            indoc::indoc! {r#"
                export type Pet = {
                  petType: "dog";
                } & Dog | {
                  petType: "cat";
                } & Cat;
            "#}
        );
    }

    #[test]
    fn test_tagged_union_with_rename() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
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
                      canine: '#/components/schemas/Dog'
                      feline: '#/components/schemas/Cat'
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Tagged(_, tagged)) = &schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).display().to_string();
        let comments = Comments::new();
        let ty = ts_tagged_type(tagged, &comments);
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
            indoc::indoc! {r#"
                export type Pet = {
                  type: "canine";
                } & Dog | {
                  type: "feline";
                } & Cat;
            "#}
        );
    }

    #[test]
    fn test_tagged_union_with_description() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
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
                  description: Represents different types of pets
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                    - $ref: '#/components/schemas/Cat'
                  discriminator:
                    propertyName: type
                    mapping:
                      dog: '#/components/schemas/Dog'
                      cat: '#/components/schemas/Cat'
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Tagged(_, tagged)) = &schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).display().to_string();

        // Description is handled by the caller (schema.rs) via Comments.
        let comments = Comments::new();
        let ty = ts_tagged_type(tagged, &comments);
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
            indoc::indoc! {r#"
                export type Pet = {
                  type: "dog";
                } & Dog | {
                  type: "cat";
                } & Cat;
            "#}
        );
    }
}
