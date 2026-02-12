use itertools::Itertools;
use quasiquodo_ts::{Comments, swc::ecma_ast::Module, ts_quote};

use super::{
    TsSource,
    graph::CodegenGraph,
    naming::{CodegenTypeName, CodegenTypeNameSortKey},
};

/// Generates the barrel `types/index.ts` that re-exports all schema types.
pub struct CodegenTypesModule<'a> {
    graph: &'a CodegenGraph<'a>,
}

impl<'a> CodegenTypesModule<'a> {
    pub fn new(graph: &'a CodegenGraph<'a>) -> Self {
        Self { graph }
    }
}

impl CodegenTypesModule<'_> {
    /// Generates the barrel module and returns it as a [`TsSource`].
    pub fn into_code(self) -> TsSource<Module> {
        let mut tys = self.graph.schemas().collect_vec();
        tys.sort_by(|a, b| {
            CodegenTypeNameSortKey::for_schema(a).cmp(&CodegenTypeNameSortKey::for_schema(b))
        });

        let comments = Comments::new();
        let mut module = Module::default();
        for ty in &tys {
            let name = CodegenTypeName::Schema(ty);
            let type_name = name.display().to_string();
            let module_name = name.into_module_name();
            let file_name = module_name.display();
            let spec = format!("./{file_name}");
            module.body.push(ts_quote!(
                r#"export type { #{n} } from #{spec}"# as ModuleItem,
                n: Ident = type_name,
                spec: &str = &spec,
            ));
        }

        TsSource::new("types/index.ts".to_owned(), comments, module)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{codegen::Code, ir::Ir, parse::Document};
    use pretty_assertions::assert_eq;

    use crate::CodegenGraph;

    #[test]
    fn test_types_module_barrel_export() {
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
                Status:
                  type: string
                  enum:
                    - active
                    - inactive
                Order:
                  type: object
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let code = CodegenTypesModule::new(&graph).into_code();
        let content = code.into_string().unwrap();
        assert_eq!(
            content,
            indoc::indoc! {r#"
                export type { Order } from "./order";
                export type { Pet } from "./pet";
                export type { Status } from "./status";
            "#}
        );
    }
}
