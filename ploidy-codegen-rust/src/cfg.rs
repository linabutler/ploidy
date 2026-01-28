//! Feature-gating for conditional compilation.
//!
//! Ploidy supports three modes of `#[cfg(...)]` feature flagging:
//!
//! **None.** If a spec doesn't have any resource markers (`x-resourceId` on types,
//! or `x-resource-name` on operations), Ploidy doesn't generate any
//! `#[cfg(...)]` attributes.
//!
//! **Forward propagation** using `x-resourceId` on schema types.
//! Types declare their own features; for example, `#[cfg(feature = "customer")]`.
//! Transitivity is handled by Cargo feature dependencies: if `Customer` depends on
//! `Address`, the `customer` feature enables the `address` feature in `Cargo.toml`.
//! This is the style used by [Stripe's OpenAPI spec][stripe].
//!
//! **Backward propagation** using `x-resource-name` on operations.
//! Operations declare their features, which propagate to all the types they depend on.
//! Each type needs at least one of the features of the operations that use it; for example,
//! `#[cfg(any(feature = "orders", feature = "billing"))]`.
//!
//! When a spec mixes both styles, types can both have an own resource, and be used by
//! operations. This produces compound predicates like
//! `#[cfg(all(feature = "customer", any(feature = "orders", feature = "billing")))]`.
//!
//! [stripe]: https://github.com/stripe/openapi

use std::collections::BTreeSet;

use ploidy_core::ir::{InlineIrTypeView, IrOperationView, IrTypeView, SchemaIrTypeView, View};
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use super::{graph::CodegenGraph, naming::CargoFeature};

/// Generates a `#[cfg(...)]` attribute for conditional compilation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CfgFeature {
    /// A single `feature = "name"` predicate.
    Single(CargoFeature),
    /// A compound `any(feature = "a", feature = "b", ...)` predicate.
    AnyOf(BTreeSet<CargoFeature>),
    /// A compound `all(feature = "a", feature = "b", ...)` predicate.
    AllOf(BTreeSet<CargoFeature>),
    /// A compound `all(feature = "own", any(feature = "a", ...))` predicate,
    /// used for schema types that both specify an `x-resourceId`, and are
    /// used by operations that specify an `x-resource-name`.
    OwnAndUsedBy {
        own: CargoFeature,
        used_by: BTreeSet<CargoFeature>,
    },
}

impl CfgFeature {
    /// Builds a `#[cfg(...)]` attribute for a schema type, based on
    /// its own resource, and the resources of the operations that use it.
    /// Returns `None` if the type graph has no named resources.
    pub fn for_schema_type(graph: &CodegenGraph<'_>, view: &SchemaIrTypeView<'_>) -> Option<Self> {
        if !graph.has_resources() {
            return None;
        }

        let used_by_features: BTreeSet<_> = view
            .used_by()
            .filter_map(|op| op.resource().map(CargoFeature::from_name))
            .collect();

        match (view.resource(), used_by_features.is_empty()) {
            // Type has own resource, _and_ is used by operations.
            (Some(name), false) => {
                let needs_full = view
                    .reachable()
                    .skip(1) // Ignore ourselves.
                    .filter_map(IrTypeView::as_schema)
                    .any(|ty| ty.resource().is_none());
                let cfg = if needs_full {
                    Self::Single(CargoFeature::Full)
                } else {
                    Self::own_and_used_by(CargoFeature::from_name(name), used_by_features)
                };
                Some(cfg)
            }
            // Type has own resource only (Stripe-style; no resources on operations).
            (Some(name), true) => {
                let needs_full = view
                    .reachable()
                    .skip(1) // Ignore ourselves.
                    .filter_map(IrTypeView::as_schema)
                    .any(|ty| ty.resource().is_none());
                let feature = if needs_full {
                    CargoFeature::Full
                } else {
                    CargoFeature::from_name(name)
                };
                Some(Self::Single(feature))
            }
            // Type has no own resource, but is used by operations
            // (Swagger `@Tag` annotation style; no resources on types).
            (None, false) => Self::any_of(used_by_features),
            // No resource name; not used by any operation.
            (None, true) => Some(Self::Single(CargoFeature::Full)),
        }
    }

    /// Builds a `#[cfg(...)]` attribute for an inline type.
    /// Returns `None` if the type graph has no named resources.
    pub fn for_inline_type(graph: &CodegenGraph<'_>, view: &InlineIrTypeView<'_>) -> Option<Self> {
        if !graph.has_resources() {
            return None;
        }

        let used_by_features: BTreeSet<_> = view
            .used_by()
            .filter_map(|op| op.resource().map(CargoFeature::from_name))
            .collect();

        if used_by_features.is_empty() {
            // No operations use this inline type directly;
            // use its transitive schema types for feature-gating.
            let reachable_features: BTreeSet<_> = view
                .reachable()
                .skip(1) // Ignore ourselves.
                .filter_map(IrTypeView::as_schema)
                .map(|ty| {
                    ty.resource()
                        .map(CargoFeature::from_name)
                        .unwrap_or_default()
                })
                .collect();
            Self::all_of(reachable_features)
        } else {
            // Some operations use this inline type;
            // use those operations for feature-gating.
            Self::any_of(used_by_features)
        }
    }

    /// Builds a `#[cfg(...)]` attribute for a client method.
    /// Returns `None` if the type graph has no named resources.
    pub fn for_operation(graph: &CodegenGraph<'_>, view: &IrOperationView<'_>) -> Option<Self> {
        if !graph.has_resources() {
            return None;
        }

        // Collect features from transitive dependencies. Filter out unnamed
        // resources; "full" is handled via the feature itself.
        let features: BTreeSet<_> = view
            .reachable()
            .skip(1) // Ignore ourselves.
            .filter_map(IrTypeView::as_schema)
            .filter_map(|ty| ty.resource())
            .map(CargoFeature::from_name)
            .collect();

        Self::all_of(features)
    }

    /// Builds a `#[cfg(...)]` attribute for a resource `mod` declaration in a
    /// [`CodegenClientModule`](super::client::CodegenClientModule).
    /// Returns `None` if the type graph has no named resources.
    pub fn for_resource_module(graph: &CodegenGraph<'_>, feature: &CargoFeature) -> Option<Self> {
        if graph.has_resources() {
            Some(Self::Single(feature.clone()))
        } else {
            None
        }
    }

    /// Builds a `#[cfg(any(...))]` predicate, simplifying if possible.
    fn any_of(mut features: BTreeSet<CargoFeature>) -> Option<Self> {
        let first = features.pop_first()?;
        Some(if features.is_empty() {
            // Simplify `any(first)` to `first`.
            Self::Single(first)
        } else {
            features.insert(first);
            Self::AnyOf(features)
        })
    }

    /// Builds a `#[cfg(all(...))]` predicate, simplifying if possible.
    fn all_of(mut features: BTreeSet<CargoFeature>) -> Option<Self> {
        if features.contains(&CargoFeature::Full) {
            // `full` enables all other features, so `all(feature = "full", ...)`
            // simplifies to `feature = "full"`.
            return Some(Self::Single(CargoFeature::Full));
        }
        let first = features.pop_first()?;
        Some(if features.is_empty() {
            // Simplify `all(first)` to `first`.
            Self::Single(first)
        } else {
            features.insert(first);
            Self::AllOf(features)
        })
    }

    /// Builds a `#[cfg(all(own, any(...)))]` predicate, simplifying if possible.
    fn own_and_used_by(own: CargoFeature, mut used_by: BTreeSet<CargoFeature>) -> Self {
        if matches!(own, CargoFeature::Full) || used_by.contains(&CargoFeature::Full) {
            return Self::Single(CargoFeature::Full);
        }
        let Some(first) = used_by.pop_first() else {
            // No `used_by`; simplify to `own`.
            return Self::Single(own);
        };
        if used_by.is_empty() {
            // Simplify `all(own, any(first))` to `all(own, first)`.
            Self::AllOf(BTreeSet::from_iter([own, first]))
        } else {
            // Keep `all(own, any(first, used_by...))`.
            used_by.insert(first);
            Self::OwnAndUsedBy { own, used_by }
        }
    }
}

impl ToTokens for CfgFeature {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let predicate = match self {
            Self::Single(feature) => {
                let name = feature.display().to_string();
                quote! { feature = #name }
            }
            Self::AnyOf(features) => {
                let predicates = features.iter().map(|f| {
                    let name = f.display().to_string();
                    quote! { feature = #name }
                });
                quote! { any(#(#predicates),*) }
            }
            Self::AllOf(features) => {
                let predicates = features.iter().map(|f| {
                    let name = f.display().to_string();
                    quote! { feature = #name }
                });
                quote! { all(#(#predicates),*) }
            }
            Self::OwnAndUsedBy { own, used_by } => {
                let own_name = own.display().to_string();
                let used_by_predicates = used_by.iter().map(|f| {
                    let name = f.display().to_string();
                    quote! { feature = #name }
                });
                quote! { all(feature = #own_name, any(#(#used_by_predicates),*)) }
            }
        };
        tokens.extend(quote! { #[cfg(#predicate)] });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use itertools::Itertools;
    use ploidy_core::{
        ir::{IrGraph, IrSpec},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    // MARK: Predicates

    #[test]
    fn test_single_feature() {
        let cfg = CfgFeature::Single(CargoFeature::from_name("pets"));

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "pets")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_any_of_features() {
        let cfg = CfgFeature::AnyOf(BTreeSet::from_iter([
            CargoFeature::from_name("cats"),
            CargoFeature::from_name("dogs"),
            CargoFeature::from_name("aardvarks"),
        ]));

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute =
            parse_quote!(#[cfg(any(feature = "aardvarks", feature = "cats", feature = "dogs"))]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_all_of_features() {
        let cfg = CfgFeature::AllOf(BTreeSet::from_iter([
            CargoFeature::from_name("cats"),
            CargoFeature::from_name("dogs"),
            CargoFeature::from_name("aardvarks"),
        ]));

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute =
            parse_quote!(#[cfg(all(feature = "aardvarks", feature = "cats", feature = "dogs"))]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_own_and_used_by_feature() {
        let cfg = CfgFeature::OwnAndUsedBy {
            own: CargoFeature::from_name("own"),
            used_by: BTreeSet::from_iter([
                CargoFeature::from_name("a"),
                CargoFeature::from_name("b"),
            ]),
        };

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute =
            parse_quote!(#[cfg(all(feature = "own", any(feature = "a", feature = "b")))]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_any_of_simplifies_single_feature() {
        let cfg = CfgFeature::any_of(BTreeSet::from_iter([CargoFeature::from_name("pets")]));

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "pets")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_all_of_simplifies_single_feature() {
        let cfg = CfgFeature::all_of(BTreeSet::from_iter([CargoFeature::from_name("pets")]));

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "pets")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_any_of_returns_none_for_empty() {
        let cfg = CfgFeature::any_of(BTreeSet::new());
        assert_eq!(cfg, None);
    }

    #[test]
    fn test_all_of_returns_none_for_empty() {
        let cfg = CfgFeature::all_of(BTreeSet::new());
        assert_eq!(cfg, None);
    }

    #[test]
    fn test_all_of_simplifies_to_full_when_contains_full() {
        let cfg = CfgFeature::all_of(BTreeSet::from_iter([
            CargoFeature::from_name("customer"),
            CargoFeature::Full,
            CargoFeature::from_name("billing"),
        ]));

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "full")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_own_and_used_by_simplifies_single_used_by_to_all_of() {
        // `OwnedAndUsedBy` with one `used_by` feature should simplify to `AllOf`.
        let cfg = CfgFeature::own_and_used_by(
            CargoFeature::from_name("own"),
            BTreeSet::from_iter([CargoFeature::from_name("other")]),
        );
        assert_eq!(
            cfg,
            CfgFeature::AllOf(BTreeSet::from_iter([
                CargoFeature::from_name("other"),
                CargoFeature::from_name("own"),
            ]))
        );
    }

    #[test]
    fn test_own_and_used_by_simplifies_empty_to_single() {
        // `OwnedAndUsedBy` with no `used_by` features should simplify to `Single`.
        let cfg = CfgFeature::own_and_used_by(CargoFeature::from_name("own"), BTreeSet::new());
        assert_eq!(cfg, CfgFeature::Single(CargoFeature::from_name("own")));
    }

    #[test]
    fn test_own_and_used_by_simplifies_to_full_when_own_is_full() {
        let cfg = CfgFeature::own_and_used_by(
            CargoFeature::Full,
            BTreeSet::from_iter([CargoFeature::from_name("other")]),
        );
        assert_eq!(cfg, CfgFeature::Single(CargoFeature::Full));
    }

    #[test]
    fn test_own_and_used_by_simplifies_to_full_when_used_by_contains_full() {
        let cfg = CfgFeature::own_and_used_by(
            CargoFeature::from_name("own"),
            BTreeSet::from_iter([CargoFeature::from_name("other"), CargoFeature::Full]),
        );
        assert_eq!(cfg, CfgFeature::Single(CargoFeature::Full));
    }

    // MARK: Schema types

    #[test]
    fn test_for_schema_type_returns_empty_when_no_named_resources() {
        // Spec with no `x-resourceId` or `x-resource-name`.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Customer:
                  type: object
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        let customer = graph.schemas().find(|s| s.name() == "Customer").unwrap();

        // Shouldn't generate any feature gates for graph without named resources.
        let cfg = CfgFeature::for_schema_type(&graph, &customer);
        assert_eq!(cfg, None);
    }

    // MARK: Stripe-style

    #[test]
    fn test_for_schema_type_with_own_resource_no_deps() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        let customer = graph.schemas().find(|s| s.name() == "Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &customer);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "customer")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_schema_type_with_own_resource_and_unnamed_deps() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    address:
                      $ref: '#/components/schemas/Address'
                Address:
                  type: object
                  properties:
                    street:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        let customer = graph.schemas().find(|s| s.name() == "Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &customer);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "full")]);
        assert_eq!(actual, expected);
    }

    // MARK: Swagger `@Tag` style

    #[test]
    fn test_for_schema_type_used_by_single_operation() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /customers:
                get:
                  operationId: listCustomers
                  x-resource-name: customer
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            type: array
                            items:
                              $ref: '#/components/schemas/Customer'
            components:
              schemas:
                Customer:
                  type: object
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        let customer = graph.schemas().find(|s| s.name() == "Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &customer);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "customer")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_schema_type_used_by_multiple_operations() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /customers:
                get:
                  operationId: listCustomers
                  x-resource-name: customer
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            type: array
                            items:
                              $ref: '#/components/schemas/Address'
              /orders:
                get:
                  operationId: listOrders
                  x-resource-name: orders
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            type: array
                            items:
                              $ref: '#/components/schemas/Address'
            components:
              schemas:
                Address:
                  type: object
                  properties:
                    street:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        let address = graph.schemas().find(|s| s.name() == "Address").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &address);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute =
            parse_quote!(#[cfg(any(feature = "customer", feature = "orders"))]);
        assert_eq!(actual, expected);
    }

    // MARK: Hybrid style

    #[test]
    fn test_for_schema_type_with_own_and_used_by() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /billing:
                get:
                  operationId: getBilling
                  x-resource-name: billing
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Customer'
            components:
              schemas:
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        let customer = graph.schemas().find(|s| s.name() == "Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &customer);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute =
            parse_quote!(#[cfg(all(feature = "billing", feature = "customer"))]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_schema_type_with_own_and_multiple_used_by() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /billing:
                get:
                  operationId: getBilling
                  x-resource-name: billing
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Customer'
              /orders:
                get:
                  operationId: getOrders
                  x-resource-name: orders
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Customer'
            components:
              schemas:
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        let customer = graph.schemas().find(|s| s.name() == "Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &customer);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(
            #[cfg(all(feature = "customer", any(feature = "billing", feature = "orders")))]
        );
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_schema_type_with_own_used_by_and_unnamed_deps() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /billing:
                get:
                  operationId: getBilling
                  x-resource-name: billing
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Customer'
            components:
              schemas:
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    address:
                      $ref: '#/components/schemas/Address'
                Address:
                  type: object
                  properties:
                    street:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        let customer = graph.schemas().find(|s| s.name() == "Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &customer);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "full")]);
        assert_eq!(actual, expected);
    }

    // MARK: Types without resources

    #[test]
    fn test_for_schema_type_unnamed_no_operations() {
        // Spec has a named resource (`Customer`), but `Simple` has
        // no `x-resourceId` and isn't used.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Simple:
                  type: object
                  properties:
                    id:
                      type: string
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    name:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        let simple = graph.schemas().find(|s| s.name() == "Simple").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &simple);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "full")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_schema_type_full_as_resource_name_gets_full_feature() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Simple:
                  type: object
                  x-resourceId: full
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        let simple = graph.schemas().find(|s| s.name() == "Simple").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &simple);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "full")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_schema_type_empty_resource_name_gets_full_feature() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Simple:
                  type: object
                  x-resourceId: ''
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        let simple = graph.schemas().find(|s| s.name() == "Simple").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &simple);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "full")]);
        assert_eq!(actual, expected);
    }

    // MARK: Cycles with mixed resources

    #[test]
    fn test_for_schema_type_cycle_with_mixed_resources() {
        // Type A (resource `a`) -> Type B (no resource) -> Type C (resource `c`) -> Type A.
        // All types should get `full` because B has no resource.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                A:
                  type: object
                  x-resourceId: a
                  properties:
                    b:
                      $ref: '#/components/schemas/B'
                B:
                  type: object
                  properties:
                    c:
                      $ref: '#/components/schemas/C'
                C:
                  type: object
                  x-resourceId: c
                  properties:
                    a:
                      $ref: '#/components/schemas/A'
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        // A depends on B (unnamed), so A needs `full`.
        let a = graph.schemas().find(|s| s.name() == "A").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &a);
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "full")]);
        assert_eq!(actual, expected);

        // C depends on A, which depends on B (unnamed), so C also needs `full`.
        let c = graph.schemas().find(|s| s.name() == "C").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &c);
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "full")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_schema_type_cycle_with_all_named_resources() {
        // Type A (resource `a`) -> Type B (resource `b`) -> Type C (resource `c`) -> Type A.
        // Each type gets its own feature; transitivity is handled by
        // Cargo feature dependencies.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                A:
                  type: object
                  x-resourceId: a
                  properties:
                    b:
                      $ref: '#/components/schemas/B'
                B:
                  type: object
                  x-resourceId: b
                  properties:
                    c:
                      $ref: '#/components/schemas/C'
                C:
                  type: object
                  x-resourceId: c
                  properties:
                    a:
                      $ref: '#/components/schemas/A'
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        // Each type uses just its own feature; Cargo feature dependencies
        // handle the transitive requirements.
        let a = graph.schemas().find(|s| s.name() == "A").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &a);
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "a")]);
        assert_eq!(actual, expected);

        let b = graph.schemas().find(|s| s.name() == "B").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &b);
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "b")]);
        assert_eq!(actual, expected);

        let c = graph.schemas().find(|s| s.name() == "C").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &c);
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "c")]);
        assert_eq!(actual, expected);
    }

    // MARK: Inline types

    #[test]
    fn test_for_inline_returns_empty_when_no_named_resources() {
        // Spec with no `x-resourceId` or `x-resource-name`.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: getItems
                  parameters:
                    - name: filter
                      in: query
                      schema:
                        type: object
                        properties:
                          status:
                            type: string
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        let ops = graph.operations().collect_vec();
        let inlines = ops.iter().flat_map(|op| op.inlines()).collect_vec();
        assert!(!inlines.is_empty());

        // Shouldn't generate any feature gates for graph without named resources.
        let cfg = CfgFeature::for_inline_type(&graph, &inlines[0]);
        assert_eq!(cfg, None);
    }

    // MARK: Resource modules

    #[test]
    fn test_for_resource_module_returns_empty_when_no_named_resources() {
        // Spec with no `x-resourceId` or `x-resource-name`.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /health:
                get:
                  operationId: healthCheck
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        let cfg = CfgFeature::for_resource_module(&graph, &CargoFeature::from_name("pets"));
        assert_eq!(cfg, None);
    }

    #[test]
    fn test_for_resource_module_with_named_resources() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /pets:
                get:
                  operationId: listPets
                  x-resource-name: pets
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        let cfg = CfgFeature::for_resource_module(&graph, &CargoFeature::from_name("pets"));

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "pets")]);
        assert_eq!(actual, expected);
    }
}
