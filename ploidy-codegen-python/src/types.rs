//! Generation of the `models/__init__.py` module.

use std::collections::BTreeMap;

use itertools::Itertools;
use ploidy_core::{
    codegen::{Code, IntoCode},
    ir::{ExtendableView, SccId, SchemaIrTypeView, ViewNode},
};
use quasiquodo_py::{
    py_quote,
    ruff::{
        python_ast::{Alias, Expr, Identifier, Suite},
        text_size::TextRange,
    },
};

use crate::{
    graph::CodegenGraph,
    naming::{CodegenIdent, CodegenIdentUsage},
};

/// Generates the `models/__init__.py` module that exports all types.
pub struct CodegenTypesModule<'a> {
    graph: &'a CodegenGraph<'a>,
}

impl<'a> CodegenTypesModule<'a> {
    pub fn new(graph: &'a CodegenGraph<'a>) -> Self {
        Self { graph }
    }

    fn to_suite(&self) -> Suite {
        let mut suite = Suite::new();

        // Group schemas by SCC to determine which module each schema
        // lives in. Multi-schema SCCs share a module named after the
        // alphabetically first schema.
        let mut sccs: BTreeMap<SccId, Vec<SchemaIrTypeView<'_>>> = BTreeMap::new();
        for view in self.graph.schemas() {
            sccs.entry(view.scc_id()).or_default().push(view);
        }

        // Compute the module name for each SCC (alphabetically first
        // schema's module name).
        let scc_module_names: BTreeMap<SccId, String> = sccs
            .iter()
            .map(|(&scc_id, schemas)| {
                let first = schemas.iter().min_by_key(|s| s.name()).unwrap();
                let ident = first.extensions().get::<CodegenIdent>().unwrap();
                let module_name = CodegenIdentUsage::Module(&ident).display().to_string();
                (scc_id, module_name)
            })
            .collect();

        // Collect all schemas with their idents and SCC module names,
        // sorted alphabetically by ident.
        let mut types_with_info: Vec<_> = self
            .graph
            .schemas()
            .filter_map(|view| {
                let ident = view.extensions().get::<CodegenIdent>()?.clone();
                let module_name = scc_module_names[&view.scc_id()].clone();
                Some((ident, module_name))
            })
            .collect();
        // Sort by module name first (so `chunk_by` groups all schemas in
        // the same SCC module together), then by class name within each
        // module.
        types_with_info.sort_by(|(a, ma), (b, mb)| ma.cmp(mb).then_with(|| a.cmp(b)));

        // Generate grouped import statements. Schemas in the same SCC
        // module produce a single `from .module import A, B` statement.
        for (_module_name, group) in &types_with_info
            .iter()
            .chunk_by(|(_, module_name)| module_name.clone())
        {
            let group = group.collect_vec();
            let module_name = &group[0].1;
            let names: Vec<Alias> = group
                .iter()
                .map(|(ident, _)| {
                    let type_name = CodegenIdentUsage::Class(ident).display().to_string();
                    py_quote!(
                        "#{n}" as Alias,
                        n: Identifier = Identifier::new(
                            &type_name,
                            TextRange::default()
                        )
                    )
                })
                .collect();
            suite.push(py_quote!(
                "from .#{module} import #{names}" as Stmt,
                module: Identifier = Identifier::new(
                    module_name,
                    TextRange::default()
                ),
                names: Vec<Alias> = names
            ));
        }

        // Generate `__all__` list.
        if !types_with_info.is_empty() {
            let all_items: Vec<Expr> = types_with_info
                .iter()
                .map(|(ident, _)| {
                    let type_name = CodegenIdentUsage::Class(ident).display().to_string();
                    py_quote!("#{name}" as Expr, name: &str = &type_name)
                })
                .collect();
            suite.push(py_quote!(
                "__all__ = [#{items}]" as Stmt,
                items: Vec<Expr> = all_items
            ));
        }

        suite
    }
}

impl IntoCode for CodegenTypesModule<'_> {
    type Code = PythonInitCode;

    fn into_code(self) -> Self::Code {
        PythonInitCode {
            suite: self.to_suite(),
        }
    }
}

/// Represents the generated `__init__.py` code.
#[derive(Debug)]
pub struct PythonInitCode {
    suite: Suite,
}

impl Code for PythonInitCode {
    fn path(&self) -> &str {
        "models/__init__.py"
    }

    fn into_string(self) -> miette::Result<String> {
        Ok(crate::generate_source(&self.suite))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;
    use ploidy_core::{
        ir::{IrGraph, IrSpec},
        parse::Document,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn test_types_module_exports_all_schemas() {
        let doc = Document::from_yaml(indoc! {"
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
                User:
                  type: object
                  properties:
                    id:
                      type: string
                Status:
                  type: string
                  enum:
                    - active
                    - inactive
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let code = CodegenTypesModule::new(&graph).into_code();

        assert_eq!(code.path(), "models/__init__.py");

        let source = code.into_string().unwrap();

        assert_eq!(
            source,
            indoc! {"
                from .pet import Pet
                from .status import Status
                from .user import User
                __all__ = ['Pet', 'Status', 'User']"
            },
        );
    }

    #[test]
    fn test_types_module_empty() {
        let doc = Document::from_yaml(indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let code = CodegenTypesModule::new(&graph).into_code();
        let source = code.into_string().unwrap();

        assert!(source.is_empty());
    }

    #[test]
    fn test_types_module_groups_scc_imports() {
        let doc = Document::from_yaml(indoc! {"
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
                    owner:
                      $ref: '#/components/schemas/Owner'
                Owner:
                  type: object
                  properties:
                    name:
                      type: string
                    pets:
                      type: array
                      items:
                        $ref: '#/components/schemas/Pet'
                Status:
                  type: string
                  enum:
                    - active
                    - inactive
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let code = CodegenTypesModule::new(&graph).into_code();
        let source = code.into_string().unwrap();

        assert_eq!(
            source,
            indoc! {"
                from .owner import Owner, Pet
                from .status import Status
                __all__ = ['Owner', 'Pet', 'Status']"
            },
        );
    }

    #[test]
    fn test_types_module_groups_scc_imports_interleaved() {
        // Alpha and Charlie form a cycle (same SCC module = "alpha").
        // Beta is independent and sorts between them alphabetically.
        // The grouped import for Alpha/Charlie must not be split by Beta.
        let doc = Document::from_yaml(indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Alpha:
                  type: object
                  properties:
                    c:
                      $ref: '#/components/schemas/Charlie'
                Beta:
                  type: object
                  properties:
                    name:
                      type: string
                Charlie:
                  type: object
                  properties:
                    a:
                      $ref: '#/components/schemas/Alpha'
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let code = CodegenTypesModule::new(&graph).into_code();
        let source = code.into_string().unwrap();

        assert_eq!(
            source,
            indoc! {"
                from .alpha import Alpha, Charlie
                from .beta import Beta
                __all__ = ['Alpha', 'Charlie', 'Beta']"
            },
        );
    }
}
