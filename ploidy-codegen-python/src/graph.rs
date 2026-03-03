//! The codegen graph wrapper with Python-specific metadata.

use std::{
    collections::{BTreeMap, HashMap},
    ops::Deref,
};

use ploidy_core::{
    codegen::UniqueNames,
    ir::{ExtendableView, IrGraph, IrTypeView, SchemaIrTypeView},
};

use super::naming::CodegenIdentScope;

/// Decorates an [`IrGraph`] with Python-specific information.
#[derive(Debug)]
pub struct CodegenGraph<'a>(IrGraph<'a>);

impl<'a> CodegenGraph<'a> {
    /// Creates a new codegen graph, computing unique Python names for all
    /// schemas and tracking discriminator fields for tagged union variants.
    pub fn new(graph: IrGraph<'a>) -> Self {
        let unique = UniqueNames::new();
        let mut scope = CodegenIdentScope::new(&unique);

        // First pass: assign unique identifiers to all schemas.
        for mut view in graph.schemas() {
            let ident = scope.uniquify(view.name());
            view.extensions_mut().insert(ident);
        }

        // Second pass: collect discriminator info for all variant schemas.
        // Use BTreeMap to maintain sorted order and deduplicate by field name.
        let mut discriminators: HashMap<&str, BTreeMap<String, Vec<String>>> = HashMap::new();
        for schema in graph.schemas() {
            if let SchemaIrTypeView::Tagged(_, tagged) = schema {
                let tag = tagged.tag();
                for variant in tagged.variants() {
                    let value = variant.aliases().first().copied().unwrap_or(variant.name());
                    if let IrTypeView::Schema(variant_schema) = variant.ty() {
                        discriminators
                            .entry(variant_schema.name())
                            .or_default()
                            .entry(tag.to_owned())
                            .or_default()
                            .push(value.to_owned());
                    }
                }
            }
        }

        // Third pass: insert collected discriminator info into schema extensions.
        for mut schema in graph.schemas() {
            if let Some(fields_by_name) = discriminators.remove(schema.name()) {
                let fields = fields_by_name
                    .into_iter()
                    .map(|(field_name, values)| DiscriminatorField { field_name, values })
                    .collect();
                schema.extensions_mut().insert(DiscriminatorFields(fields));
            }
        }

        Self(graph)
    }
}

impl<'a> Deref for CodegenGraph<'a> {
    type Target = IrGraph<'a>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Information about a discriminator field that should be added to a schema
/// because it's used as a variant in a tagged union.
#[derive(Clone, Debug)]
pub struct DiscriminatorField {
    /// The discriminator field name.
    pub field_name: String,
    /// The discriminator values for this variant. Multiple values occur when a
    /// schema participates in multiple tagged unions with the same discriminator
    /// field name.
    pub values: Vec<String>,
}

/// Collection of discriminator fields for a schema that's used as a variant
/// in one or more tagged unions.
#[derive(Clone, Debug, Default)]
pub struct DiscriminatorFields(pub Vec<DiscriminatorField>);
