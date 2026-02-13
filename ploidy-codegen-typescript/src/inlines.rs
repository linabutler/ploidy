use itertools::Itertools;
use oxc_ast::AstBuilder;
use oxc_ast::ast::Declaration;
use oxc_span::SPAN;
use ploidy_core::ir::{InlineIrTypeView, SchemaIrTypeView, View};

use super::{
    emit::{TsComments, export_decl, namespace_decl},
    enum_::ts_enum,
    naming::{CodegenTypeName, CodegenTypeNameSortKey},
    struct_::ts_struct,
    tagged::ts_tagged,
    untagged::ts_untagged,
};

/// Collects inline type declarations for a schema and wraps them in a
/// `namespace`.
///
/// Returns `None` if there are no inline types to emit.
pub fn ts_inlines<'a>(
    ast: &AstBuilder<'a>,
    schema: &SchemaIrTypeView<'_>,
    comments: &TsComments,
) -> Option<Declaration<'a>> {
    let mut inlines = schema.inlines().collect_vec();
    inlines.sort_by(|a, b| {
        CodegenTypeNameSortKey::for_inline(a).cmp(&CodegenTypeNameSortKey::for_inline(b))
    });

    let body = ast.vec_from_iter(
        inlines
            .into_iter()
            .filter_map(|view| {
                let name = CodegenTypeName::Inline(&view).type_name();
                match &view {
                    InlineIrTypeView::Enum(_, view) => Some(ts_enum(ast, &name, view)),
                    InlineIrTypeView::Struct(_, view) => {
                        Some(ts_struct(ast, &name, view, comments))
                    }
                    InlineIrTypeView::Tagged(_, view) => Some(ts_tagged(ast, &name, view)),
                    InlineIrTypeView::Untagged(_, view) => Some(ts_untagged(ast, &name, view)),
                    // Container types, primitive types, and untyped values
                    // are emitted directly; they don't need type aliases.
                    InlineIrTypeView::Container(..)
                    | InlineIrTypeView::Primitive(..)
                    | InlineIrTypeView::Any(..) => None,
                }
            })
            .map(|decl| export_decl(ast, decl, SPAN)),
    );

    if body.is_empty() {
        return None;
    }

    let schema_name = CodegenTypeName::Schema(schema).type_name();
    Some(namespace_decl(ast, &schema_name, body))
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
        emit::{TsComments, emit_module},
    };

    #[test]
    fn test_inlines_with_inline_structs() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Container:
                  type: object
                  properties:
                    zebra:
                      type: object
                      properties:
                        name:
                          type: string
                    apple:
                      type: object
                      properties:
                        name:
                          type: string
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(schema @ SchemaIrTypeView::Struct(_, _)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };

        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let comments = TsComments::new();
        let ns = ts_inlines(&ast, schema, &comments).expect("expected inline types");
        // Inline types should be sorted alphabetically: Apple, Zebra.
        let items = ast.vec1(export_decl(&ast, ns, SPAN));
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
            indoc::indoc! {"
                export namespace Container {
                  export interface Apple {
                    name?: string;
                  }
                  export interface Zebra {
                    name?: string;
                  }
                }
            "}
        );
    }

    #[test]
    fn test_inlines_none_when_no_inline_types() {
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
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Struct(_, _)) = &schema else {
            panic!("expected struct `Pet`; got `{schema:?}`");
        };

        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let comments = TsComments::new();
        assert!(ts_inlines(&ast, schema, &comments).is_none());
    }

    #[test]
    fn test_inlines_skips_container_primitives() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Container:
                  type: object
                  properties:
                    tags:
                      type: array
                      items:
                        type: string
                    count:
                      type: integer
                      format: int32
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(schema @ SchemaIrTypeView::Struct(_, _)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };

        // Container and primitive inlines should be skipped.
        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let comments = TsComments::new();
        assert!(ts_inlines(&ast, schema, &comments).is_none());
    }
}
