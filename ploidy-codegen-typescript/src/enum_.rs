use oxc_ast::AstBuilder;
use oxc_ast::ast::Declaration;
use oxc_span::SPAN;
use ploidy_core::ir::{IrEnumVariant, IrEnumView};

use super::emit::{lit_bool, lit_num, lit_str, type_alias_decl, union};

/// Generates a TypeScript literal union type from an enum.
pub fn ts_enum<'a>(ast: &AstBuilder<'a>, name: &str, ty: &IrEnumView<'_>) -> Declaration<'a> {
    let has_unrepresentable = ty.variants().iter().any(|variant| match variant {
        IrEnumVariant::Number(_) | IrEnumVariant::Bool(_) => true,
        IrEnumVariant::String(s) => s.chars().all(|c| !unicode_ident::is_xid_continue(c)),
    });

    if has_unrepresentable {
        // Fall back to `string` for enums with unrepresentable variants.
        return type_alias_decl(ast, name, ast.ts_type_string_keyword(SPAN));
    }

    let variants = ast.vec_from_iter(ty.variants().iter().map(|variant| match variant {
        IrEnumVariant::String(s) => lit_str(ast, s),
        IrEnumVariant::Number(n) => lit_num(ast, &n.to_string()),
        IrEnumVariant::Bool(b) => lit_bool(ast, *b),
    }));

    type_alias_decl(ast, name, union(ast, variants))
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

        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_enum(&ast, &name, enum_view);
        let comments = TsComments::new();
        let items = ast.vec1(export_decl(&ast, decl, SPAN));
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
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

        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_enum(&ast, &name, enum_view);

        // Description is handled by the caller (schema.rs) via TsComments.
        let comments = TsComments::new();
        let items = ast.vec1(export_decl(&ast, decl, SPAN));
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
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

        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_enum(&ast, &name, enum_view);
        let comments = TsComments::new();
        let items = ast.vec1(export_decl(&ast, decl, SPAN));
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
            "export type Priority = string;\n"
        );
    }
}
