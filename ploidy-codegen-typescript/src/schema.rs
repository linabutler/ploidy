use std::collections::BTreeMap;

use ploidy_core::ir::{ContainerView, ExtendableView, IrTypeView, SchemaIrTypeView, View};
use quasiquodo_ts::{
    Comments, JsDoc,
    swc::ecma_ast::{Module, ModuleItem, TsType},
    ts_quote,
};

use super::{
    TsSource,
    enum_::ts_enum_type,
    naming::{CodegenIdent, CodegenIdentUsage, CodegenTypeName},
    primitive::ts_primitive,
    ref_::ts_type_ref,
    struct_::ts_struct,
    tagged::ts_tagged_type,
    untagged::ts_untagged_type,
};

/// Generates a TypeScript module for a named schema type.
pub struct CodegenSchemaType<'a> {
    ty: &'a SchemaIrTypeView<'a>,
}

impl<'a> CodegenSchemaType<'a> {
    pub fn new(ty: &'a SchemaIrTypeView<'a>) -> Self {
        Self { ty }
    }

    /// Generates the TypeScript module and returns it as a
    /// [`TsSource`].
    pub fn into_code(self) -> TsSource<Module> {
        let name = CodegenTypeName::Schema(self.ty);
        let type_name = name.display().to_string();
        let comments = Comments::new();

        // Build module items: imports then main decl (with JSDoc).
        let mut module = Module::default();

        // Collect imports.
        for import in collect_imports(self.ty) {
            module.body.push(import);
        }

        // Build the main type item.
        let main_item = match self.ty {
            SchemaIrTypeView::Struct(_, view) => {
                ts_struct(&type_name, view, &comments, view.description())
            }
            SchemaIrTypeView::Enum(_, view) => ts_quote!(
                comments,
                "#{doc} export type #{n} = #{t}" as ModuleItem,
                doc: Option<JsDoc> = view.description().map(JsDoc::new),
                n: Ident = &type_name,
                t: TsType = ts_enum_type(view)
            ),
            SchemaIrTypeView::Tagged(_, view) => ts_quote!(
                comments,
                "#{doc} export type #{n} = #{t}" as ModuleItem,
                doc: Option<JsDoc> = view.description().map(JsDoc::new),
                n: Ident = &type_name,
                t: TsType = ts_tagged_type(view, &comments)
            ),
            SchemaIrTypeView::Untagged(_, view) => ts_quote!(
                comments,
                "#{doc} export type #{n} = #{t}" as ModuleItem,
                doc: Option<JsDoc> = view.description().map(JsDoc::new),
                n: Ident = &type_name,
                t: TsType = ts_untagged_type(view, &comments)
            ),
            SchemaIrTypeView::Container(_, ContainerView::Array(inner)) => {
                let inner_ty = inner.ty();
                let elem = ts_type_ref(&inner_ty, &comments);
                let arr_ty = match elem {
                    TsType::TsUnionOrIntersectionType(ty) => {
                        ts_quote!("(#{ty})[]" as TsType, ty: TsType = ty)
                    }
                    ty => ts_quote!("#{ty}[]" as TsType, ty: TsType = ty),
                };
                ts_quote!(
                    comments,
                    "#{doc} export type #{n} = #{t}" as ModuleItem,
                    doc: Option<JsDoc> = inner.description().map(JsDoc::new),
                    n: Ident = &type_name,
                    t: TsType = arr_ty
                )
            }
            SchemaIrTypeView::Container(_, ContainerView::Map(inner)) => {
                let inner_ty = inner.ty();
                let v = ts_type_ref(&inner_ty, &comments);
                ts_quote!(
                    comments,
                    "#{doc} export type #{n} = #{t}" as ModuleItem,
                    doc: Option<JsDoc> = inner.description().map(JsDoc::new),
                    n: Ident = &type_name,
                    t: TsType = ts_quote!("Record<string, #{v}>" as TsType, v: TsType = v)
                )
            }
            SchemaIrTypeView::Container(_, ContainerView::Optional(inner)) => {
                let inner_ty = inner.ty();
                let t = ts_type_ref(&inner_ty, &comments);
                ts_quote!(
                    comments,
                    "#{doc} export type #{n} = #{t}" as ModuleItem,
                    doc: Option<JsDoc> = inner.description().map(JsDoc::new),
                    n: Ident = &type_name,
                    t: TsType = ts_quote!("#{t} | null" as TsType, t: TsType = t)
                )
            }
            SchemaIrTypeView::Primitive(_, view) => ts_quote!(
                comments,
                "export type #{n} = #{t}" as ModuleItem,
                n: Ident = &type_name,
                t: TsType = ts_primitive(view.ty())
            ),
            SchemaIrTypeView::Any(_, _) => ts_quote!(
                comments,
                "export type #{n} = unknown" as ModuleItem,
                n: Ident = &type_name,
            ),
        };

        module.body.push(main_item);

        let module_name = name.into_module_name();
        let file_name = module_name.display();

        TsSource::new(format!("types/{file_name}.ts"), comments, module)
    }
}

/// Collects `import type` declarations needed for a schema's file.
fn collect_imports(schema: &SchemaIrTypeView<'_>) -> Vec<ModuleItem> {
    let mut imported_schemas: BTreeMap<String, CodegenIdent> = BTreeMap::new();

    // Walk all type dependencies to find referenced schemas.
    // `dependencies()` already excludes self (see `IrGraph::new`).
    for dep in schema.dependencies() {
        if let IrTypeView::Schema(view) = &dep {
            let ext = view.extensions();
            let ident = ext.get::<CodegenIdent>().unwrap().clone();
            let type_name = CodegenIdentUsage::Type(&ident).display().to_string();
            imported_schemas.entry(type_name).or_insert(ident);
        }
    }

    imported_schemas
        .into_iter()
        .map(|(type_name, ident)| {
            let file_name = CodegenIdentUsage::Module(&ident).display().to_string();
            let spec = format!("./{file_name}");
            ts_quote!(
                r#"import type { #{n} } from #{spec}"# as ModuleItem,
                n: Ident = type_name,
                spec: &str = &spec,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{codegen::Code, ir::Ir, parse::Document};
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
            code.into_string().unwrap(),
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
            code.into_string().unwrap(),
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
            code.into_string().unwrap(),
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
        assert_eq!(
            code.into_string().unwrap(),
            "export type Tags = string[];\n"
        );
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
            code.into_string().unwrap(),
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
        assert_eq!(
            code.into_string().unwrap(),
            "export type Anything = unknown;\n"
        );
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
            code.into_string().unwrap(),
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
