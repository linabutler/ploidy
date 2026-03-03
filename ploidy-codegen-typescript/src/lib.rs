use std::path::Path;

use ploidy_core::codegen::{Code, write_to_disk};
use quasiquodo_ts::{
    Comments,
    swc::common::{SourceMap, sync::Lrc},
};
use swc_ecma_codegen::{Node, text_writer::JsWriter};

mod client;
mod enum_;
mod graph;
mod naming;
mod operation;
mod primitive;
mod ref_;
mod schema;
mod struct_;
mod tagged;
mod types;
mod untagged;

#[cfg(test)]
mod tests;

pub use client::*;
pub use graph::*;
pub use naming::*;
pub use schema::*;
pub use types::*;

/// Writes TypeScript type declarations to disk.
///
/// Generates one `.ts` file per schema type under `types/`, plus a
/// barrel `types/index.ts` that re-exports all types.
pub fn write_types_to_disk(output: &Path, graph: &CodegenGraph<'_>) -> miette::Result<()> {
    for view in graph.schemas() {
        let code = CodegenSchemaType::new(&view).into_code();
        write_to_disk(output, code)?;
    }

    write_to_disk(output, CodegenTypesModule::new(graph).into_code())?;

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

pub struct TsSource<T> {
    path: String,
    comments: Comments,
    body: T,
}

impl<T> TsSource<T> {
    pub fn new(path: String, comments: Comments, body: T) -> Self {
        Self {
            path,
            comments,
            body,
        }
    }
}

impl<T: Node> Code for TsSource<T> {
    fn path(&self) -> &str {
        &self.path
    }

    fn into_string(self) -> miette::Result<String> {
        let mut buf = Vec::new();
        let cm = Lrc::new(SourceMap::default());
        let mut wr = JsWriter::new(cm.clone(), "\n", &mut buf, None);
        wr.set_indent_str("  ");
        let mut emitter = swc_ecma_codegen::Emitter {
            cfg: swc_ecma_codegen::Config::default(),
            cm,
            comments: Some(&*self.comments),
            wr: Box::new(wr),
        };
        self.body.emit_with(&mut emitter).unwrap();
        Ok(String::from_utf8(buf).unwrap())
    }
}
