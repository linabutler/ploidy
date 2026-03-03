//! Python enum generation from IR enums.

use std::collections::BTreeSet;

use ploidy_core::ir::{IrEnumVariant, IrEnumView};
use quasiquodo_py::{
    py_quote,
    ruff::{
        python_ast::{Identifier, Suite},
        text_size::TextRange,
    },
};

use super::naming::{CodegenIdent, CodegenIdentUsage, CodegenTypeName};

/// Generates a Python `Enum` class from an IR enum.
#[derive(Clone, Debug)]
pub struct CodegenEnum<'a> {
    name: CodegenTypeName<'a>,
    ty: &'a IrEnumView<'a>,
}

impl<'a> CodegenEnum<'a> {
    pub fn new(name: CodegenTypeName<'a>, ty: &'a IrEnumView<'a>) -> Self {
        Self { name, ty }
    }

    /// Generates the enum definition.
    pub fn to_suite(&self) -> Suite {
        // Check if all variants can be represented as Python enum members.
        // Non-string variants, and string variants without at least one
        // usable identifier character, can't be valid Python identifiers.
        let has_unrepresentable = self.ty.variants().iter().any(|variant| match variant {
            IrEnumVariant::Number(_) | IrEnumVariant::Bool(_) => true,
            IrEnumVariant::String(s) => !s.chars().any(unicode_ident::is_xid_continue),
        });

        let class_name = self.name.as_class_name();
        let name_ident = Identifier::new(&class_name, TextRange::default());

        if has_unrepresentable {
            // If any variant is unrepresentable, emit a type alias for
            // the union of all variant types.
            let types: BTreeSet<_> = self
                .ty
                .variants()
                .iter()
                .map(|variant| match variant {
                    IrEnumVariant::String(_) => "str",
                    IrEnumVariant::Number(n) => {
                        if n.is_i64() || n.is_u64() {
                            "int"
                        } else {
                            "float"
                        }
                    }
                    IrEnumVariant::Bool(_) => "bool",
                })
                .collect();
            let union_ty = types
                .into_iter()
                .map(|ty| {
                    py_quote!(
                        "#{ty}" as Expr,
                        ty: Identifier = Identifier::new(ty, TextRange::default())
                    )
                })
                .reduce(|a, b| py_quote!("#{a} | #{b}" as Expr, a: Expr = a, b: Expr = b))
                .unwrap_or_else(|| py_quote!("never" as Expr));
            py_quote!(
                {"
                    #{desc}
                    #{name} = #{ty}
                "} as Suite,
                desc: Option<&str> = self.ty.description(),
                name: Identifier = name_ident,
                ty: Expr = union_ty,
            )
        } else {
            // Otherwise, emit an enum class.
            let class_body = self
                .ty
                .variants()
                .iter()
                .filter_map(|variant| match variant {
                    &IrEnumVariant::String(name) => Some(name),
                    _ => None,
                })
                .map(|name| {
                    let ident = CodegenIdent::new(name);
                    let variant_name = CodegenIdentUsage::Variant(&ident).display().to_string();
                    py_quote!(
                        "#{name} = #{value}" as Stmt,
                        name: Identifier = Identifier::new(&variant_name, TextRange::default()),
                        value: &str = name
                    )
                })
                .collect::<Suite>();
            py_quote!(
                {"
                    from enum import Enum

                    class #{name}(Enum):
                        #{desc}
                        #{body}
                "} as Suite,
                name: Identifier = name_ident,
                desc: Option<&str> = self.ty.description(),
                body: Suite = class_body,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::generate_source;
    use indoc::indoc;
    use ploidy_core::{
        ir::{IrGraph, IrSpec, SchemaIrTypeView},
        parse::Document,
    };
    use pretty_assertions::assert_eq;

    use crate::CodegenGraph;

    fn to_source(suite: &Suite) -> String {
        generate_source(suite)
    }

    #[test]
    fn test_enum_string_variants() {
        let doc = Document::from_yaml(indoc! {"
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Status");
        let Some(schema @ SchemaIrTypeView::Enum(_, enum_view)) = &schema else {
            panic!("expected enum `Status`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenEnum::new(name, enum_view).to_suite();

        let source = to_source(&suite);
        assert_eq!(
            source,
            indoc! {"
                from enum import Enum
                class Status(Enum):
                    ACTIVE = 'active'
                    INACTIVE = 'inactive'
                    PENDING = 'pending'"
            },
        );
    }

    #[test]
    fn test_enum_with_description() {
        let doc = Document::from_yaml(indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Status:
                  description: The status of an entity.
                  type: string
                  enum:
                    - active
                    - inactive
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Status");
        let Some(schema @ SchemaIrTypeView::Enum(_, enum_view)) = &schema else {
            panic!("expected enum `Status`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenEnum::new(name, enum_view).to_suite();

        let source = to_source(&suite);
        assert_eq!(
            source,
            indoc! {"
                from enum import Enum
                class Status(Enum):
                    'The status of an entity.'
                    ACTIVE = 'active'
                    INACTIVE = 'inactive'"
            },
        );
    }

    #[test]
    fn test_enum_unrepresentable_becomes_type_alias() {
        let doc = Document::from_yaml(indoc! {"
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Priority");
        let Some(schema @ SchemaIrTypeView::Enum(_, view)) = &schema else {
            panic!("expected enum `Priority`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenEnum::new(name, view).to_suite();

        let source = to_source(&suite);
        assert_eq!(source, "Priority = int");
    }

    #[test]
    fn test_enum_mixed_types_becomes_union() {
        let doc = Document::from_yaml(indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Mixed:
                  enum:
                    - text
                    - 42
                    - true
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Mixed");
        let Some(schema @ SchemaIrTypeView::Enum(_, view)) = &schema else {
            panic!("expected enum `Mixed`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenEnum::new(name, view).to_suite();

        let source = to_source(&suite);
        assert_eq!(source, "Mixed = bool | int | str");
    }

    #[test]
    fn test_enum_kebab_case_variants() {
        let doc = Document::from_yaml(indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                ContentType:
                  type: string
                  enum:
                    - application-json
                    - text-plain
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "ContentType");
        let Some(schema @ SchemaIrTypeView::Enum(_, enum_view)) = &schema else {
            panic!("expected enum `ContentType`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenEnum::new(name, enum_view).to_suite();

        let source = to_source(&suite);
        assert_eq!(
            source,
            indoc! {"
                from enum import Enum
                class ContentType(Enum):
                    APPLICATION_JSON = 'application-json'
                    TEXT_PLAIN = 'text-plain'"
            },
        );
    }
}
