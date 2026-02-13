use oxc_ast::AstBuilder;
use oxc_ast::ast::TSType;
use oxc_span::SPAN;
use ploidy_core::ir::{
    ContainerView, ExtendableView, InlineIrTypePathRoot, InlineIrTypeView, IrTypeView,
};

use super::{
    emit::{array, nullable, record, type_ref},
    naming::{CodegenIdent, CodegenTypeName},
    primitive::ts_primitive,
};

/// Resolves an [`IrTypeView`] to a TypeScript type expression.
pub fn ts_type_ref<'a>(ast: &AstBuilder<'a>, ty: &IrTypeView<'_>) -> TSType<'a> {
    match ty {
        // Inline containers are emitted directly.
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Array(inner))) => {
            let inner_ty = inner.ty();
            array(ast, ts_type_ref(ast, &inner_ty))
        }
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Map(inner))) => {
            let inner_ty = inner.ty();
            record(ast, ts_type_ref(ast, &inner_ty))
        }
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Optional(inner))) => {
            let inner_ty = inner.ty();
            nullable(ast, ts_type_ref(ast, &inner_ty))
        }

        // Inline primitives are emitted directly.
        IrTypeView::Inline(InlineIrTypeView::Primitive(_, view)) => ts_primitive(ast, view.ty()),

        // Inline `Any` becomes `unknown`.
        IrTypeView::Inline(InlineIrTypeView::Any(_, _)) => ast.ts_type_unknown_keyword(SPAN),

        // Inline structured types use namespace-qualified names.
        IrTypeView::Inline(ty) => {
            let path = ty.path();
            let schema_name = match path.root {
                InlineIrTypePathRoot::Type(name) => CodegenIdent::new(name).to_type_name(),
                InlineIrTypePathRoot::Resource(_) => {
                    // Resource-rooted inlines come from operation
                    // request/response bodies. Use the bare inline name
                    // without a namespace prefix.
                    let inline_name = CodegenTypeName::Inline(ty).type_name();
                    return type_ref(ast, &inline_name);
                }
            };
            let inline_name = CodegenTypeName::Inline(ty).type_name();
            type_ref(ast, &format!("{schema_name}.{inline_name}"))
        }

        // Schema types are bare references.
        IrTypeView::Schema(ty) => {
            let ext = ty.extensions();
            let ident = ext.get::<CodegenIdent>().unwrap();
            type_ref(ast, &ident.to_type_name())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use oxc_allocator::Allocator;
    use ploidy_core::{
        ir::{Ir, IrStructFieldName, SchemaIrTypeView},
        parse::Document,
    };
    use pretty_assertions::assert_eq;

    use crate::{
        CodegenGraph,
        emit::{TsComments, emit_module, export_decl, type_alias_decl},
    };

    /// Emits a type as `export type T = <ty>;` and returns the output string.
    fn emit_ty(ty_fn: impl for<'a> FnOnce(&'a AstBuilder<'a>) -> TSType<'a>) -> String {
        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let comments = TsComments::new();
        let ty = ty_fn(&ast);
        let items = ast.vec1(export_decl(&ast, type_alias_decl(&ast, "T", ty), SPAN));
        emit_module(&allocator, &ast, items, &comments)
    }

    #[test]
    fn test_ref_schema_type() {
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

        let schema = graph
            .schemas()
            .find(|s| s.name() == "Pet")
            .expect("expected schema `Pet`");
        let ty = IrTypeView::Schema(schema);
        assert_eq!(
            emit_ty(|ast| ts_type_ref(ast, &ty)),
            "export type T = Pet;\n"
        );
    }

    #[test]
    fn test_ref_inline_array_of_strings() {
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
                  required:
                    - items
                  properties:
                    items:
                      type: array
                      items:
                        type: string
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), IrStructFieldName::Name("items")))
            .unwrap();
        let ty = field.ty();
        assert_eq!(
            emit_ty(|ast| ts_type_ref(ast, &ty)),
            "export type T = string[];\n"
        );
    }

    #[test]
    fn test_ref_inline_map_of_strings() {
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
                  required:
                    - metadata
                  properties:
                    metadata:
                      type: object
                      additionalProperties:
                        type: string
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), IrStructFieldName::Name("metadata")))
            .unwrap();
        let ty = field.ty();
        assert_eq!(
            emit_ty(|ast| ts_type_ref(ast, &ty)),
            "export type T = Record<string, string>;\n"
        );
    }

    #[test]
    fn test_ref_nullable_string() {
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
                    value:
                      type: string
                      nullable: true
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), IrStructFieldName::Name("value")))
            .unwrap();
        let ty = field.ty();
        assert_eq!(
            emit_ty(|ast| ts_type_ref(ast, &ty)),
            "export type T = string | null;\n"
        );
    }

    #[test]
    fn test_ref_any_type() {
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
                  required:
                    - data
                  properties:
                    data: {}
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), IrStructFieldName::Name("data")))
            .unwrap();
        let ty = field.ty();
        assert_eq!(
            emit_ty(|ast| ts_type_ref(ast, &ty)),
            "export type T = unknown;\n"
        );
    }

    #[test]
    fn test_ref_inline_struct() {
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
                  required:
                    - nested
                  properties:
                    nested:
                      type: object
                      properties:
                        value:
                          type: string
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };
        let field = struct_view
            .fields()
            .find(|f| matches!(f.name(), IrStructFieldName::Name("nested")))
            .unwrap();
        let ty = field.ty();
        assert_eq!(
            emit_ty(|ast| ts_type_ref(ast, &ty)),
            "export type T = Container.Nested;\n"
        );
    }
}
