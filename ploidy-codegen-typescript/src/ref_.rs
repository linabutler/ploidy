use ploidy_core::ir::{
    ContainerView, ExtendableView, InlineIrTypePathRoot, InlineIrTypeView, IrTypeView,
};
use swc_ecma_ast::{TsKeywordTypeKind, TsType};

use super::{
    emit::{array, kw, nullable, record, type_ref},
    naming::{CodegenIdent, CodegenTypeName},
    primitive::ts_primitive,
};

/// Resolves an [`IrTypeView`] to a TypeScript type expression.
pub fn ts_type_ref(ty: &IrTypeView<'_>) -> Box<TsType> {
    match ty {
        // Inline containers are emitted directly.
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Array(inner))) => {
            let inner_ty = inner.ty();
            array(ts_type_ref(&inner_ty))
        }
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Map(inner))) => {
            let inner_ty = inner.ty();
            record(ts_type_ref(&inner_ty))
        }
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Optional(inner))) => {
            let inner_ty = inner.ty();
            nullable(ts_type_ref(&inner_ty))
        }

        // Inline primitives are emitted directly.
        IrTypeView::Inline(InlineIrTypeView::Primitive(_, view)) => ts_primitive(view.ty()),

        // Inline `Any` becomes `unknown`.
        IrTypeView::Inline(InlineIrTypeView::Any(_, _)) => kw(TsKeywordTypeKind::TsUnknownKeyword),

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
                    return type_ref(&inline_name);
                }
            };
            let inline_name = CodegenTypeName::Inline(ty).type_name();
            type_ref(&format!("{schema_name}.{inline_name}"))
        }

        // Schema types are bare references.
        IrTypeView::Schema(ty) => {
            let ext = ty.extensions();
            let ident = ext.get::<CodegenIdent>().unwrap();
            type_ref(&ident.to_type_name())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        ir::{Ir, IrStructFieldName, SchemaIrTypeView},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use swc_common::DUMMY_SP;

    use crate::{
        CodegenGraph,
        emit::{TsComments, emit_module, export_decl, type_alias_decl},
    };

    /// Emits a type as `export type T = <ty>;` and returns the output string.
    fn emit_ty(ty: Box<TsType>) -> String {
        let comments = TsComments::new();
        let items = vec![export_decl(type_alias_decl("T", ty), DUMMY_SP)];
        emit_module(items, &comments)
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
        let ts = ts_type_ref(&ty);
        assert_eq!(emit_ty(ts), "export type T = Pet;\n");
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
        let ts = ts_type_ref(&ty);
        assert_eq!(emit_ty(ts), "export type T = string[];\n");
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
        let ts = ts_type_ref(&ty);
        assert_eq!(emit_ty(ts), "export type T = Record<string, string>;\n");
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
        let ts = ts_type_ref(&ty);
        assert_eq!(emit_ty(ts), "export type T = string | null;\n");
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
        let ts = ts_type_ref(&ty);
        assert_eq!(emit_ty(ts), "export type T = unknown;\n");
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
        let ts = ts_type_ref(&ty);
        assert_eq!(emit_ty(ts), "export type T = Container.Nested;\n");
    }
}
