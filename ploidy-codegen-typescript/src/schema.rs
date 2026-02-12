use std::collections::BTreeSet;

use ploidy_core::{
    codegen::Code,
    ir::{ContainerView, ExtendableView, InlineIrTypePathRoot, SchemaIrTypeView, View},
};
use swc_common::DUMMY_SP;
use swc_ecma_ast::{ModuleItem, TsKeywordTypeKind};

use super::{
    emit::{
        TsComments, array, emit_module, export_decl, import_type_decl, kw, nullable, record,
        type_alias_decl,
    },
    enum_::ts_enum,
    inlines::ts_inlines,
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
        let name = CodegenTypeName::Schema(self.ty);
        let type_name = name.type_name();
        let comments = TsComments::new();

        // Build the main type declaration.
        let (main_decl, description) = match self.ty {
            SchemaIrTypeView::Struct(_, view) => {
                let desc = view.description().map(|s| s.to_owned());
                (ts_struct(&type_name, view, &comments), desc)
            }
            SchemaIrTypeView::Enum(_, view) => {
                let desc = view.description().map(|s| s.to_owned());
                (ts_enum(&type_name, view), desc)
            }
            SchemaIrTypeView::Tagged(_, view) => {
                let desc = view.description().map(|s| s.to_owned());
                (ts_tagged(&type_name, view), desc)
            }
            SchemaIrTypeView::Untagged(_, view) => {
                let desc = view.description().map(|s| s.to_owned());
                (ts_untagged(&type_name, view), desc)
            }
            SchemaIrTypeView::Container(_, ContainerView::Array(inner)) => {
                let inner_ty = inner.ty();
                let desc = inner.description().map(|s| s.to_owned());
                (
                    type_alias_decl(&type_name, array(ts_type_ref(&inner_ty))),
                    desc,
                )
            }
            SchemaIrTypeView::Container(_, ContainerView::Map(inner)) => {
                let inner_ty = inner.ty();
                let desc = inner.description().map(|s| s.to_owned());
                (
                    type_alias_decl(&type_name, record(ts_type_ref(&inner_ty))),
                    desc,
                )
            }
            SchemaIrTypeView::Container(_, ContainerView::Optional(inner)) => {
                let inner_ty = inner.ty();
                let desc = inner.description().map(|s| s.to_owned());
                (
                    type_alias_decl(&type_name, nullable(ts_type_ref(&inner_ty))),
                    desc,
                )
            }
            SchemaIrTypeView::Primitive(_, view) => {
                (type_alias_decl(&type_name, ts_primitive(view.ty())), None)
            }
            SchemaIrTypeView::Any(_, _) => (
                type_alias_decl(&type_name, kw(TsKeywordTypeKind::TsUnknownKeyword)),
                None,
            ),
        };

        // Build module items: imports, main decl (with JSDoc), then
        // inline namespace.
        let mut items: Vec<ModuleItem> = Vec::new();

        // Collect imports.
        for import in collect_imports(self.ty) {
            items.push(import);
        }

        // Main declaration with optional JSDoc.
        let span = comments.span_with_jsdoc(description.as_deref());
        items.push(export_decl(main_decl, span));

        // Add inline types as a namespace.
        if let Some(ns) = ts_inlines(self.ty, &comments) {
            items.push(export_decl(ns, DUMMY_SP));
        }

        let file_name = name.display_file_name();

        TsCode {
            path: format!("types/{file_name}.ts"),
            content: emit_module(items, &comments),
        }
    }
}

/// Collects `import type` declarations needed for a schema's file.
fn collect_imports(schema: &SchemaIrTypeView<'_>) -> Vec<ModuleItem> {
    let current_name = schema.name();
    let mut imported_schemas: BTreeSet<String> = BTreeSet::new();

    // Walk all type dependencies to find referenced schemas.
    for dep in schema.dependencies() {
        match &dep {
            ploidy_core::ir::IrTypeView::Schema(view) => {
                if view.name() != current_name {
                    let ext = view.extensions();
                    let ident = ext.get::<CodegenIdent>().unwrap();
                    imported_schemas.insert(ident.to_type_name());
                }
            }
            ploidy_core::ir::IrTypeView::Inline(view) => {
                let path = view.path();
                if let InlineIrTypePathRoot::Type(name) = path.root
                    && name != current_name
                {
                    imported_schemas.insert(CodegenIdent::new(name).to_type_name());
                }
            }
        }
    }

    imported_schemas
        .into_iter()
        .map(|name| {
            let file_name = heck::AsSnekCase(&name).to_string();
            import_type_decl(&[name], &format!("./{file_name}"))
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
                  nested?: Container.Nested;
                }
                export namespace Container {
                  export interface Nested {
                    value?: string;
                  }
                }
            "}
        );
    }
}
