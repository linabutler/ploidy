use ploidy_core::ir::IrUntaggedView;
use swc_ecma_ast::{Decl, TsKeywordTypeKind};

use super::{
    emit::{kw, type_alias_decl, union},
    ref_::ts_type_ref,
};

/// Generates a TypeScript union type from an untagged union.
pub fn ts_untagged(name: &str, ty: &IrUntaggedView<'_>) -> Decl {
    let mut members = Vec::new();

    for variant in ty.variants() {
        match variant.ty() {
            Some(variant) => {
                members.push(ts_type_ref(&variant.view));
            }
            None => {
                members.push(kw(TsKeywordTypeKind::TsNullKeyword));
            }
        }
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

        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_untagged(&name, untagged_view);
        let comments = TsComments::new();
        let items = vec![export_decl(decl, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
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

        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_untagged(&name, untagged_view);
        let comments = TsComments::new();
        let items = vec![export_decl(decl, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
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

        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_untagged(&name, untagged_view);

        // Description is handled by the caller (schema.rs) via TsComments.
        let comments = TsComments::new();
        let items = vec![export_decl(decl, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
            "export type StringOrInt = string | number;\n"
        );
    }
}
