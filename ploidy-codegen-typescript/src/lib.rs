use std::path::Path;

use ploidy_core::codegen::write_to_disk;

mod client;
mod emit;
mod enum_;
mod graph;
mod naming;
mod operation;
mod primitive;
mod ref_;
mod schema;
mod struct_;
mod tagged;
mod types_module;
mod untagged;

#[cfg(test)]
mod tests;

pub use client::*;
pub use graph::*;
pub use naming::*;
pub use schema::*;
pub use types_module::*;

/// Writes TypeScript type declarations to disk.
///
/// Generates one `.ts` file per schema type under `types/`, plus a
/// barrel `types/index.ts` that re-exports all types.
pub fn write_types_to_disk(output: &Path, graph: &CodegenGraph<'_>) -> miette::Result<()> {
    for view in graph.schemas() {
        let code = CodegenSchemaType::new(&view).into_code();
        write_to_disk(output, code)?;
    }

    write_to_disk(output, CodegenTypesModule::new(graph))?;

    Ok(())
}

/// Writes a TypeScript HTTP client to disk.
///
/// Generates a `client.ts` file with a `Client` class containing
/// async methods for each OpenAPI operation.
pub fn write_client_to_disk(output: &Path, graph: &CodegenGraph<'_>) -> miette::Result<()> {
    let code = CodegenClient::new(graph).into_code();
    write_to_disk(output, code)?;
    Ok(())
}
