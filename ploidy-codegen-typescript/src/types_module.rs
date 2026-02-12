use itertools::Itertools;
use ploidy_core::codegen::Code;

use super::{
    emit::{TsComments, emit_module, reexport_type},
    graph::CodegenGraph,
    naming::{CodegenTypeName, CodegenTypeNameSortKey},
    schema::TsCode,
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

impl Code for CodegenTypesModule<'_> {
    fn path(&self) -> &str {
        "types/index.ts"
    }

    fn into_string(self) -> miette::Result<String> {
        let mut tys = self.graph.schemas().collect_vec();
        tys.sort_by(|a, b| {
            CodegenTypeNameSortKey::for_schema(a).cmp(&CodegenTypeNameSortKey::for_schema(b))
        });

        let comments = TsComments::new();
        let items = tys
            .iter()
            .map(|ty| {
                let name = CodegenTypeName::Schema(ty);
                let type_name = name.type_name();
                let file_name = name.display_file_name();
                reexport_type(&[type_name], &format!("./{file_name}"))
            })
            .collect();

        Ok(emit_module(items, &comments))
    }
}

/// Converts a [`CodegenTypesModule`] into a [`TsCode`].
impl<'a> From<CodegenTypesModule<'a>> for TsCode {
    fn from(module: CodegenTypesModule<'a>) -> Self {
        let path = module.path().to_owned();
        let content = module.into_string().unwrap();
        TsCode::new(path, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{ir::Ir, parse::Document};
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

        let module = CodegenTypesModule::new(&graph);
        let content = module.into_string().unwrap();
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
