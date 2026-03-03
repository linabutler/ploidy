//! Python union type alias generation from IR untagged unions.
//!
//! Untagged unions in OpenAPI (`oneOf` without `discriminator`) are generated
//! as simple union type aliases: `StringOrInt = str | int`

use ploidy_core::ir::{IrUntaggedView, SomeIrUntaggedVariant};
use quasiquodo_py::{
    py_quote,
    ruff::{
        python_ast::{Expr, Identifier, Suite},
        text_size::TextRange,
    },
};

use crate::{imports::ImportContext, naming::CodegenTypeName, ref_::CodegenRef};

/// Generates a Python union type alias from an IR untagged union.
#[derive(Clone, Debug)]
pub struct CodegenUntagged<'a> {
    name: CodegenTypeName<'a>,
    ty: &'a IrUntaggedView<'a>,
}

impl<'a> CodegenUntagged<'a> {
    pub fn new(name: CodegenTypeName<'a>, ty: &'a IrUntaggedView<'a>) -> Self {
        Self { name, ty }
    }

    /// Generates all statements for this untagged union: dependency
    /// imports + type alias.
    pub fn to_suite(&self, context: ImportContext<'_>) -> Suite {
        let mut suite = Suite::new();

        // Dependency imports (cross-SCC refs, datetime, uuid, etc.).
        match &self.name {
            CodegenTypeName::Schema(sv) => {
                suite.extend(crate::imports::collect_imports(*sv, context));
            }
            CodegenTypeName::Inline(iv) => {
                suite.extend(crate::imports::collect_imports(*iv, context));
            }
        }

        // Type definition.
        let union_name = self.name.as_class_name();

        if let Some(desc) = self.ty.description() {
            suite.push(py_quote!("#{desc}" as Stmt, desc: &str = desc));
        }

        // Collect variant types.
        let variant_exprs: Vec<Expr> = self
            .ty
            .variants()
            .map(|variant| match variant.ty() {
                Some(SomeIrUntaggedVariant { view, hint: _ }) => CodegenRef::new(&view).to_expr(),
                None => py_quote!("None" as Expr),
            })
            .collect();

        // Create the PEP 695 type alias: `type Name = T1 | T2 | T3`.
        if let Some(union_expr) = variant_exprs
            .into_iter()
            .reduce(|left, right| py_quote!("#{l} | #{r}" as Expr, l: Expr = left, r: Expr = right))
        {
            suite.push(py_quote!(
                "type #{name} = #{ty}" as Stmt,
                name: Identifier = Identifier::new(&union_name, TextRange::default()),
                ty: Expr = union_expr
            ));
        }

        suite
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;

    use crate::{
        CodegenGraph, generate_source,
        naming::{CodegenIdent, CodegenIdentUsage},
    };
    use indoc::indoc;
    use ploidy_core::{
        ir::{ExtendableView, IrGraph, IrSpec, SccId, SchemaIrTypeView, ViewNode},
        parse::Document,
    };
    use pretty_assertions::assert_eq;

    fn to_source(suite: &Suite) -> String {
        generate_source(suite)
    }

    #[test]
    fn test_untagged_union_primitives() {
        let doc = Document::from_yaml(indoc! {"
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "StringOrInt");
        let Some(schema @ SchemaIrTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `StringOrInt`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenUntagged::new(name, untagged_view)
            .to_suite(ImportContext::new(schema.scc_id(), &BTreeMap::new()));

        let source = to_source(&suite);
        assert_eq!(source, "type StringOrInt = str | int");
    }

    #[test]
    fn test_untagged_union_with_description() {
        let doc = Document::from_yaml(indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                StringOrInt:
                  description: A value that can be either a string or an integer.
                  oneOf:
                    - type: string
                    - type: integer
                      format: int32
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "StringOrInt");
        let Some(schema @ SchemaIrTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `StringOrInt`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenUntagged::new(name, untagged_view)
            .to_suite(ImportContext::new(schema.scc_id(), &BTreeMap::new()));

        let source = to_source(&suite);
        assert_eq!(
            source,
            indoc! {"
                'A value that can be either a string or an integer.'
                type StringOrInt = str | int"
            },
        );
    }

    #[test]
    fn test_untagged_union_with_refs() {
        let doc = Document::from_yaml(indoc! {"
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let scc_module_names: BTreeMap<SccId, String> = graph
            .schemas()
            .map(|s| {
                let ident = s.extensions().get::<CodegenIdent>().unwrap();
                (
                    s.scc_id(),
                    CodegenIdentUsage::Module(&ident).display().to_string(),
                )
            })
            .collect();

        let schema = graph.schemas().find(|s| s.name() == "Animal");
        let Some(schema @ SchemaIrTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `Animal`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenUntagged::new(name, untagged_view)
            .to_suite(ImportContext::new(schema.scc_id(), &scc_module_names));

        let source = to_source(&suite);
        assert_eq!(
            source,
            indoc! {"
                from .dog import Dog
                from .cat import Cat
                type Animal = Dog | Cat"
            },
        );
    }

    #[test]
    fn test_untagged_union_multiple_types() {
        let doc = Document::from_yaml(indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                MultiType:
                  oneOf:
                    - type: string
                    - type: integer
                      format: int32
                    - type: boolean
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "MultiType");
        let Some(schema @ SchemaIrTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `MultiType`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenUntagged::new(name, untagged_view)
            .to_suite(ImportContext::new(schema.scc_id(), &BTreeMap::new()));

        let source = to_source(&suite);
        assert_eq!(source, "type MultiType = str | int | bool");
    }
}
