use std::collections::BTreeSet;

use oxc_allocator::Allocator;
use oxc_ast::AstBuilder;
use oxc_ast::ast::Statement;
use oxc_span::SPAN;
use ploidy_core::{
    codegen::Code,
    ir::{ContainerView, ExtendableView, SchemaIrTypeView, View},
};

use super::{
    emit::{
        TsComments, array, emit_module, export_decl, import_type_decl, nullable, record,
        type_alias_decl,
    },
    enum_::ts_enum,
    naming::{CodegenIdent, CodegenTypeName},
    primitive::ts_primitive,
    ref_::ts_type_ref,
    struct_::ts_struct,
    tagged::ts_tagged,
    untagged::ts_untagged,
};

/// A generated TypeScript file for a single schema type.
pub struct TsCode {
    path: String,
    content: String,
}

impl TsCode {
    pub(crate) fn new(path: String, content: String) -> Self {
        Self { path, content }
    }
}

impl Code for TsCode {
    fn path(&self) -> &str {
        &self.path
    }

    fn into_string(self) -> miette::Result<String> {
        Ok(self.content)
    }
}

/// Generates a TypeScript module for a named schema type.
pub struct CodegenSchemaType<'a> {
    ty: &'a SchemaIrTypeView<'a>,
}

impl<'a> CodegenSchemaType<'a> {
    pub fn new(ty: &'a SchemaIrTypeView<'a>) -> Self {
        Self { ty }
    }

    /// Generates the TypeScript module and returns it as a [`TsCode`].
    pub fn into_code(self) -> TsCode {
        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);

        let name = CodegenTypeName::Schema(self.ty);
        let type_name = name.type_name();
        let comments = TsComments::new();

        // Build the main type declaration.
        let (main_decl, description) = match self.ty {
            SchemaIrTypeView::Struct(_, view) => {
                let desc = view.description().map(|s| s.to_owned());
                (ts_struct(&ast, &type_name, view, &comments), desc)
            }
            SchemaIrTypeView::Enum(_, view) => {
                let desc = view.description().map(|s| s.to_owned());
                (ts_enum(&ast, &type_name, view), desc)
            }
            SchemaIrTypeView::Tagged(_, view) => {
                let desc = view.description().map(|s| s.to_owned());
                (ts_tagged(&ast, &type_name, view, &comments), desc)
            }
            SchemaIrTypeView::Untagged(_, view) => {
                let desc = view.description().map(|s| s.to_owned());
                (ts_untagged(&ast, &type_name, view, &comments), desc)
            }
            SchemaIrTypeView::Container(_, ContainerView::Array(inner)) => {
                let inner_ty = inner.ty();
                let desc = inner.description().map(|s| s.to_owned());
                (
                    type_alias_decl(
                        &ast,
                        &type_name,
                        array(&ast, ts_type_ref(&ast, &inner_ty, &comments)),
                    ),
                    desc,
                )
            }
            SchemaIrTypeView::Container(_, ContainerView::Map(inner)) => {
                let inner_ty = inner.ty();
                let desc = inner.description().map(|s| s.to_owned());
                (
                    type_alias_decl(
                        &ast,
                        &type_name,
                        record(&ast, ts_type_ref(&ast, &inner_ty, &comments)),
                    ),
                    desc,
                )
            }
            SchemaIrTypeView::Container(_, ContainerView::Optional(inner)) => {
                let inner_ty = inner.ty();
                let desc = inner.description().map(|s| s.to_owned());
                (
                    type_alias_decl(
                        &ast,
                        &type_name,
                        nullable(&ast, ts_type_ref(&ast, &inner_ty, &comments)),
                    ),
                    desc,
                )
            }
            SchemaIrTypeView::Primitive(_, view) => (
                type_alias_decl(&ast, &type_name, ts_primitive(&ast, view.ty())),
                None,
            ),
            SchemaIrTypeView::Any(_, _) => (
                type_alias_decl(&ast, &type_name, ast.ts_type_unknown_keyword(SPAN)),
                None,
            ),
        };

        // Build module items: imports then main decl (with JSDoc).
        let mut items: Vec<Statement<'_>> = Vec::new();

        // Collect imports.
        for import in collect_imports(&ast, self.ty) {
            items.push(import);
        }

        // Main declaration with optional JSDoc.
        let span = comments.span_with_jsdoc(description.as_deref());
        items.push(export_decl(&ast, main_decl, span));

        let file_name = name.display_file_name();
        let body = ast.vec_from_iter(items);

        TsCode {
            path: format!("types/{file_name}.ts"),
            content: emit_module(&allocator, &ast, body, &comments),
        }
    }
}

/// Collects `import type` declarations needed for a schema's file.
fn collect_imports<'a>(ast: &AstBuilder<'a>, schema: &SchemaIrTypeView<'_>) -> Vec<Statement<'a>> {
    let current_name = schema.name();
    let mut imported_schemas: BTreeSet<String> = BTreeSet::new();

    // Walk all type dependencies to find referenced schemas.
    for dep in schema.dependencies() {
        if let ploidy_core::ir::IrTypeView::Schema(view) = &dep
            && view.name() != current_name
        {
            let ext = view.extensions();
            let ident = ext.get::<CodegenIdent>().unwrap();
            imported_schemas.insert(ident.to_type_name());
        }
    }

    imported_schemas
        .into_iter()
        .map(|name| {
            let file_name = heck::AsSnekCase(&name).to_string();
            import_type_decl(ast, &[name], &format!("./{file_name}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{ir::Ir, parse::Document};
    use pretty_assertions::assert_eq;

    use crate::CodegenGraph;

    #[test]
    fn test_schema_struct() {
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
                  required:
                    - name
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph
            .schemas()
            .find(|s| s.name() == "Pet")
            .expect("expected schema `Pet`");
        let code = CodegenSchemaType::new(&schema).into_code();
        assert_eq!(code.path(), "types/pet.ts");
        assert_eq!(
            code.content,
            indoc::indoc! {"
                export interface Pet {
                  name: string;
                }
            "}
        );
    }

    #[test]
    fn test_schema_enum() {
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
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph
            .schemas()
            .find(|s| s.name() == "Status")
            .expect("expected schema `Status`");
        let code = CodegenSchemaType::new(&schema).into_code();
        assert_eq!(code.path(), "types/status.ts");
        assert_eq!(
            code.content,
            "export type Status = \"active\" | \"inactive\";\n"
        );
    }

    #[test]
    fn test_schema_with_imports() {
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

        let schema = graph
            .schemas()
            .find(|s| s.name() == "Animal")
            .expect("expected schema `Animal`");
        let code = CodegenSchemaType::new(&schema).into_code();
        assert_eq!(code.path(), "types/animal.ts");
        assert_eq!(
            code.content,
            indoc::indoc! {r#"
                import type { Cat } from "./cat";
                import type { Dog } from "./dog";
                export type Animal = Dog | Cat;
            "#}
        );
    }

    #[test]
    fn test_schema_container_array() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Tags:
                  type: array
                  items:
                    type: string
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph
            .schemas()
            .find(|s| s.name() == "Tags")
            .expect("expected schema `Tags`");
        let code = CodegenSchemaType::new(&schema).into_code();
        assert_eq!(code.path(), "types/tags.ts");
        assert_eq!(code.content, "export type Tags = string[];\n");
    }

    #[test]
    fn test_schema_container_map() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Metadata:
                  type: object
                  additionalProperties:
                    type: string
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph
            .schemas()
            .find(|s| s.name() == "Metadata")
            .expect("expected schema `Metadata`");
        let code = CodegenSchemaType::new(&schema).into_code();
        assert_eq!(code.path(), "types/metadata.ts");
        assert_eq!(
            code.content,
            "export type Metadata = Record<string, string>;\n"
        );
    }

    #[test]
    fn test_schema_any() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Anything:
                  {}
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph
            .schemas()
            .find(|s| s.name() == "Anything")
            .expect("expected schema `Anything`");
        let code = CodegenSchemaType::new(&schema).into_code();
        assert_eq!(code.path(), "types/anything.ts");
        assert_eq!(code.content, "export type Anything = unknown;\n");
    }

    #[test]
    fn test_schema_with_inline_types() {
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
                    nested:
                      type: object
                      properties:
                        value:
                          type: string
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph
            .schemas()
            .find(|s| s.name() == "Container")
            .expect("expected schema `Container`");
        let code = CodegenSchemaType::new(&schema).into_code();
        assert_eq!(code.path(), "types/container.ts");
        assert_eq!(
            code.content,
            indoc::indoc! {"
                export interface Container {
                  nested?: {
                    value?: string;
                  };
                }
            "}
        );
    }
}
