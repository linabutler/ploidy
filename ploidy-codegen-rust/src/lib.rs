use std::{collections::BTreeMap, path::Path};

use itertools::Itertools;
use proc_macro2::TokenStream;
use quote::quote;

use ploidy_core::{
    codegen::{IntoCode, WrittenFile, write_to_disk},
    ir::{HasTypeId, ResponseView, TypeId, TypeView},
};

mod cargo;
mod cfg;
mod client;
mod config;
mod derives;
mod enum_;
mod ext;
mod graph;
mod inlines;
mod naming;
mod operation;
mod primitive;
mod query;
mod ref_;
mod resource;
mod schema;
mod statics;
mod struct_;
mod tagged;
mod types;
mod untagged;

#[cfg(test)]
mod tests;

pub use cargo::*;
pub use cfg::*;
pub use client::*;
pub use config::*;
pub use graph::*;
pub use naming::*;
pub use operation::*;
pub use primitive::*;
pub use query::*;
pub use resource::*;
pub use schema::*;
pub use statics::*;
pub use types::*;

pub fn write_types_to_disk(
    output: &Path,
    graph: &CodegenGraph<'_>,
) -> miette::Result<Vec<WrittenFile>> {
    let mut written = Vec::new();

    for schema in graph.schemas() {
        let code = CodegenSchemaType::new(graph, &schema).into_code();
        written.push(write_to_disk(output, code)?);
    }

    for op in graph.operations() {
        let mut shapes: Vec<Option<TypeId>> = vec![];
        for case in op.response_cases() {
            let shape = case.body().map(|body| match body {
                ResponseView::Json(view) | ResponseView::Headers(view) => match view {
                    TypeView::Schema(view) => view.id(),
                    TypeView::Inline(view) => view.id(),
                },
            });
            if !shapes.contains(&shape) {
                shapes.push(shape);
            }
        }
        if shapes.len() > 1 {
            let code = CodegenOperationResult::new(graph, &op).into_code();
            written.push(write_to_disk(output, code)?);
        }
    }

    written.push(write_to_disk(output, CodegenTypesModule::new(graph))?);

    Ok(written)
}

pub fn write_client_to_disk(
    output: &Path,
    graph: &CodegenGraph<'_>,
) -> miette::Result<Vec<WrittenFile>> {
    // Group operations by resource name.
    let ops_by_resource: BTreeMap<_, Vec<_>> =
        graph.operations().fold(BTreeMap::default(), |mut map, op| {
            let resource = graph.resource_for(&op);
            map.entry(resource).or_default().push(op);
            map
        });

    let mut written = Vec::new();

    // Write a module per resource.
    for (ident, ops) in &ops_by_resource {
        written.push(write_to_disk(
            output,
            CodegenResource::new(graph, *ident, ops),
        )?);
    }

    // Write the top-level client module.
    let idents = ops_by_resource.keys().copied().collect_vec();
    written.push(write_to_disk(
        output,
        CodegenClientModule::new(graph, &idents),
    )?);

    Ok(written)
}

/// Generates one or more `#[doc]` attributes for a schema description,
/// wrapping at 80 characters for readability.
pub fn doc_attrs(description: &str) -> TokenStream {
    use textwrap::{Options, dedent, wrap};
    let dedented = dedent(description);
    let lines = wrap(
        &dedented,
        Options::new(80)
            .initial_indent(" ")
            .subsequent_indent(" ")
            .break_words(false),
    )
    .into_iter()
    .map(|line| quote!(#[doc = #line]));
    quote! { #(#lines)* }
}
