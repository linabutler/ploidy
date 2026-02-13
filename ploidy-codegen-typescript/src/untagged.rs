use oxc_ast::AstBuilder;
use oxc_ast::ast::Declaration;
use oxc_span::SPAN;
use ploidy_core::ir::IrUntaggedView;

use super::{
    emit::{type_alias_decl, union},
    ref_::ts_type_ref,
};

/// Generates a TypeScript union type from an untagged union.
pub fn ts_untagged<'a>(
    ast: &AstBuilder<'a>,
    name: &str,
    ty: &IrUntaggedView<'_>,
) -> Declaration<'a> {
    let members = ast.vec_from_iter(ty.variants().map(|variant| match variant.ty() {
        Some(variant) => ts_type_ref(ast, &variant.view),
        None => ast.ts_type_null_keyword(SPAN),
    }));

    type_alias_decl(ast, name, union(ast, members))
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

        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_untagged(&ast, &name, untagged_view);
        let comments = TsComments::new();
        let items = ast.vec1(export_decl(&ast, decl, SPAN));
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
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

        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_untagged(&ast, &name, untagged_view);
        let comments = TsComments::new();
        let items = ast.vec1(export_decl(&ast, decl, SPAN));
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
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

        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_untagged(&ast, &name, untagged_view);

        // Description is handled by the caller (schema.rs) via TsComments.
        let comments = TsComments::new();
        let items = ast.vec1(export_decl(&ast, decl, SPAN));
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
            "export type StringOrInt = string | number;\n"
        );
    }
}
