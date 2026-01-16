//! Ploidy Python code generator using Pydantic v2 models.
//!
//! This crate generates Python code from OpenAPI schemas, producing Pydantic
//! `BaseModel` classes with full type hints for Python 3.10+.

use std::{collections::BTreeMap, path::Path};

use ploidy_core::{
    codegen::{IntoCode, write_to_disk},
    ir::{ExtendableView, SccId, ViewNode},
};
use quasiquodo_py::ruff::python_ast::Suite;

mod enum_;
mod graph;
mod imports;
mod model;
mod naming;
mod ref_;
mod schema;
mod tagged;
mod types;
mod untagged;

#[cfg(test)]
mod tests;

pub use graph::*;
pub use naming::*;
pub use schema::*;
pub use types::*;

/// Generates Python source code from a list of statements.
pub(crate) fn generate_source(suite: &Suite) -> String {
    use ruff_python_codegen::{Generator, Indentation};
    use ruff_source_file::LineEnding;

    let indent = Indentation::default();
    suite
        .iter()
        .map(|stmt| Generator::new(&indent, LineEnding::Lf).stmt(stmt))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generates Python source code from an expression.
#[cfg(test)]
pub(crate) fn generate_expr_source(expr: &quasiquodo_py::ruff::python_ast::Expr) -> String {
    use ruff_python_codegen::{Generator, Indentation};
    use ruff_source_file::LineEnding;

    let indent = Indentation::default();
    Generator::new(&indent, LineEnding::Lf).expr(expr)
}

/// Writes generated Python types to disk.
///
/// Groups schemas by SCC and emits one `.py` file per SCC, plus an
/// `__init__.py` module file that exports all types.
pub fn write_types_to_disk(output: &Path, graph: &CodegenGraph<'_>) -> miette::Result<()> {
    // Group schemas by SCC.
    let mut sccs: BTreeMap<SccId, Vec<_>> = BTreeMap::new();
    for view in graph.schemas() {
        sccs.entry(view.scc_id()).or_default().push(view);
    }

    // Pre-compute the module name for each SCC (the alphabetically first
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

    // Emit one module per SCC.
    for schemas in sccs.values() {
        let code = CodegenSccModule::new(schemas, &scc_module_names).into_code();
        write_to_disk(output, code)?;
    }

    write_to_disk(output, CodegenTypesModule::new(graph))?;

    Ok(())
}
