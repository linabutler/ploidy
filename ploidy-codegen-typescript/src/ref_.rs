use ploidy_core::ir::{ContainerView, ExtendableView, InlineIrTypeView, IrTypeView};
use quasiquodo_ts::{Comments, swc::ecma_ast::TsType, ts_quote};

use super::{
    enum_::ts_enum_type,
    naming::{CodegenIdent, CodegenIdentUsage},
    primitive::ts_primitive,
    struct_::ts_struct_type,
    tagged::ts_tagged_type,
    untagged::ts_untagged_type,
};

/// Resolves an [`IrTypeView`] to a TypeScript type expression.
pub fn ts_type_ref(ty: &IrTypeView<'_>, comments: &Comments) -> TsType {
    match ty {
        // Inline containers are emitted directly.
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Array(inner))) => {
            let inner_ty = inner.ty();
            let elem = ts_type_ref(&inner_ty, comments);
            match elem {
                TsType::TsUnionOrIntersectionType(_) => {
                    ts_quote!("(#{ty})[]" as TsType, ty: TsType = elem)
                }
                _ => ts_quote!("#{ty}[]" as TsType, ty: TsType = elem),
            }
        }
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Map(inner))) => {
            let inner_ty = inner.ty();
            let v = ts_type_ref(&inner_ty, comments);
            ts_quote!("Record<string, #{v}>" as TsType, v: TsType = v)
        }
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Optional(inner))) => {
            let inner_ty = inner.ty();
            let t = ts_type_ref(&inner_ty, comments);
            ts_quote!("#{t} | null" as TsType, t: TsType = t)
        }

        // Inline primitives are emitted directly.
        IrTypeView::Inline(InlineIrTypeView::Primitive(_, view)) => ts_primitive(view.ty()),

        // Inline `Any` becomes `unknown`.
        IrTypeView::Inline(InlineIrTypeView::Any(_, _)) => ts_quote!("unknown" as TsType),

        // Inline structured types are expanded in place.
        IrTypeView::Inline(InlineIrTypeView::Struct(_, view)) => ts_struct_type(view, comments),
        IrTypeView::Inline(InlineIrTypeView::Enum(_, view)) => ts_enum_type(view),
        IrTypeView::Inline(InlineIrTypeView::Tagged(_, view)) => ts_tagged_type(view, comments),
        IrTypeView::Inline(InlineIrTypeView::Untagged(_, view)) => ts_untagged_type(view, comments),

        // Schema types are bare references.
        IrTypeView::Schema(ty) => {
            let ext = ty.extensions();
            let ident = ext.get::<CodegenIdent>().unwrap();
            let type_name = CodegenIdentUsage::Type(&ident).display().to_string();
            ts_quote!("#{name}" as TsType, name: Ident = type_name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        codegen::Code,
        ir::{Ir, IrStructFieldName, SchemaIrTypeView},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use quasiquodo_ts::{Comments, swc::ecma_ast::Module};

    use crate::{CodegenGraph, TsSource};

    /// Emits a type as `export type T = <ty>;` and returns the output string.
    fn emit_ty(ty_fn: impl FnOnce(&Comments) -> TsType) -> String {
        let comments = Comments::new();
        let ty = ty_fn(&comments);
        let items = vec![quasiquodo_ts::ts_quote!(
            "export type T = #{t}" as ModuleItem,
            t: TsType = ty
        )];
        TsSource::new(
            String::new(),
            comments,
            Module {
                body: items,
                ..Module::default()
            },
        )
        .into_string()
        .unwrap()
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
            emit_ty(|comments| ts_type_ref(&ty, comments)),
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
            emit_ty(|comments| ts_type_ref(&ty, comments)),
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
            emit_ty(|comments| ts_type_ref(&ty, comments)),
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
                  required:
                    - value
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
            emit_ty(|comments| ts_type_ref(&ty, comments)),
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
            emit_ty(|comments| ts_type_ref(&ty, comments)),
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
            emit_ty(|comments| ts_type_ref(&ty, comments)),
            "export type T = {\n  value?: string;\n};\n"
        );
    }
}
