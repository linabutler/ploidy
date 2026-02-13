use oxc_ast::AstBuilder;
use oxc_ast::ast::{Declaration, TSType};
use oxc_span::SPAN;
use ploidy_core::ir::IrTaggedView;

use super::{
    emit::{TsComments, intersection, lit_str, property_sig, type_alias_decl, type_lit, union},
    ref_::ts_type_ref,
};

/// Resolves a tagged union to a TypeScript type expression (a union
/// of `{ tag: 'value' } & VariantRef` intersections).
pub fn ts_tagged_type<'a>(
    ast: &AstBuilder<'a>,
    ty: &IrTaggedView<'_>,
    comments: &TsComments,
) -> TSType<'a> {
    let tag = ty.tag();
    let members = ast.vec_from_iter(ty.variants().map(|variant| {
        let view = variant.ty();
        let variant_ref = ts_type_ref(ast, &view, comments);

        let discriminator_value = variant.aliases().first().copied().unwrap_or(variant.name());

        let tag_object = type_lit(
            ast,
            ast.vec1(property_sig(
                ast,
                tag,
                false,
                lit_str(ast, discriminator_value),
                SPAN,
            )),
        );

        let parts = ast.vec_from_array([tag_object, variant_ref]);
        intersection(ast, parts)
    }));

    union(ast, members)
}

/// Generates a TypeScript discriminated union type alias from a
/// tagged union.
///
/// Each variant becomes `{ <tag>: '<value>' } & VariantRef`, and the
/// variants are joined with `|`. This enables TypeScript's type
/// narrowing on the discriminator field.
pub fn ts_tagged<'a>(
    ast: &AstBuilder<'a>,
    name: &str,
    ty: &IrTaggedView<'_>,
    comments: &TsComments,
) -> Declaration<'a> {
    type_alias_decl(ast, name, ts_tagged_type(ast, ty, comments))
}

#[cfg(test)]
mod tests {
    use super::*;

    use oxc_allocator::Allocator;
    use ploidy_core::{
        ir::{Ir, SchemaIrTypeView},
        parse::Document,
    };
    use pretty_assertions::assert_eq;

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

        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let name = CodegenTypeName::Schema(schema).type_name();
        let comments = TsComments::new();
        let decl = ts_tagged(&ast, &name, tagged, &comments);
        let items = ast.vec1(export_decl(&ast, decl, SPAN));
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
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

        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let name = CodegenTypeName::Schema(schema).type_name();
        let comments = TsComments::new();
        let decl = ts_tagged(&ast, &name, tagged, &comments);
        let items = ast.vec1(export_decl(&ast, decl, SPAN));
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
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

        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let name = CodegenTypeName::Schema(schema).type_name();

        // Description is handled by the caller (schema.rs) via TsComments.
        let comments = TsComments::new();
        let decl = ts_tagged(&ast, &name, tagged, &comments);
        let items = ast.vec1(export_decl(&ast, decl, SPAN));
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
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
