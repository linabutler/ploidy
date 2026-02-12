use itertools::Itertools;
use ploidy_core::ir::{InlineIrTypeView, SchemaIrTypeView, View};
use swc_common::DUMMY_SP;
use swc_ecma_ast::Decl;

use super::{
    emit::{export_decl, namespace_decl},
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
pub fn ts_inlines(schema: &SchemaIrTypeView<'_>) -> Option<Decl> {
    let mut inlines = schema.inlines().collect_vec();
    inlines.sort_by(|a, b| {
        CodegenTypeNameSortKey::for_inline(a).cmp(&CodegenTypeNameSortKey::for_inline(b))
    });

    let body = inlines
        .into_iter()
        .filter_map(|view| {
            let name = CodegenTypeName::Inline(&view).type_name();
            match &view {
                InlineIrTypeView::Enum(_, view) => Some(ts_enum(&name, view)),
                InlineIrTypeView::Struct(_, view) => Some(ts_struct(&name, view)),
                InlineIrTypeView::Tagged(_, view) => Some(ts_tagged(&name, view)),
                InlineIrTypeView::Untagged(_, view) => Some(ts_untagged(&name, view)),
                // Container types, primitive types, and untyped values
                // are emitted directly; they don't need type aliases.
                InlineIrTypeView::Container(..)
                | InlineIrTypeView::Primitive(..)
                | InlineIrTypeView::Any(..) => None,
            }
        })
        .map(|decl| export_decl(decl, DUMMY_SP))
        .collect::<Vec<_>>();

    if body.is_empty() {
        return None;
    }

    let schema_name = CodegenTypeName::Schema(schema).type_name();
    Some(namespace_decl(&schema_name, body))
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let ns = ts_inlines(schema).expect("expected inline types");
        let comments = TsComments::new();
        // Inline types should be sorted alphabetically: Apple, Zebra.
        let items = vec![export_decl(ns, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
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

        assert!(ts_inlines(schema).is_none());
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
        assert!(ts_inlines(schema).is_none());
    }
}
