use ploidy_core::ir::IrTaggedView;
use swc_common::DUMMY_SP;
use swc_ecma_ast::Decl;

use super::{
    emit::{intersection, lit_str, property_sig, type_alias_decl, type_lit, union},
    ref_::ts_type_ref,
};

/// Generates a TypeScript discriminated union from a tagged union.
///
/// Each variant becomes `{ <tag>: '<value>' } & VariantRef`, and the
/// variants are joined with `|`. This enables TypeScript's type narrowing
/// on the discriminator field.
pub fn ts_tagged(name: &str, ty: &IrTaggedView<'_>) -> Decl {
    let tag = ty.tag();
    let mut members = Vec::new();

    for variant in ty.variants() {
        let view = variant.ty();
        let variant_ref = ts_type_ref(&view);

        // Use the first alias as the discriminator value.
        let discriminator_value = variant.aliases().first().copied().unwrap_or(variant.name());

        // Build `{ tag: 'value' } & VariantRef`.
        let tag_object = type_lit(vec![property_sig(
            tag,
            false,
            lit_str(discriminator_value),
            DUMMY_SP,
        )]);

        members.push(intersection(vec![tag_object, variant_ref]));
    }

    type_alias_decl(name, union(members))
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        ir::{Ir, SchemaIrTypeView},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use swc_common::DUMMY_SP;

    use crate::{
        CodegenGraph,
        emit::{TsComments, emit_module, export_decl},
        naming::CodegenTypeName,
    };

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

        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_tagged(&name, tagged);
        let comments = TsComments::new();
        let items = vec![export_decl(decl, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
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

        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_tagged(&name, tagged);
        let comments = TsComments::new();
        let items = vec![export_decl(decl, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
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

        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_tagged(&name, tagged);

        // Description is handled by the caller (schema.rs) via TsComments.
        let comments = TsComments::new();
        let items = vec![export_decl(decl, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
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
