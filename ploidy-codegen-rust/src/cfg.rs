//! Feature-gating for conditional compilation.
//!
//! Ploidy infers Cargo features from resource markers (`x-resourceId` on types;
//! `x-resource-name` on operations), and propagates them forward and backward.
//!
//! In **forward propagation**, `x-resourceId` fields become `#[cfg(...)]` attributes
//! on types and the operations that use them. Transitivity is handled by
//! Cargo feature dependencies: for example, if `Customer` depends on `Address`,
//! the `customer` feature enables the `address` feature in `Cargo.toml`, and
//! the attribute reduces to `#[cfg(feature = "customer")]`.
//! This is the style used by [Stripe's OpenAPI spec][stripe].
//!
//! In **backward propagation**, `x-resource-name` fields become `#[cfg(...)]` attributes
//! on operations and the types they depend on. Each type needs at least one of
//! the features of the operations that use it: for example,
//! `#[cfg(any(feature = "orders", feature = "billing"))]`.
//!
//! When a spec mixes both styles, types can both have an own resource, and be used by
//! operations. This produces compound predicates like
//! `#[cfg(all(feature = "customer", any(feature = "orders", feature = "billing")))]`.
//!
//! [stripe]: https://github.com/stripe/openapi

use std::collections::BTreeSet;

use ploidy_core::ir::{HasResource, InlineTypeView, OperationView, SchemaTypeView, View};
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use super::{
    graph::CodegenGraph,
    naming::{AsFeatureName, ResourceGroup, UniqueIdent},
};

/// Generates a `#[cfg(...)]` attribute for conditional compilation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CfgFeature<'a> {
    /// A single `feature = "name"` predicate.
    Single(&'a UniqueIdent),
    /// A compound `any(feature = "a", feature = "b", ...)` predicate.
    AnyOf(BTreeSet<&'a UniqueIdent>),
    /// A compound `all(feature = "a", feature = "b", ...)` predicate.
    AllOf(BTreeSet<&'a UniqueIdent>),
    /// A compound `all(feature = "own", any(feature = "a", ...))` predicate,
    /// used for schema types that both specify an `x-resourceId`, and are
    /// used by operations that specify an `x-resource-name`.
    OwnAndUsedBy {
        own: &'a UniqueIdent,
        used_by: BTreeSet<&'a UniqueIdent>,
    },
}

impl<'a> CfgFeature<'a> {
    /// Builds a `#[cfg(...)]` attribute for a schema type, based on
    /// its own resource, and the resources of the operations that use it.
    pub fn for_schema_type(
        graph: &CodegenGraph<'a>,
        view: &SchemaTypeView<'_, 'a>,
    ) -> Option<Self> {
        // Types in the default resource group aren't feature-gated.
        // If this type is in the default group, or is depended on by a type
        // in that group, then it can't have a feature gate, either.
        if in_default_resource_group(graph, view)
            || view
                .dependents()
                .filter_map(|ty| ty.into_schema().right())
                .any(|schema| in_default_resource_group(graph, &schema))
        {
            return None;
        }

        // Compute all the operations with resources that use this type.
        let used_by_resources: BTreeSet<_> = view
            .used_by()
            .filter_map(|op| graph.resource_for(&op).name())
            .collect();

        match (graph.resource_for(view), used_by_resources.is_empty()) {
            // Type has own resource, _and_ is used by operations.
            (ResourceGroup::Named(own), false) => {
                Some(Self::own_and_used_by(own, used_by_resources))
            }
            // Type has own resource only (Stripe-style; no resources on operations).
            (ResourceGroup::Named(own), true) => Some(Self::Single(own)),
            // Type has no own resource, but is used by operations
            // (resource annotation-style; no resources on types).
            (ResourceGroup::Default, false) => Self::any_of(used_by_resources),
            // No resource name; not used by any operation.
            (ResourceGroup::Default, true) => None,
        }
    }

    /// Builds a `#[cfg(...)]` attribute for an inline type.
    pub fn for_inline_type(
        graph: &CodegenGraph<'a>,
        view: &InlineTypeView<'_, 'a>,
    ) -> Option<Self> {
        // If this type is depended on by a type in the default resource group,
        // then it can't have a feature gate.
        if view
            .dependents()
            .filter_map(|ty| ty.into_schema().right())
            .any(|schema| in_default_resource_group(graph, &schema))
        {
            return None;
        }

        let used_by_resources: BTreeSet<_> = view
            .used_by()
            .filter_map(|op| {
                use ResourceGroup::*;
                match (graph.resource_for(view), graph.resource_for(&op)) {
                    (Default, Named(resource)) => Some(resource),
                    (Named(own), Named(resource)) if own != resource => Some(resource),
                    _ => None,
                }
            })
            .collect();

        if used_by_resources.is_empty() {
            // No operations use this inline type directly, so use its
            // transitive dependencies for gating.
            Self::for_transitive_dependencies(graph, view)
        } else {
            // Some operations use this inline type; use those operations for gating.
            Self::any_of(used_by_resources)
        }
    }

    /// Builds a `#[cfg(...)]` attribute for a client method.
    pub fn for_operation(graph: &CodegenGraph<'a>, view: &OperationView<'_, 'a>) -> Option<Self> {
        Self::for_transitive_dependencies(graph, view)
    }

    /// Builds a `#[cfg(...)]` attribute from a view's resource dependencies.
    ///
    /// Reduces the set of resource features in a view's dependencies by
    /// removing features that are transitively implied by other features.
    /// If feature A's type depends on feature B's type, then enabling A
    /// in `Cargo.toml` already enables B, so B is redundant.
    fn for_transitive_dependencies<'graph>(
        graph: &'graph CodegenGraph<'a>,
        view: &(impl HasResource<'a> + View<'graph, 'a>),
    ) -> Option<Self> {
        Self::all_of(
            view.dependencies()
                .filter_map(|ty| {
                    let schema = ty.into_schema().right()?;
                    // Filter out dependencies without a resource name,
                    // because these aren't feature-gated.
                    let resource = graph.resource_for(&schema).name()?;
                    // Keep our resource feature unless the other schema
                    // depends on it, meaning that the other feature already
                    // enables ours. If this schema and the other schema
                    // depend on each other, the lexicographically lower
                    // resource feature breaks the tie.
                    let implied = view
                        .dependencies()
                        .filter_map(|ty| ty.into_schema().right())
                        .any(|other_schema| {
                            let ResourceGroup::Named(other_resource) =
                                graph.resource_for(&other_schema)
                            else {
                                return false;
                            };
                            other_schema.depends_on(&schema)
                                && (!schema.depends_on(&other_schema) || other_resource < resource)
                        });
                    (!implied).then_some(resource)
                })
                .filter(|&resource| match graph.resource_for(view) {
                    ResourceGroup::Default => true,
                    ResourceGroup::Named(own) if own != resource => true,
                    _ => false,
                })
                .collect(),
        )
    }

    /// Builds a `#[cfg(any(...))]` predicate, simplifying if possible.
    fn any_of(mut features: BTreeSet<&'a UniqueIdent>) -> Option<Self> {
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
    fn all_of(mut features: BTreeSet<&'a UniqueIdent>) -> Option<Self> {
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
    fn own_and_used_by(own: &'a UniqueIdent, mut used_by: BTreeSet<&'a UniqueIdent>) -> Self {
        if used_by.contains(own) {
            // Simplify `all(own, any(own, ...))` to `own`.
            return Self::Single(own);
        }
        let Some(first) = used_by.pop_first() else {
            // Simplify `all(own)` to `own`.
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

impl ToTokens for CfgFeature<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let predicate = match self {
            Self::Single(resource) => {
                let name = AsFeatureName(resource).to_string();
                quote! { feature = #name }
            }
            Self::AnyOf(resources) => {
                let predicates = resources.iter().map(|f| {
                    let name = AsFeatureName(f).to_string();
                    quote! { feature = #name }
                });
                quote! { any(#(#predicates),*) }
            }
            Self::AllOf(resources) => {
                let predicates = resources.iter().map(|f| {
                    let name = AsFeatureName(f).to_string();
                    quote! { feature = #name }
                });
                quote! { all(#(#predicates),*) }
            }
            Self::OwnAndUsedBy { own, used_by } => {
                let own_name = AsFeatureName(own).to_string();
                let used_by_predicates = used_by.iter().map(|f| {
                    let name = AsFeatureName(f).to_string();
                    quote! { feature = #name }
                });
                quote! { all(feature = #own_name, any(#(#used_by_predicates),*)) }
            }
        };
        tokens.extend(quote! { #[cfg(#predicate)] });
    }
}

fn in_default_resource_group<'graph, 'a>(
    graph: &'graph CodegenGraph<'a>,
    view: &(impl View<'graph, 'a> + HasResource<'a>),
) -> bool {
    let mut used_by = view.used_by().map(|op| graph.resource_for(&op)).peekable();
    graph.resource_for(view).is_default()
        && (used_by.peek().is_none() || used_by.any(|resource| resource.is_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    use itertools::Itertools;
    use ploidy_core::{
        arena::Arena,
        ir::{RawGraph, Spec},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    use crate::{UniqueIdents, graph::CodegenGraph};

    // MARK: Predicates

    #[test]
    fn test_single_feature() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let cfg = CfgFeature::Single(scope.ident("pets"));

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "pets")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_any_of_features() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let cfg = CfgFeature::AnyOf(BTreeSet::from_iter([
            scope.ident("cats"),
            scope.ident("dogs"),
            scope.ident("aardvarks"),
        ]));

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute =
            parse_quote!(#[cfg(any(feature = "aardvarks", feature = "cats", feature = "dogs"))]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_all_of_features() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let cfg = CfgFeature::AllOf(BTreeSet::from_iter([
            scope.ident("cats"),
            scope.ident("dogs"),
            scope.ident("aardvarks"),
        ]));

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute =
            parse_quote!(#[cfg(all(feature = "aardvarks", feature = "cats", feature = "dogs"))]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_own_and_used_by_feature() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let cfg = CfgFeature::OwnAndUsedBy {
            own: scope.ident("own"),
            used_by: BTreeSet::from_iter([scope.ident("a"), scope.ident("b")]),
        };

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute =
            parse_quote!(#[cfg(all(feature = "own", any(feature = "a", feature = "b")))]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_any_of_simplifies_single_feature() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let cfg = CfgFeature::any_of(BTreeSet::from_iter([scope.ident("pets")]));

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "pets")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_all_of_simplifies_single_feature() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let cfg = CfgFeature::all_of(BTreeSet::from_iter([scope.ident("pets")]));

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
    fn test_own_and_used_by_simplifies_single_used_by_to_all_of() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let own = scope.ident("own");
        let other = scope.ident("other");
        // `OwnedAndUsedBy` with one `used_by` feature should simplify to `AllOf`.
        let cfg = CfgFeature::own_and_used_by(own, BTreeSet::from_iter([other]));
        assert_eq!(cfg, CfgFeature::AllOf(BTreeSet::from_iter([other, own])),);
    }

    #[test]
    fn test_own_and_used_by_simplifies_empty_to_single() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let own = scope.ident("own");
        // `OwnedAndUsedBy` with no `used_by` features should simplify to `Single`.
        let cfg = CfgFeature::own_and_used_by(own, BTreeSet::new());
        assert_eq!(cfg, CfgFeature::Single(own));
    }

    #[test]
    fn test_own_and_used_by_simplifies_own_used_by_to_single() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let own = scope.ident("own");
        let other = scope.ident("other");
        let cfg = CfgFeature::own_and_used_by(own, BTreeSet::from_iter([own, other]));
        assert_eq!(cfg, CfgFeature::Single(own));
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let customer = graph.schema("Customer").unwrap();

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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let customer = graph.schema("Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &customer);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "customer")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_schema_type_with_own_resource_and_unnamed_deps() {
        // `Customer` (with `x-resourceId`) depends on `Address` (no `x-resourceId`).
        // `Customer` keeps its own feature gate; `Address` is ungated.
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        // `Customer` should be gated.
        let customer = graph.schema("Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &customer);
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "customer")]);
        assert_eq!(actual, expected);

        // `Address` should be ungated.
        let address = graph.schema("Address").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &address);
        assert_eq!(cfg, None);
    }

    #[test]
    fn test_for_schema_type_with_resource_id_named_default() {
        // `CodegenGraph` reserves `default` for the `default` Cargo feature,
        // so a resource named "default" won't collide with it.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Default:
                  type: object
                  x-resourceId: default
                  properties:
                    value:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let default_type = graph.schema("Default").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &default_type);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "default2")]);
        assert_eq!(actual, expected);
    }

    // MARK: Resource annotation-style

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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let customer = graph.schema("Customer").unwrap();
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let address = graph.schema("Address").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &address);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute =
            parse_quote!(#[cfg(any(feature = "customer", feature = "orders"))]);
        assert_eq!(actual, expected);
    }

    // MARK: Mixed styles

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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let customer = graph.schema("Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &customer);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute =
            parse_quote!(#[cfg(all(feature = "billing", feature = "customer"))]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_schema_type_with_own_and_same_used_by_uses_single_feature() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /defaults:
                get:
                  operationId: listDefaults
                  x-resource-name: default
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Default'
            components:
              schemas:
                Default:
                  type: object
                  x-resourceId: default
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let default_type = graph.schema("Default").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &default_type);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "default2")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_schema_type_with_own_same_and_other_used_by_uses_single_feature() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /customer:
                get:
                  operationId: getCustomer
                  x-resource-name: customer
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Customer'
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let customer = graph.schema("Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &customer);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "customer")]);
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let customer = graph.schema("Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &customer);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(
            #[cfg(all(feature = "customer", any(feature = "billing", feature = "orders")))]
        );
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_schema_type_with_own_used_by_and_unnamed_deps() {
        // `Customer` (with `x-resourceId`) is used by `getBilling`, and depends on `Address`
        // (no `x-resourceId`). `Address` is transitively used by the operation, so it inherits
        // the operation's feature gate. `Customer` keeps its compound feature gate.
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        // `Customer` keeps its compound feature gate (own + used by).
        let customer = graph.schema("Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &customer);
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute =
            parse_quote!(#[cfg(all(feature = "billing", feature = "customer"))]);
        assert_eq!(actual, expected);

        // `Address` has no `x-resourceId`, but is used by the operation transitively,
        // so it inherits the operation's feature gate.
        let address = graph.schema("Address").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &address);
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "billing")]);
        assert_eq!(actual, expected);
    }

    // MARK: Types without resources

    #[test]
    fn test_for_schema_type_unnamed_no_operations() {
        // Spec has a named resource (`Customer`), but `Simple` has
        // no `x-resourceId` and isn't used, so it shouldn't be gated.
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let simple = graph.schema("Simple").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &simple);

        // Types without a resource, and without operations that use them,
        // should be ungated.
        assert_eq!(cfg, None);
    }

    #[test]
    fn test_for_schema_type_used_by_unresourced_operation() {
        // `Status` is only used by `healthCheck`, which doesn't have an
        // `x-resource-name`. Even though the spec has other resourced
        // operations, `Status` should be ungated because its only consumer
        // is ungated.
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
                            $ref: '#/components/schemas/Customer'
              /health:
                get:
                  operationId: healthCheck
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Status'
            components:
              schemas:
                Customer:
                  type: object
                  properties:
                    name:
                      type: string
                Status:
                  type: object
                  properties:
                    ok:
                      type: boolean
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        // `Customer` is used by a resourced operation; it should be gated.
        let customer = graph.schema("Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &customer);
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "customer")]);
        assert_eq!(actual, expected);

        // `Status` is only used by an unresourced operation;
        // it should be ungated.
        let status = graph.schema("Status").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &status);
        assert_eq!(cfg, None);
    }

    #[test]
    fn test_for_schema_type_used_by_resourced_and_unresourced_operations() {
        // `Status` is used by both an unresourced operation and a resourced
        // operation. It must stay ungated for the unresourced operation.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /health:
                get:
                  operationId: getHealth
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Status'
              /billing/status:
                get:
                  operationId: getBillingStatus
                  x-resource-name: billing
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Status'
            components:
              schemas:
                Status:
                  type: object
                  properties:
                    ok:
                      type: boolean
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let status = graph.schema("Status").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &status);

        assert_eq!(cfg, None);
    }

    // MARK: Cycles with mixed resources

    #[test]
    fn test_for_schema_type_cycle_with_mixed_resources() {
        // Type A (resource `a`) -> Type B (no resource) -> Type C (resource `c`) -> Type A.
        // Since B is ungated (no `x-resourceId`), and transitively depends on A and C,
        // A and C should also be ungated.
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        // In a cycle involving B, all types become ungated, because
        // B depends on C, which depends on A, which depends on B.
        let a = graph.schema("A").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &a);
        assert_eq!(cfg, None);

        let b = graph.schema("B").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &b);
        assert_eq!(cfg, None);

        let c = graph.schema("C").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &c);
        assert_eq!(cfg, None);
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        // Each type uses just its own feature; Cargo feature dependencies
        // handle the transitive requirements.
        let a = graph.schema("A").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &a);
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "a")]);
        assert_eq!(actual, expected);

        let b = graph.schema("B").unwrap();
        let cfg = CfgFeature::for_schema_type(&graph, &b);
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "b")]);
        assert_eq!(actual, expected);

        let c = graph.schema("C").unwrap();
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let ops = graph.operations().collect_vec();
        let inlines = ops.iter().flat_map(|op| op.inlines()).collect_vec();
        assert!(!inlines.is_empty());

        // Shouldn't generate any feature gates for graph without named resources.
        let cfg = CfgFeature::for_inline_type(&graph, &inlines[0]);
        assert_eq!(cfg, None);
    }

    #[test]
    fn test_for_inline_type_used_by_own_resource_has_no_cfg() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /defaults:
                get:
                  operationId: listDefaults
                  x-resource-name: default
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            type: object
                            properties:
                              value:
                                $ref: '#/components/schemas/Default'
            components:
              schemas:
                Default:
                  type: object
                  x-resourceId: default
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let ops = graph.operations().collect_vec();
        let inlines = ops.iter().flat_map(|op| op.inlines()).collect_vec();
        assert!(!inlines.is_empty());

        let cfg = CfgFeature::for_inline_type(&graph, &inlines[0]);
        assert_eq!(cfg, None);
    }

    // MARK: Reduction

    #[test]
    fn test_for_operation_reduces_transitive_chain() {
        // A -> B -> C, each with a resource. The operation uses A.
        // Since A depends on B and C, only `feature = "a"` is needed.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /things:
                get:
                  operationId: getThings
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/A'
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
                    value:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().find(|o| o.id() == "getThings").unwrap();
        let cfg = CfgFeature::for_operation(&graph, &op);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "a")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_operation_reduces_cycle() {
        // A -> B -> C -> A, all with resources. The operation uses A.
        // Since they're all part of the same cycle, only the
        // lexicographically lowest feature should be present.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /things:
                get:
                  operationId: getThings
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/A'
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().find(|o| o.id() == "getThings").unwrap();
        let cfg = CfgFeature::for_operation(&graph, &op);

        // All three are in a cycle; the lowest feature name wins.
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "a")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_operation_keeps_independent_features() {
        // A and B are independent (no dependency between them), so
        // both features should be present.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /things:
                get:
                  operationId: getThings
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            type: object
                            properties:
                              a:
                                $ref: '#/components/schemas/A'
                              b:
                                $ref: '#/components/schemas/B'
            components:
              schemas:
                A:
                  type: object
                  x-resourceId: a
                  properties:
                    value:
                      type: string
                B:
                  type: object
                  x-resourceId: b
                  properties:
                    value:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().find(|o| o.id() == "getThings").unwrap();
        let cfg = CfgFeature::for_operation(&graph, &op);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(all(feature = "a", feature = "b"))]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_operation_reduces_partial_deps() {
        // A -> B, C independent; all three have resources. A depends on B, so
        // feature `b` is redundant, but `c` must be present.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /things:
                get:
                  operationId: getThings
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            type: object
                            properties:
                              a:
                                $ref: '#/components/schemas/A'
                              c:
                                $ref: '#/components/schemas/C'
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
                    value:
                      type: string
                C:
                  type: object
                  x-resourceId: c
                  properties:
                    value:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().find(|o| o.id() == "getThings").unwrap();
        let cfg = CfgFeature::for_operation(&graph, &op);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(all(feature = "a", feature = "c"))]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_operation_reduces_diamond_deps() {
        // A -> B, A -> C, B -> D, C -> D. The operation uses A.
        // Since A depends on B and C (which both depend on D), only `a` should remain.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /things:
                get:
                  operationId: getThings
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/A'
            components:
              schemas:
                A:
                  type: object
                  x-resourceId: a
                  properties:
                    b:
                      $ref: '#/components/schemas/B'
                    c:
                      $ref: '#/components/schemas/C'
                B:
                  type: object
                  x-resourceId: b
                  properties:
                    d:
                      $ref: '#/components/schemas/D'
                C:
                  type: object
                  x-resourceId: c
                  properties:
                    d:
                      $ref: '#/components/schemas/D'
                D:
                  type: object
                  x-resourceId: d
                  properties:
                    value:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph.operations().find(|o| o.id() == "getThings").unwrap();
        let cfg = CfgFeature::for_operation(&graph, &op);

        // A transitively implies B, C, and D; only `a` should remain.
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "a")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_operation_with_no_types() {
        // An operation with no parameters, request body, or response body.
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let op = graph
            .operations()
            .find(|o| o.id() == "healthCheck")
            .unwrap();
        let cfg = CfgFeature::for_operation(&graph, &op);

        // An operation with no type dependencies should have no feature gate.
        assert_eq!(cfg, None);
    }

    #[test]
    fn test_for_inline_type_reduces_transitive_features() {
        // Inline type inside a response, with A -> B -> C chain.
        // Only `a` should remain after reduction.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /things:
                get:
                  operationId: getThings
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            type: object
                            properties:
                              a:
                                $ref: '#/components/schemas/A'
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
                    value:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let ops = graph.operations().collect_vec();
        let inlines = ops.iter().flat_map(|op| op.inlines()).collect_vec();
        assert!(!inlines.is_empty());

        let cfg = CfgFeature::for_inline_type(&graph, &inlines[0]);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "a")]);
        assert_eq!(actual, expected);
    }
}
