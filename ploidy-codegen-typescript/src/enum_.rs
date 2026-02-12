use ploidy_core::ir::{IrEnumVariant, IrEnumView};
use swc_ecma_ast::{Decl, TsKeywordTypeKind};

use super::emit::{kw, lit_bool, lit_num, lit_str, type_alias_decl, union};

/// Generates a TypeScript literal union type from an enum.
pub fn ts_enum(name: &str, ty: &IrEnumView<'_>) -> Decl {
    let has_unrepresentable = ty.variants().iter().any(|variant| match variant {
        IrEnumVariant::Number(_) | IrEnumVariant::Bool(_) => true,
        IrEnumVariant::String(s) => s.chars().all(|c| !unicode_ident::is_xid_continue(c)),
    });

    if has_unrepresentable {
        // Fall back to `string` for enums with unrepresentable variants.
        return type_alias_decl(name, kw(TsKeywordTypeKind::TsStringKeyword));
    }

    let variants = ty
        .variants()
        .iter()
        .map(|variant| match variant {
            IrEnumVariant::String(s) => lit_str(s),
            IrEnumVariant::Number(n) => lit_num(&n.to_string()),
            IrEnumVariant::Bool(b) => lit_bool(*b),
        })
        .collect();

    type_alias_decl(name, union(variants))
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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Status");
        let Some(schema @ SchemaIrTypeView::Enum(_, enum_view)) = &schema else {
            panic!("expected enum `Status`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_enum(&name, enum_view);
        let comments = TsComments::new();
        let items = vec![export_decl(decl, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
            "export type Status = \"active\" | \"inactive\" | \"pending\";\n"
        );
    }

    #[test]
    fn test_enum_with_description() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Status:
                  description: The status of a resource.
                  type: string
                  enum:
                    - active
                    - inactive
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Status");
        let Some(schema @ SchemaIrTypeView::Enum(_, enum_view)) = &schema else {
            panic!("expected enum `Status`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_enum(&name, enum_view);

        // Description is handled by the caller (schema.rs) via TsComments.
        let comments = TsComments::new();
        let items = vec![export_decl(decl, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
            "export type Status = \"active\" | \"inactive\";\n"
        );
    }

    #[test]
    fn test_enum_unrepresentable_becomes_string() {
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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Priority");
        let Some(schema @ SchemaIrTypeView::Enum(_, enum_view)) = &schema else {
            panic!("expected enum `Priority`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_enum(&name, enum_view);
        let comments = TsComments::new();
        let items = vec![export_decl(decl, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
            "export type Priority = string;\n"
        );
    }
}
