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

pub fn write_types_to_disk(output: &Path, graph: &CodegenGraph<'_>) -> miette::Result<()> {
    for schema in graph.schemas() {
        let code = CodegenSchemaType::new(graph, &schema).into_code();
        write_to_disk(output, code)?;
    }

    write_to_disk(output, CodegenTypesModule::new(graph))?;

    Ok(())
}

pub fn write_client_to_disk(output: &Path, graph: &CodegenGraph<'_>) -> miette::Result<()> {
    // Group operations by resource name. Operations without
    // `x-resource-name` go into the `None` group.
    let ops_by_resource = graph
        .operations()
        .fold(BTreeMap::<_, Vec<_>>::new(), |mut map, op| {
            map.entry(op.resource()).or_default().push(op);
            map
        });

    // Derive a `CargoFeature` and module identifier for each resource.
    let resource_meta: BTreeMap<_, _> = ops_by_resource
        .keys()
        .map(|&resource| {
            let feature = resource.map(CargoFeature::from_name).unwrap_or_default();
            let module_ident = resource.and_then(|r| graph.resource(r)).unwrap_or_default();
            (resource, (feature, module_ident))
        })
        .collect();

    // Write each resource's operations into a separate module.
    for (&resource, ops) in &ops_by_resource {
        let (_, module_ident) = &resource_meta[&resource];
        write_to_disk(output, CodegenResource::new(graph, *module_ident, ops))?;
    }

    // Write the top-level client module.
    let resources = resource_meta.values().map(|(f, m)| (f, *m)).collect_vec();
    let mod_code = CodegenClientModule::new(graph, &resources);
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
