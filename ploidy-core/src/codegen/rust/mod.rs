use std::{collections::BTreeMap, path::Path};

use indexmap::IndexMap;
use itertools::Itertools;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, parse_quote};

use crate::{
    codegen::{IntoCode, unique::UniqueNameSpace, write_to_disk},
    ir::{IrGraph, IrOperationView, IrType, IrTypeRef},
};

mod cargo;
mod client;
mod context;
mod derives;
mod enum_;
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
pub use context::*;
pub use naming::*;
pub use operation::*;
pub use resource::*;
pub use schema::*;
pub use statics::*;
pub use types::*;

pub fn write_types_to_disk(output: &Path, context: &CodegenContext<'_>) -> miette::Result<()> {
    for (name, view) in context.graph.schemas() {
        let Some(info) = context.map.0.get(name) else {
            continue;
        };
        if view.used_by().next().is_none() {
            // Skip types that aren't used by any operations.
            continue;
        }
        let name = CodegenTypeName::Schema(name, &info.ty);
        let code = match view.to_ref() {
            IrTypeRef::Schema(ty) => CodegenSchemaType::new(context, name, ty).into_code(),
            IrTypeRef::Nullable(ty) | IrTypeRef::Array(ty) | IrTypeRef::Map(ty) => {
                CodegenSchemaTypeAlias::new(context, name, ty).into_code()
            }
            IrTypeRef::Primitive(ty) => {
                let ty = IrType::Primitive(ty);
                CodegenSchemaTypeAlias::new(context, name, &ty).into_code()
            }
            IrTypeRef::Any => CodegenSchemaTypeAlias::new(context, name, &IrType::Any).into_code(),
            IrTypeRef::Inline(..) | IrTypeRef::Ref(..) => continue,
        };
        write_to_disk(output, code)?;
    }

    write_to_disk(output, CodegenTypesModule::new(context))?;

    Ok(())
}

pub fn write_client_to_disk(output: &Path, context: &CodegenContext<'_>) -> miette::Result<()> {
    let mut operations_by_resource: BTreeMap<&str, Vec<IrOperationView<'_>>> = BTreeMap::new();
    for view in context.graph.operations() {
        let resource = view.as_operation().resource;
        operations_by_resource
            .entry(resource)
            .or_default()
            .push(view);
    }

    for (resource, operations) in &operations_by_resource {
        let code = CodegenResource::new(context, resource, operations.as_slice());
        write_to_disk(output, code)?;
    }

    let resources = operations_by_resource.keys().cloned().collect_vec();
    let mod_code = CodegenClientModule::new(context, &resources);
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

#[derive(Debug)]
pub struct SchemaIdents {
    pub module: Ident,
    pub ty: Ident,
}

/// A mapping of schema names from the original spec
/// to Rust identifiers for the generated module and type names.
#[derive(Debug)]
pub struct SchemaIdentMap<'a>(pub IndexMap<&'a str, SchemaIdents>);

impl<'a> SchemaIdentMap<'a> {
    pub fn new(graph: &'a IrGraph<'a>) -> Self {
        let mut space = UniqueNameSpace::new();
        let map = graph
            .schemas()
            .map(|(name, _)| {
                let unique_name = space.uniquify(name);
                let module = CodegenIdent::Module(&unique_name);
                let ty = CodegenIdent::Type(&unique_name);
                (
                    name,
                    SchemaIdents {
                        module: parse_quote!(#module),
                        ty: parse_quote!(#ty),
                    },
                )
            })
            .collect();
        Self(map)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &SchemaIdents)> {
        self.0.iter().map(|(&key, idents)| (key, idents))
    }

    pub fn module(&self, name: &str) -> Option<&Ident> {
        self.0.get(name).map(|idents| &idents.module)
    }

    pub fn ty(&self, name: &str) -> Option<&Ident> {
        self.0.get(name).map(|idents| &idents.ty)
    }
}
