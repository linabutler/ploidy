use itertools::Itertools;
use ploidy_core::ir::{IrEnumVariant, IrEnumView};
use quasiquodo_ts::{swc::ecma_ast::TsType, ts_quote};

/// Resolves an enum to a TypeScript type expression (a union of
/// literals, or `string` for unrepresentable variants).
pub fn ts_enum_type(ty: &IrEnumView<'_>) -> TsType {
    let mut types = ty.variants().iter().map(|variant| match variant {
        IrEnumVariant::String(s) => ts_quote!("#{s}" as TsType, s: &str = s),
        IrEnumVariant::Number(n) => {
            ts_quote!("#{n}" as TsType, n: f64 = n.as_f64().unwrap_or(0.0))
        }
        IrEnumVariant::Bool(true) => ts_quote!("true" as TsType),
        IrEnumVariant::Bool(false) => ts_quote!("false" as TsType),
    });
    let Some(first) = types.next() else {
        return ts_quote!("never" as TsType);
    };
    ts_quote!(
        "#{first} | #{rest}" as TsType,
        first: TsType = first,
        rest: Vec<Box<TsType>> = types.map(Box::new).collect_vec(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        codegen::Code,
        ir::{Ir, SchemaIrTypeView},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use quasiquodo_ts::{Comments, swc::ecma_ast::Module, ts_quote};

    use crate::{CodegenGraph, TsSource, naming::CodegenTypeName};

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

        let name = CodegenTypeName::Schema(schema).display().to_string();
        let comments = Comments::new();
        let ty = ts_enum_type(enum_view);
        let items = vec![ts_quote!(
            "export type #{n} = #{t}" as ModuleItem,
            n: Ident = name,
            t: TsType = ty
        )];
        assert_eq!(
            TsSource::new(
                String::new(),
                comments,
                Module {
                    body: items,
                    ..Module::default()
                }
            )
            .into_string()
            .unwrap(),
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

        let name = CodegenTypeName::Schema(schema).display().to_string();

        // Description is handled by the caller (schema.rs) via Comments.
        let comments = Comments::new();
        let ty = ts_enum_type(enum_view);
        let items = vec![ts_quote!(
            "export type #{n} = #{t}" as ModuleItem,
            n: Ident = name,
            t: TsType = ty
        )];
        assert_eq!(
            TsSource::new(
                String::new(),
                comments,
                Module {
                    body: items,
                    ..Module::default()
                }
            )
            .into_string()
            .unwrap(),
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

        let name = CodegenTypeName::Schema(schema).display().to_string();
        let comments = Comments::new();
        let ty = ts_enum_type(enum_view);
        let items = vec![ts_quote!(
            "export type #{n} = #{t}" as ModuleItem,
            n: Ident = name,
            t: TsType = ty
        )];
        assert_eq!(
            TsSource::new(
                String::new(),
                comments,
                Module {
                    body: items,
                    ..Module::default()
                }
            )
            .into_string()
            .unwrap(),
            "export type Priority = 1 | 2 | 3;\n"
        );
    }
}
