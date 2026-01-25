use std::{collections::BTreeMap, path::Path};

use itertools::Itertools;
use proc_macro2::TokenStream;
use quote::quote;

use ploidy_core::codegen::{IntoCode, write_to_disk};

mod cargo;
mod cfg;
mod client;
mod config;
mod derives;
mod enum_;
mod graph;
mod inlines;
mod naming;
mod operation;
mod primitive;
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
pub use inlines::*;
pub use naming::*;
pub use operation::*;
pub use primitive::*;
pub use resource::*;
pub use schema::*;
pub use statics::*;
pub use types::*;

pub fn write_types_to_disk(output: &Path, graph: &CodegenGraph<'_>) -> miette::Result<()> {
    for view in graph.schemas() {
        let code = CodegenSchemaType::new(&view).into_code();
        write_to_disk(output, code)?;
    }

    write_to_disk(output, CodegenTypesModule::new(graph))?;

    Ok(())
}

pub fn write_client_to_disk(output: &Path, graph: &CodegenGraph<'_>) -> miette::Result<()> {
    // Group operations by feature. All operations belong to a feature,
    // or `full` for operations without a named resource.
    let ops_by_feature = graph
        .operations()
        .fold(BTreeMap::<_, Vec<_>>::new(), |mut map, view| {
            let feature = view
                .resource()
                .map(CargoFeature::from_name)
                .unwrap_or_default();
            map.entry(feature).or_default().push(view);
            map
        });

    // Write all operations for each feature into separate modules.
    for (feature, ops) in &ops_by_feature {
        let code = CodegenResource::new(graph, feature, ops);
        write_to_disk(output, code)?;
    }

    // Write the top-level client module.
    let features = ops_by_feature.keys().collect_vec();
    let mod_code = CodegenClientModule::new(graph, &features);
    write_to_disk(output, mod_code)?;

    Ok(())
}

/// Generates one or more `#[doc]` attributes for a schema description,
/// wrapping at 80 characters for readability.
pub fn doc_attrs(description: &str) -> TokenStream {
    let lines = textwrap::wrap(description, 80)
        .into_iter()
        .map(|line| quote!(#[doc = #line]));
    quote! { #(#lines)* }
}
