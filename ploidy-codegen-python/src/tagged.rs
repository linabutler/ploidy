//! Pydantic discriminated union generation from IR tagged unions.
//!
//! Tagged unions in OpenAPI (`oneOf` with `discriminator`) are generated as
//! Pydantic discriminated unions:
//!
//! - For schema reference variants, the discriminator field is added to the
//!   schema itself (in `model.rs`), so we just reference it
//! - For inline variants, a new `BaseModel` class is generated with the
//!   discriminator field
//! - A PEP 695 type alias: `type Pet = Annotated[Dog | Cat, Field(discriminator="tag")]`
//!
//! Using PEP 695 `type` statements (Python 3.12+) ensures lazy evaluation of
//! the right-hand side, which is essential for recursive types like JSON Schema
//! where a tagged union's variants may reference the union itself.
//!
//! This approach avoids duplicating fields from variant schemas and follows
//! Pydantic's recommended discriminated union pattern.

use ploidy_core::{
    codegen::UniqueNames,
    ir::{ExtendableView, IrTaggedView, IrTypeView},
};
use quasiquodo_py::{
    py_quote,
    ruff::{
        python_ast::{Expr, Identifier, Suite},
        text_size::TextRange,
    },
};

use crate::{
    imports::ImportContext,
    model::generate_field_stmts,
    naming::{CodegenIdent, CodegenIdentScope, CodegenIdentUsage, CodegenTypeName},
    ref_::CodegenRef,
};

/// Generates a Pydantic discriminated union from an IR tagged union.
#[derive(Clone, Debug)]
pub struct CodegenTagged<'a> {
    name: CodegenTypeName<'a>,
    ty: &'a IrTaggedView<'a>,
}

impl<'a> CodegenTagged<'a> {
    pub fn new(name: CodegenTypeName<'a>, ty: &'a IrTaggedView<'a>) -> Self {
        Self { name, ty }
    }

    /// Generates all statements for this tagged union: imports + inline
    /// variant classes + type alias.
    pub fn to_suite(&self, context: ImportContext<'_>) -> Suite {
        let mut import_stmts = Suite::new();

        // Dependency imports (cross-SCC refs, datetime, uuid, etc.).
        match &self.name {
            CodegenTypeName::Schema(sv) => {
                import_stmts.extend(crate::imports::collect_imports(*sv, context));
            }
            CodegenTypeName::Inline(iv) => {
                import_stmts.extend(crate::imports::collect_imports(*iv, context));
            }
        }

        // Structural imports.
        import_stmts.push(py_quote!("from typing import Annotated" as Stmt));
        import_stmts.push(py_quote!("from pydantic import Field" as Stmt));

        let has_inline_variant = self
            .ty
            .variants()
            .any(|v| matches!(v.ty(), IrTypeView::Inline(_)));
        if has_inline_variant {
            import_stmts.push(py_quote!("from pydantic import BaseModel" as Stmt));
            import_stmts.push(py_quote!("from typing import Literal" as Stmt));
        }

        // Type definitions.
        let discriminator_field = self.ty.tag();
        let union_name = self.name.as_class_name();

        let unique = UniqueNames::new();
        let mut scope = CodegenIdentScope::new(&unique);

        let mut variant_classes = Vec::new();
        let mut variant_type_exprs: Vec<Expr> = Vec::new();

        for variant in self.ty.variants() {
            let view = variant.ty();
            match &view {
                // For schema references, use direct name references. PEP 695
                // type statements evaluate lazily, so recursive types work.
                ploidy_core::ir::IrTypeView::Schema(schema_view) => {
                    let ident = schema_view.extensions().get::<CodegenIdent>().unwrap();
                    let class_name = CodegenIdentUsage::Class(&ident).display().to_string();

                    variant_type_exprs.push(py_quote!(
                        "#{name}" as Expr,
                        name: Identifier = Identifier::new(&class_name, TextRange::default())
                    ));
                }

                // For inline struct types, generate a class with the struct's
                // fields plus the discriminator.
                ploidy_core::ir::IrTypeView::Inline(ploidy_core::ir::InlineIrTypeView::Struct(
                    _,
                    struct_view,
                )) => {
                    let discriminator_value =
                        variant.aliases().first().copied().unwrap_or(variant.name());

                    let variant_ident = scope.uniquify(variant.name());
                    let variant_class_name = CodegenIdentUsage::Class(&variant_ident)
                        .display()
                        .to_string();

                    let discriminator_field_name =
                        CodegenIdentUsage::Field(&CodegenIdent::new(discriminator_field))
                            .display()
                            .to_string();
                    let discriminator_stmt = py_quote!(
                        r#"#{name}: Literal[#{value}] = #{value}"# as Stmt,
                        name: Identifier = Identifier::new(&discriminator_field_name, TextRange::default()),
                        value: &str = discriminator_value
                    );

                    let mut class_body = vec![discriminator_stmt];
                    class_body.extend(generate_field_stmts(struct_view));

                    let class_name_ident =
                        Identifier::new(&variant_class_name, TextRange::default());
                    variant_classes.push(py_quote!(
                        "class #{name}(BaseModel):
                             #{body}
                        " as Stmt,
                        name: Identifier = class_name_ident,
                        body: Suite = class_body
                    ));

                    variant_type_exprs.push(py_quote!(
                        "#{name}" as Expr,
                        name: Identifier = Identifier::new(&variant_class_name, TextRange::default())
                    ));
                }

                // For other inline types (tagged, untagged, enum), generate a
                // wrapper class with the discriminator and a `value` field.
                _ => {
                    let discriminator_value =
                        variant.aliases().first().copied().unwrap_or(variant.name());

                    let variant_ident = scope.uniquify(variant.name());
                    let variant_class_name = CodegenIdentUsage::Class(&variant_ident)
                        .display()
                        .to_string();

                    let discriminator_field_name =
                        CodegenIdentUsage::Field(&CodegenIdent::new(discriminator_field))
                            .display()
                            .to_string();
                    let discriminator_stmt = py_quote!(
                        r#"#{name}: Literal[#{value}] = #{value}"# as Stmt,
                        name: Identifier = Identifier::new(&discriminator_field_name, TextRange::default()),
                        value: &str = discriminator_value
                    );

                    let ty_ref = CodegenRef::new(&view).to_expr();
                    let value_stmt = py_quote!(
                        "#{name}: #{ty}" as Stmt,
                        name: Identifier = Identifier::new("value", TextRange::default()),
                        ty: Expr = ty_ref
                    );
                    let class_body = vec![discriminator_stmt, value_stmt];

                    let class_name_ident =
                        Identifier::new(&variant_class_name, TextRange::default());
                    variant_classes.push(py_quote!(
                        "class #{name}(BaseModel):
                             #{body}
                        " as Stmt,
                        name: Identifier = class_name_ident,
                        body: Suite = class_body
                    ));

                    variant_type_exprs.push(py_quote!(
                        "#{name}" as Expr,
                        name: Identifier = Identifier::new(&variant_class_name, TextRange::default())
                    ));
                }
            }
        }

        import_stmts.extend(variant_classes);

        // Generate the PEP 695 type alias:
        // `type Pet = Annotated[Dog | Cat, Field(discriminator="pet_type")]`
        if !variant_type_exprs.is_empty() {
            let union_expr = variant_type_exprs
                .into_iter()
                .reduce(|l, r| py_quote!("#{l} | #{r}" as Expr, l: Expr = l, r: Expr = r))
                .expect("variant_type_exprs is non-empty");

            let discriminator_python_name =
                CodegenIdentUsage::Field(&CodegenIdent::new(discriminator_field))
                    .display()
                    .to_string();

            let annotated = py_quote!(
                r#"Annotated[#{union}, Field(discriminator=#{disc})]"# as Expr,
                union: Expr = union_expr,
                disc: &str = &discriminator_python_name
            );

            if let Some(desc) = self.ty.description() {
                import_stmts.push(py_quote!("#{desc}" as Stmt, desc: &str = desc));
            }

            import_stmts.push(py_quote!(
                "type #{name} = #{ty}" as Stmt,
                name: Identifier = Identifier::new(&union_name, TextRange::default()),
                ty: Expr = annotated
            ));
        }

        import_stmts
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
    fn test_tagged_union_basic() {
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
                Pet:
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                    - $ref: '#/components/schemas/Cat'
                  discriminator:
                    propertyName: petType
                    mapping:
                      dog: '#/components/schemas/Dog'
                      cat: '#/components/schemas/Cat'
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

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Tagged(_, tagged)) = &schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenTagged::new(name, tagged)
            .to_suite(ImportContext::new(schema.scc_id(), &scc_module_names));

        let source = to_source(&suite);
        assert_eq!(
            source,
            indoc! {"
                from .dog import Dog
                from .cat import Cat
                from typing import Annotated
                from pydantic import Field
                type Pet = Annotated[Dog | Cat, Field(discriminator='pet_type')]"
            },
        );
    }

    #[test]
    fn test_tagged_union_with_description() {
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
                Pet:
                  description: A pet can be either a dog or a cat.
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                    - $ref: '#/components/schemas/Cat'
                  discriminator:
                    propertyName: type
                    mapping:
                      dog: '#/components/schemas/Dog'
                      cat: '#/components/schemas/Cat'
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

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Tagged(_, tagged)) = &schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenTagged::new(name, tagged)
            .to_suite(ImportContext::new(schema.scc_id(), &scc_module_names));

        let source = to_source(&suite);
        assert_eq!(
            source,
            indoc! {"
                from .dog import Dog
                from .cat import Cat
                from typing import Annotated
                from pydantic import Field
                'A pet can be either a dog or a cat.'
                type Pet = Annotated[Dog | Cat, Field(discriminator='type_')]"
            },
        );
    }

    #[test]
    fn test_tagged_union_custom_discriminator_mapping() {
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
                Pet:
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                    - $ref: '#/components/schemas/Cat'
                  discriminator:
                    propertyName: type
                    mapping:
                      canine: '#/components/schemas/Dog'
                      feline: '#/components/schemas/Cat'
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

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Tagged(_, tagged)) = &schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenTagged::new(name, tagged)
            .to_suite(ImportContext::new(schema.scc_id(), &scc_module_names));

        let source = to_source(&suite);
        assert_eq!(
            source,
            indoc! {"
                from .dog import Dog
                from .cat import Cat
                from typing import Annotated
                from pydantic import Field
                type Pet = Annotated[Dog | Cat, Field(discriminator='type_')]"
            },
        );
    }
}
