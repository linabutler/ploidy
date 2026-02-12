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

use itertools::Itertools;
use ploidy_core::ir::{InlineIrTypeView, IrOperationView, SchemaIrTypeView, View};
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use super::naming::CargoFeature;

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
    pub fn for_schema_type(view: &SchemaIrTypeView<'_>) -> Option<Self> {
        // If this type has any transitive ungated root dependents,
        // it can't have a feature gate. An "ungated root" type
        // has no `x-resourceId`, _and_ isn't used by any operation with
        // `x-resource-name`. Because it's ungated, none of its
        // transitive dependencies can be gated, either.
        let has_ungated_root_dependent = view
            .dependents()
            .filter_map(|v| v.into_schema().ok())
            .any(|s| s.resource().is_none() && s.used_by().all(|op| op.resource().is_none()));
        if has_ungated_root_dependent {
            return None;
        }

        // Compute all the operations with resources that use this type.
        let used_by_features: BTreeSet<_> = view
            .used_by()
            .filter_map(|op| op.resource())
            .map(CargoFeature::from_name)
            .collect();

        match (view.resource(), used_by_features.is_empty()) {
            // Type has own resource, _and_ is used by operations.
            (Some(name), false) => {
                Self::own_and_used_by(CargoFeature::from_name(name), used_by_features)
            }
            // Type has own resource only (Stripe-style; no resources on operations).
            (Some(name), true) => Some(Self::Single(CargoFeature::from_name(name))),
            // Type has no own resource, but is used by operations
            // (resource annotation-style; no resources on types).
            (None, false) => Self::any_of(used_by_features),
            // No resource name; not used by any operation.
            (None, true) => None,
        }
    }

    /// Builds a `#[cfg(...)]` attribute for an inline type.
    pub fn for_inline_type(view: &InlineIrTypeView<'_>) -> Option<Self> {
        // Inline types depended on by ungated root types can't be gated, either.
        // See `for_schema_type` for the definition of an "ungated root".
        let has_ungated_root_dependent = view
            .dependents()
            .filter_map(|v| v.into_schema().ok())
            .any(|s| s.resource().is_none() && s.used_by().all(|op| op.resource().is_none()));
        if has_ungated_root_dependent {
            return None;
        }

        let used_by_features: BTreeSet<_> = view
            .used_by()
            .filter_map(|op| op.resource())
            .map(CargoFeature::from_name)
            .collect();

        if used_by_features.is_empty() {
            // No operations use this inline type directly, so use its
            // transitive dependencies for gating. Filter out dependencies
            // without a resource name, because these aren't gated.
            let pairs = view
                .dependencies()
                .filter_map(|v| v.into_schema().ok())
                .filter_map(|ty| ty.resource().map(|r| (CargoFeature::from_name(r), ty)))
                .collect_vec();
            Self::all_of(reduce_transitive_features(&pairs))
        } else {
            // Some operations use this inline type; use those operations for gating.
            Self::any_of(used_by_features)
        }
    }

    /// Builds a `#[cfg(...)]` attribute for a client method.
    pub fn for_operation(view: &IrOperationView<'_>) -> Option<Self> {
        // Collect all features from transitive dependencies, then
        // reduce redundant features.
        let pairs = view
            .dependencies()
            .filter_map(|v| v.into_schema().ok())
            .filter_map(|ty| ty.resource().map(|r| (CargoFeature::from_name(r), ty)))
            .collect_vec();

        Self::all_of(reduce_transitive_features(&pairs))
    }

    /// Builds a `#[cfg(...)]` attribute for a resource `mod` declaration in a
    /// [`CodegenClientModule`](super::client::CodegenClientModule).
    pub fn for_resource_module(feature: &CargoFeature) -> Option<Self> {
        if matches!(feature, CargoFeature::Default) {
            // Modules associated with the default resource shouldn't be gated,
            // because that would make them unreachable under
            // `--no-default-features`.
            return None;
        }
        Some(Self::Single(feature.clone()))
    }

    /// Builds a `#[cfg(any(...))]` predicate, simplifying if possible.
    fn any_of(mut features: BTreeSet<CargoFeature>) -> Option<Self> {
        if features.contains(&CargoFeature::Default) {
            // Items associated with the default resource shouldn't be gated.
            return None;
        }
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
        if features.contains(&CargoFeature::Default) {
            // Items associated with the default resource shouldn't be gated.
            return None;
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
    fn own_and_used_by(own: CargoFeature, mut used_by: BTreeSet<CargoFeature>) -> Option<Self> {
        if matches!(own, CargoFeature::Default) || used_by.contains(&CargoFeature::Default) {
            // Items associated with the default resource shouldn't be gated.
            return None;
        }
        let Some(first) = used_by.pop_first() else {
            // No `used_by`; simplify to `own`.
            return Some(Self::Single(own));
        };
        Some(if used_by.is_empty() {
            // Simplify `all(own, any(first))` to `all(own, first)`.
            Self::AllOf(BTreeSet::from_iter([own, first]))
        } else {
            // Keep `all(own, any(first, used_by...))`.
            used_by.insert(first);
            Self::OwnAndUsedBy { own, used_by }
        })
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

/// Reduces a set of (feature, type) pairs by removing all the features
/// that are transitively implied by other features. If feature A's type
/// depends on feature B's type, then enabling A in `Cargo.toml` already
/// enables B, so B is redundant.
fn reduce_transitive_features(
    pairs: &[(CargoFeature, SchemaIrTypeView<'_>)],
) -> BTreeSet<CargoFeature> {
    pairs
        .iter()
        .enumerate()
        .filter(|&(i, (feature, ty))| {
            // Keep this `feature` unless some `other_ty` depends on it,
            // meaning that `other_feature` already enables this `feature`.
            let mut others = pairs.iter().enumerate().filter(|&(j, _)| i != j);
            !others.any(|(_, (other_feature, other_ty))| {
                // Does the other type depend on this type?
                if !other_ty.depends_on(ty) {
                    return false;
                }
                // Do the types form a cycle, and depend on each other?
                // If so, the lexicographically lower feature name
                // breaks the tie.
                if ty.depends_on(other_ty) {
                    return other_feature < feature;
                }
                true
            })
        })
        .map(|(_, (feature, _))| feature.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use itertools::Itertools;
    use ploidy_core::{ir::Ir, parse::Document};
    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    use crate::graph::CodegenGraph;

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
    fn test_any_of_returns_none_when_contains_default() {
        // Items associated with the default resource shouldn't be gated.
        let cfg = CfgFeature::any_of(BTreeSet::from_iter([
            CargoFeature::from_name("customer"),
            CargoFeature::Default,
            CargoFeature::from_name("billing"),
        ]));
        assert_eq!(cfg, None);
    }

    #[test]
    fn test_all_of_returns_none_for_empty() {
        let cfg = CfgFeature::all_of(BTreeSet::new());
        assert_eq!(cfg, None);
    }

    #[test]
    fn test_all_of_returns_none_when_contains_default() {
        // Items associated with the default resource shouldn't be gated.
        let cfg = CfgFeature::all_of(BTreeSet::from_iter([
            CargoFeature::from_name("customer"),
            CargoFeature::Default,
            CargoFeature::from_name("billing"),
        ]));
        assert_eq!(cfg, None);
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
            Some(CfgFeature::AllOf(BTreeSet::from_iter([
                CargoFeature::from_name("other"),
                CargoFeature::from_name("own"),
            ])))
        );
    }

    #[test]
    fn test_own_and_used_by_simplifies_empty_to_single() {
        // `OwnedAndUsedBy` with no `used_by` features should simplify to `Single`.
        let cfg = CfgFeature::own_and_used_by(CargoFeature::from_name("own"), BTreeSet::new());
        assert_eq!(
            cfg,
            Some(CfgFeature::Single(CargoFeature::from_name("own")))
        );
    }

    #[test]
    fn test_own_and_used_by_returns_none_when_own_is_default() {
        // Items associated with the default resource shouldn't be gated.
        let cfg = CfgFeature::own_and_used_by(
            CargoFeature::Default,
            BTreeSet::from_iter([CargoFeature::from_name("other")]),
        );
        assert_eq!(cfg, None);
    }

    #[test]
    fn test_own_and_used_by_returns_none_when_used_by_contains_default() {
        // Items associated with the default resource shouldn't be gated.
        let cfg = CfgFeature::own_and_used_by(
            CargoFeature::from_name("own"),
            BTreeSet::from_iter([CargoFeature::from_name("other"), CargoFeature::Default]),
        );
        assert_eq!(cfg, None);
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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let customer = graph.schemas().find(|s| s.name() == "Customer").unwrap();

        // Shouldn't generate any feature gates for graph without named resources.
        let cfg = CfgFeature::for_schema_type(&customer);
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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let customer = graph.schemas().find(|s| s.name() == "Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&customer);

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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        // `Customer` should be gated.
        let customer = graph.schemas().find(|s| s.name() == "Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&customer);
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "customer")]);
        assert_eq!(actual, expected);

        // `Address` should be ungated.
        let address = graph.schemas().find(|s| s.name() == "Address").unwrap();
        let cfg = CfgFeature::for_schema_type(&address);
        assert_eq!(cfg, None);
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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let customer = graph.schemas().find(|s| s.name() == "Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&customer);

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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let address = graph.schemas().find(|s| s.name() == "Address").unwrap();
        let cfg = CfgFeature::for_schema_type(&address);

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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let customer = graph.schemas().find(|s| s.name() == "Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&customer);

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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let customer = graph.schemas().find(|s| s.name() == "Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&customer);

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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        // `Customer` keeps its compound feature gate (own + used by).
        let customer = graph.schemas().find(|s| s.name() == "Customer").unwrap();
        let cfg = CfgFeature::for_schema_type(&customer);
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute =
            parse_quote!(#[cfg(all(feature = "billing", feature = "customer"))]);
        assert_eq!(actual, expected);

        // `Address` has no `x-resourceId`, but is used by the operation transitively,
        // so it inherits the operation's feature gate.
        let address = graph.schemas().find(|s| s.name() == "Address").unwrap();
        let cfg = CfgFeature::for_schema_type(&address);
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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let simple = graph.schemas().find(|s| s.name() == "Simple").unwrap();
        let cfg = CfgFeature::for_schema_type(&simple);

        // Types without a resource, and without operations that use them,
        // should be ungated.
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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        // In a cycle involving B, all types become ungated, because
        // B depends on C, which depends on A, which depends on B.
        let a = graph.schemas().find(|s| s.name() == "A").unwrap();
        let cfg = CfgFeature::for_schema_type(&a);
        assert_eq!(cfg, None);

        let b = graph.schemas().find(|s| s.name() == "B").unwrap();
        let cfg = CfgFeature::for_schema_type(&b);
        assert_eq!(cfg, None);

        let c = graph.schemas().find(|s| s.name() == "C").unwrap();
        let cfg = CfgFeature::for_schema_type(&c);
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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        // Each type uses just its own feature; Cargo feature dependencies
        // handle the transitive requirements.
        let a = graph.schemas().find(|s| s.name() == "A").unwrap();
        let cfg = CfgFeature::for_schema_type(&a);
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "a")]);
        assert_eq!(actual, expected);

        let b = graph.schemas().find(|s| s.name() == "B").unwrap();
        let cfg = CfgFeature::for_schema_type(&b);
        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "b")]);
        assert_eq!(actual, expected);

        let c = graph.schemas().find(|s| s.name() == "C").unwrap();
        let cfg = CfgFeature::for_schema_type(&c);
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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let ops = graph.operations().collect_vec();
        let inlines = ops.iter().flat_map(|op| op.inlines()).collect_vec();
        assert!(!inlines.is_empty());

        // Shouldn't generate any feature gates for graph without named resources.
        let cfg = CfgFeature::for_inline_type(&inlines[0]);
        assert_eq!(cfg, None);
    }

    // MARK: Resource modules

    #[test]
    fn test_for_resource_module_with_named_feature() {
        let cfg = CfgFeature::for_resource_module(&CargoFeature::from_name("pets"));

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "pets")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_for_resource_module_skips_default_feature() {
        // The `default` feature is built in to Cargo. Gating a module
        // behind it makes operations unreachable when individual features
        // are enabled via `--no-default-features --features foo`.
        let cfg = CfgFeature::for_resource_module(&CargoFeature::Default);
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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let op = graph.operations().find(|o| o.id() == "getThings").unwrap();
        let cfg = CfgFeature::for_operation(&op);

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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let op = graph.operations().find(|o| o.id() == "getThings").unwrap();
        let cfg = CfgFeature::for_operation(&op);

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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let op = graph.operations().find(|o| o.id() == "getThings").unwrap();
        let cfg = CfgFeature::for_operation(&op);

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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let op = graph.operations().find(|o| o.id() == "getThings").unwrap();
        let cfg = CfgFeature::for_operation(&op);

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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let op = graph.operations().find(|o| o.id() == "getThings").unwrap();
        let cfg = CfgFeature::for_operation(&op);

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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let op = graph
            .operations()
            .find(|o| o.id() == "healthCheck")
            .unwrap();
        let cfg = CfgFeature::for_operation(&op);

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

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let ops = graph.operations().collect_vec();
        let inlines = ops.iter().flat_map(|op| op.inlines()).collect_vec();
        assert!(!inlines.is_empty());

        let cfg = CfgFeature::for_inline_type(&inlines[0]);

        let actual: syn::Attribute = parse_quote!(#cfg);
        let expected: syn::Attribute = parse_quote!(#[cfg(feature = "a")]);
        assert_eq!(actual, expected);
    }
}
