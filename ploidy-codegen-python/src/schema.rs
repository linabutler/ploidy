//! Per-SCC Python module file generation.
//!
//! Each strongly connected component (SCC) of schemas is emitted into a
//! single Python module. Cross-SCC imports are guaranteed acyclic, so
//! `TYPE_CHECKING` guards are no longer needed.

use std::collections::BTreeMap;

use itertools::Itertools;
use ploidy_core::{
    codegen::{Code, IntoCode},
    ir::{InlineIrTypeView, SccId, SchemaIrTypeView, View, ViewNode},
};
use quasiquodo_py::{py_quote, ruff::python_ast::Suite};

use crate::{
    enum_::CodegenEnum,
    imports::{ImportContext, isort},
    model::CodegenModel,
    naming::CodegenTypeName,
    tagged::CodegenTagged,
    untagged::CodegenUntagged,
};

/// Generates a complete Python module for all schemas in one SCC.
///
/// Each SCC is emitted as a single `.py` file. Imports are collected by
/// calling `type_imports()` for each schema's dependencies and emitting
/// structural imports (pydantic, typing, enum) inline. The final
/// `consolidate_imports()` pass deduplicates and sorts them.
#[derive(Debug)]
pub struct CodegenSccModule<'a> {
    schemas: &'a [SchemaIrTypeView<'a>],
    scc_module_names: &'a BTreeMap<SccId, String>,
}

impl<'a> CodegenSccModule<'a> {
    /// Creates a new SCC module generator for the given schemas and
    /// SCC-to-module-name mapping.
    pub fn new(
        schemas: &'a [SchemaIrTypeView<'a>],
        scc_module_names: &'a BTreeMap<SccId, String>,
    ) -> Self {
        Self {
            schemas,
            scc_module_names,
        }
    }

    /// Returns the module name for this SCC from the precomputed mapping.
    fn module_name(&self) -> &str {
        let scc_id = self.schemas[0].scc_id();
        &self.scc_module_names[&scc_id]
    }

    /// Generates the module content as a list of statements.
    fn to_suite(&self) -> Suite {
        // `from __future__ import annotations` enables lazy evaluation of
        // type hints, so intra-module forward references just work.
        let mut suite = vec![py_quote!("from __future__ import annotations" as Stmt)];
        suite.extend(self.type_definition_stmts());
        isort(&mut suite);
        suite
    }

    /// Generates type definition statements for all schemas and their
    /// inline types in this SCC.
    ///
    /// Emits inlines first (sorted for stability), then schemas. Since
    /// `from __future__ import annotations` is present, order within a
    /// module doesn't matter for forward references. Each codegen type
    /// emits its own imports; the caller deduplicates via
    /// `consolidate_imports`.
    fn type_definition_stmts(&self) -> Suite {
        let context = ImportContext::new(self.schemas[0].scc_id(), self.scc_module_names);
        let mut suite = Suite::new();

        // Collect all inline types across schemas for codegen.
        let all_inlines: Vec<(usize, Vec<InlineIrTypeView<'_>>)> = self
            .schemas
            .iter()
            .enumerate()
            .map(|(i, ty)| (i, ty.inlines().collect_vec()))
            .collect();

        for (_, inlines) in &all_inlines {
            let mut sorted_inlines: Vec<&InlineIrTypeView<'_>> = inlines.iter().collect();
            sorted_inlines.sort_by_key(|i| CodegenTypeName::Inline(i).into_sort_key());

            for inline in sorted_inlines {
                let name = CodegenTypeName::Inline(inline);
                suite.extend(match inline {
                    InlineIrTypeView::Struct(_, view) => {
                        CodegenModel::new(name, view).to_suite(context)
                    }
                    InlineIrTypeView::Enum(_, view) => CodegenEnum::new(name, view).to_suite(),
                    InlineIrTypeView::Tagged(_, view) => {
                        CodegenTagged::new(name, view).to_suite(context)
                    }
                    InlineIrTypeView::Untagged(_, view) => {
                        CodegenUntagged::new(name, view).to_suite(context)
                    }
                    InlineIrTypeView::Container(..)
                    | InlineIrTypeView::Primitive(..)
                    | InlineIrTypeView::Any(..) => vec![],
                });
            }
        }

        let mut sorted_schemas: Vec<_> = self.schemas.iter().collect();
        sorted_schemas.sort_by_key(|s| CodegenTypeName::Schema(s).into_sort_key());

        for ty in sorted_schemas {
            let name = CodegenTypeName::Schema(ty);
            suite.extend(match ty {
                SchemaIrTypeView::Struct(_, view) => CodegenModel::new(name, view).to_suite(context),
                SchemaIrTypeView::Enum(_, view) => CodegenEnum::new(name, view).to_suite(),
                SchemaIrTypeView::Tagged(_, view) => CodegenTagged::new(name, view).to_suite(context),
                SchemaIrTypeView::Untagged(_, view) => {
                    CodegenUntagged::new(name, view).to_suite(context)
                }
                SchemaIrTypeView::Container(..)
                | SchemaIrTypeView::Primitive(..)
                | SchemaIrTypeView::Any(..) => vec![],
            });
        }

        suite
    }
}

impl IntoCode for CodegenSccModule<'_> {
    type Code = PythonCode;

    fn into_code(self) -> Self::Code {
        let path = format!("models/{}.py", self.module_name());
        let suite = self.to_suite();

        PythonCode { path, suite }
    }
}

/// Represents generated Python code ready to be written to disk.
#[derive(Debug)]
pub struct PythonCode {
    path: String,
    suite: Suite,
}

impl Code for PythonCode {
    fn path(&self) -> &str {
        &self.path
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
        ir::{ExtendableView, IrGraph, IrSpec},
        parse::Document,
    };
    use pretty_assertions::assert_eq;

    use crate::{
        CodegenGraph,
        naming::{CodegenIdent, CodegenIdentUsage},
    };

    /// Builds the SCC module name mapping from a codegen graph.
    fn build_scc_module_names(graph: &CodegenGraph<'_>) -> BTreeMap<SccId, String> {
        let mut sccs: BTreeMap<SccId, Vec<_>> = BTreeMap::new();
        for view in graph.schemas() {
            sccs.entry(view.scc_id()).or_default().push(view);
        }
        sccs.iter()
            .map(|(&scc_id, schemas)| {
                let first = schemas.iter().min_by_key(|s| s.name()).unwrap();
                let ident = first.extensions().get::<CodegenIdent>().unwrap();
                let module_name = CodegenIdentUsage::Module(&ident).display().to_string();
                (scc_id, module_name)
            })
            .collect()
    }

    /// Returns all schemas in the same SCC as the named schema.
    fn schemas_in_scc<'a>(
        graph: &'a CodegenGraph<'a>,
        schema_name: &str,
    ) -> Vec<SchemaIrTypeView<'a>> {
        let target = graph.schemas().find(|s| s.name() == schema_name).unwrap();
        let target_scc = target.scc_id();
        graph
            .schemas()
            .filter(|s| s.scc_id() == target_scc)
            .collect()
    }

    #[test]
    fn test_scc_module_struct() {
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
                    age:
                      type: integer
                      format: int32
                  required:
                    - name
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let scc_module_names = build_scc_module_names(&graph);
        let schemas = schemas_in_scc(&graph, "Pet");
        let code = CodegenSccModule::new(&schemas, &scc_module_names).into_code();

        assert_eq!(code.path(), "models/pet.py");

        let source = code.into_string().unwrap();

        assert_eq!(
            source,
            indoc! {"
                from __future__ import annotations
                from pydantic import BaseModel
                class Pet(BaseModel):
                    name: str
                    age: int | None = None"
            },
        );
    }

    #[test]
    fn test_scc_module_enum() {
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
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let scc_module_names = build_scc_module_names(&graph);
        let schemas = schemas_in_scc(&graph, "Status");
        let code = CodegenSccModule::new(&schemas, &scc_module_names).into_code();

        assert_eq!(code.path(), "models/status.py");

        let source = code.into_string().unwrap();

        assert_eq!(
            source,
            indoc! {"
                from __future__ import annotations
                from enum import Enum
                class Status(Enum):
                    ACTIVE = 'active'
                    INACTIVE = 'inactive'"
            },
        );
    }

    #[test]
    fn test_scc_module_with_datetime() {
        let doc = Document::from_yaml(indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Event:
                  type: object
                  properties:
                    created_at:
                      type: string
                      format: date-time
                  required:
                    - created_at
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let scc_module_names = build_scc_module_names(&graph);
        let schemas = schemas_in_scc(&graph, "Event");
        let code = CodegenSccModule::new(&schemas, &scc_module_names).into_code();

        let source = code.into_string().unwrap();

        assert_eq!(
            source,
            indoc! {"
                from __future__ import annotations
                import datetime
                from pydantic import BaseModel
                class Event(BaseModel):
                    created_at: datetime.datetime"
            },
        );
    }

    #[test]
    fn test_scc_module_with_cross_scc_reference() {
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
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let scc_module_names = build_scc_module_names(&graph);

        // Pet and Owner form a cycle, so they should be in the same SCC.
        let pet = graph.schemas().find(|s| s.name() == "Pet").unwrap();
        let owner = graph.schemas().find(|s| s.name() == "Owner").unwrap();
        assert_eq!(
            pet.scc_id(),
            owner.scc_id(),
            "Pet and Owner should be in the same SCC"
        );

        // Build the SCC module for the Pet/Owner SCC.
        let schemas = schemas_in_scc(&graph, "Pet");
        let code = CodegenSccModule::new(&schemas, &scc_module_names).into_code();

        let source = code.into_string().unwrap();

        assert_eq!(
            source,
            indoc! {"
                from __future__ import annotations
                from pydantic import BaseModel
                class Owner(BaseModel):
                    name: str | None = None
                    pets: list[Pet] | None = None
                class Pet(BaseModel):
                    name: str | None = None
                    owner: Owner | None = None"
            },
        );
    }

    #[test]
    fn test_scc_module_cross_scc_direct_import() {
        // Pet references Status (different SCC). The import should be
        // a direct import, not under TYPE_CHECKING.
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
                    status:
                      $ref: '#/components/schemas/Status'
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
        let scc_module_names = build_scc_module_names(&graph);

        let schemas = schemas_in_scc(&graph, "Pet");
        let code = CodegenSccModule::new(&schemas, &scc_module_names).into_code();

        assert_eq!(code.path(), "models/pet.py");

        let source = code.into_string().unwrap();

        assert_eq!(
            source,
            indoc! {"
                from __future__ import annotations
                from pydantic import BaseModel
                from .status import Status
                class Pet(BaseModel):
                    name: str | None = None
                    status: Status | None = None"
            },
        );
    }
}
