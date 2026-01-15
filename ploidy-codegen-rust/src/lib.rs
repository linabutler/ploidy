use std::{collections::BTreeMap, path::Path};

use itertools::Itertools;
use proc_macro2::TokenStream;
use quote::quote;

use ploidy_core::codegen::{IntoCode, write_to_disk};

mod cargo;
mod client;
mod derives;
mod enum_;
mod graph;
mod naming;
mod operation;
mod ref_;
mod resource;
mod schema;
mod statics;
mod struct_;
mod tagged;
mod types;
mod untagged;

pub use cargo::*;
pub use client::*;
pub use graph::*;
pub use naming::*;
pub use operation::*;
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
    let by_resource = graph
        .operations()
        .fold(BTreeMap::<_, Vec<_>>::new(), |mut map, view| {
            let resource = view.resource();
            map.entry(resource).or_default().push(view);
            map
        });

    for (resource, operations) in &by_resource {
        let code = CodegenResource::new(resource, operations);
        write_to_disk(output, code)?;
    }

    let resources = by_resource.keys().copied().collect_vec();
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
