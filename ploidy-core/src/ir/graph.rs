use std::{
    any::{Any, TypeId},
    collections::VecDeque,
    fmt::Debug,
};

use atomic_refcell::AtomicRefCell;
use enum_map::{Enum, EnumMap, enum_map};
use fixedbitset::FixedBitSet;
use itertools::Itertools;
use petgraph::{
    Direction,
    adj::UnweightedList,
    algo::{TarjanScc, tred},
    data::Build,
    graph::{DiGraph, NodeIndex},
    stable_graph::StableDiGraph,
    visit::{DfsPostOrder, EdgeFiltered, EdgeRef, IntoNeighbors, VisitMap, Visitable},
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{arena::Arena, parse::Info};

use super::{
    spec::{ResolvedSpecType, Spec},
    types::{
        GraphInlineType, GraphOperation, GraphSchemaType, GraphStruct, GraphTagged,
        GraphTaggedVariant, GraphType, InlineTypePath, InlineTypePathRoot, InlineTypePathSegment,
        SchemaTypeInfo, SpecInlineType, SpecSchemaType, SpecType, SpecUntaggedVariant,
        StructFieldName, mapper::TypeMapper,
    },
    views::{operation::OperationView, primitive::PrimitiveView, schema::SchemaTypeView},
};

/// The mutable, sparse graph used for transformations.
type RawDiGraph<'a> = StableDiGraph<GraphType<'a>, EdgeKind, usize>;

/// The immutable, dense graph used for code generation.
type CookedDiGraph<'a> = DiGraph<GraphType<'a>, EdgeKind, usize>;

/// A mutable intermediate dependency graph of all the types in a [`Spec`],
/// backed by a sparse [`StableDiGraph`].
///
/// This graph is constructed directly from a [`Spec`], and represents
/// type relationships as they exist in the spec. Transformations like
/// [`inline_tagged_variants`][Self::inline_tagged_variants] rewrite this graph
/// in place.
///
/// After applying all transformations, call [`cook`][Self::cook] to
/// turn this graph into a [`CookedGraph`] that's ready for code generation.
#[derive(Debug)]
pub struct RawGraph<'a> {
    arena: &'a Arena,
    spec: &'a Spec<'a>,
    graph: RawDiGraph<'a>,
    ops: &'a [&'a GraphOperation<'a>],
}

impl<'a> RawGraph<'a> {
    pub fn new(arena: &'a Arena, spec: &'a Spec<'a>) -> Self {
        // All roots (named schemas, parameters, request and response bodies),
        // and all the types within them (inline schemas and primitives).
        let tys = SpecTypeVisitor::new(
            spec.schemas
                .values()
                .chain(spec.operations.iter().flat_map(|op| op.types().copied())),
        );

        // Build the nodes and edges.
        let mut indices = FxHashMap::default();
        let mut schemas = FxHashMap::default();
        let mut nodes = vec![];
        let mut edges = vec![];
        for (parent, kind, child) in tys {
            use std::collections::hash_map::Entry;
            let source = spec.resolve(child);
            let &mut to = match indices.entry(source) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    // We might see the same schema multiple times if it's
                    // referenced multiple times in the spec. Only add
                    // a new node for the schema if we haven't seen it before.
                    let index = NodeIndex::new(nodes.len());
                    nodes.push(*entry.key());
                    entry.insert(index)
                }
            };
            // Track schema names for later lookup.
            if let ResolvedSpecType::Schema(ty) = source {
                schemas.entry(ty.name()).or_insert(to);
            }
            if let Some(parent) = parent {
                let destination = spec.resolve(parent);
                let &mut from = match indices.entry(destination) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => {
                        let index = NodeIndex::new(nodes.len());
                        nodes.push(*entry.key());
                        entry.insert(index)
                    }
                };
                if let ResolvedSpecType::Schema(ty) = destination {
                    schemas.entry(ty.name()).or_insert(from);
                }
                edges.push((from, to, kind));
            }
        }

        // Construct a graph from the nodes and edges,
        // mapping schema type references to graph indices.
        let mut graph = RawDiGraph::with_capacity(nodes.len(), edges.len());
        let mapper = TypeMapper::new(arena, |ty: &SpecType<'_>| match ty {
            SpecType::Schema(s) => indices[&ResolvedSpecType::Schema(s)],
            SpecType::Inline(i) => indices[&ResolvedSpecType::Inline(i)],
            SpecType::Ref(r) => schemas[&*r.name()],
        });
        for node in nodes {
            let mapped = match node {
                ResolvedSpecType::Schema(ty) => GraphType::Schema(mapper.schema(ty)),
                ResolvedSpecType::Inline(ty) => GraphType::Inline(mapper.inline(ty)),
            };
            let index = graph.add_node(mapped);
            debug_assert_eq!(index, indices[&node]);
        }
        for (from, to, kind) in edges {
            graph.add_edge(from, to, kind);
        }

        // Map schema type references in operations.
        let ops = arena.alloc_slice_exact(spec.operations.iter().map(|op| mapper.operation(op)));

        Self {
            arena,
            spec,
            graph,
            ops,
        }
    }

    /// Inlines schema types used as variants of multiple tagged unions
    /// with different tags.
    ///
    /// In OpenAPI's model of tagged unions, the tag always references a field
    /// that's defined on each struct variant. This model works well for Python
    /// and TypeScript, but not Rust; Serde doesn't allow struct variants to
    /// declare fields with the same name as the tag. The Rust generator
    /// excludes tag fields when generating structs, but this introduces a
    /// new problem: a struct can't appear a variant of multiple unions
    /// with different tags [^1].
    ///
    /// This transformation finds and inlines these structs, so that
    /// the Rust generator can safely omit their tag fields.
    ///
    /// [^1]: If struct A has fields `foo` and `bar`, A is a variant of
    /// tagged unions C and D, C's tag is `foo`, and D's tag is `bar`...
    /// only `foo` should be excluded when A is used in C, and only `bar`
    /// should be excluded when A is used in D; but this can't be modeled
    /// in Serde without splitting A into two distinct types.
    pub fn inline_tagged_variants(&mut self) -> &mut Self {
        struct TaggedPlan<'a> {
            tagged_index: NodeIndex<usize>,
            info: SchemaTypeInfo<'a>,
            tagged: GraphTagged<'a>,
            inlines: Vec<VariantInline<'a>>,
        }
        struct VariantInline<'a> {
            node: GraphType<'a>,
            variant_index: NodeIndex<usize>,
            parent_indices: &'a [NodeIndex<usize>],
            name: &'a str,
            aliases: &'a [&'a str],
        }

        // Compute the set of types used (as query params, request and response
        // bodies, etc.) by operations. Operations don't create graph edges,
        // but still need to be considered when deciding whether to inline a
        // struct variant. Otherwise, a struct that's used by same-tag unions
        // _and_ an operation wouldn't be inlined, causing the Rust generator to
        // incorrectly exclude the tag field from the struct.
        let used_by_ops: FixedBitSet = self
            .ops
            .iter()
            .flat_map(|op| op.types())
            .map(|node| node.index())
            .collect();

        // Collect all inlining decisions before mutating the graph,
        // so that we can check inlinability per variant.
        let plans = self
            .graph
            .node_indices()
            .filter_map(|index| {
                let GraphType::Schema(GraphSchemaType::Tagged(info, tagged)) = self.graph[index]
                else {
                    return None;
                };
                let mut inlines = vec![];

                for variant in tagged.variants {
                    let variant_index = variant.ty;
                    let GraphType::Schema(GraphSchemaType::Struct(variant_info, variant_struct)) =
                        self.graph[variant_index]
                    else {
                        continue;
                    };

                    // A struct variant only needs inlining if it has multiple
                    // distinct uses. Skip if (1) no operation uses the struct,
                    // _and_ (2) every incoming edge is from a tagged union with
                    // the same tag and fields. If both hold, all uses agree, so
                    // the struct can be used directly without inlining.
                    if !used_by_ops.contains(variant_index.index()) {
                        let Some(first) = ({
                            self.graph
                                .neighbors_directed(variant_index, Direction::Incoming)
                                .find_map(|neighbor| match self.graph[neighbor] {
                                    GraphType::Schema(GraphSchemaType::Tagged(_, t)) => Some(t),
                                    _ => None,
                                })
                        }) else {
                            continue;
                        };
                        // Check that all the variant's inbound edges are from
                        // tagged unions, and that all their tags and fields
                        // match the first union we found.
                        let all_agree = self
                            .graph
                            .neighbors_directed(variant_index, Direction::Incoming)
                            .all(|neighbor| {
                                matches!(
                                    self.graph[neighbor],
                                    GraphType::Schema(GraphSchemaType::Tagged(_, t))
                                        if t.tag == first.tag && t.fields == first.fields,
                                )
                            });
                        if all_agree {
                            continue;
                        }
                    }

                    // Skip inlining when the inline copy would be
                    // identical to the original. This happens when
                    // the variant doesn't declare the tag as a field _and_
                    // either (a) the union has no own fields, or
                    // (b) the variant already inherits from this union,
                    // so its fields are already reachable.
                    let (has_tag_field, already_inherits) = {
                        let inherits = EdgeFiltered::from_fn(&self.graph, |e| {
                            matches!(e.weight(), EdgeKind::Inherits)
                        });
                        let mut dfs = DfsPostOrder::new(&inherits, variant_index);
                        let mut has_tag_field = false;
                        let mut already_inherits = false;
                        while let Some(ancestor) = dfs.next(&inherits)
                            && !(has_tag_field && already_inherits)
                        {
                            already_inherits |= ancestor == index;
                            has_tag_field |= match self.graph[ancestor] {
                                GraphType::Schema(GraphSchemaType::Struct(_, s))
                                | GraphType::Inline(GraphInlineType::Struct(_, s)) => {
                                    // Check own and inherited fields; OpenAPI 3.2
                                    // clarifies that the tag can be inherited.
                                    s.fields.iter().any(|f| {
                                        matches!(
                                            f.name,
                                            StructFieldName::Name(n) if n == tagged.tag,
                                        )
                                    })
                                }
                                _ => false,
                            };
                        }
                        (has_tag_field, already_inherits)
                    };
                    if !has_tag_field && (tagged.fields.is_empty() || already_inherits) {
                        continue;
                    }

                    // Build our new inline type, with the same attributes
                    // as the schema type, but a distinct inline type path.
                    // The inline struct is a clone of the original, plus
                    // an inheritance edge to the tagged union for its fields.
                    let parents = self.arena.alloc_slice(itertools::chain!(
                        variant_struct.parents.iter().copied(),
                        std::iter::once(index),
                    ));
                    let node = GraphType::Inline(GraphInlineType::Struct(
                        InlineTypePath {
                            root: InlineTypePathRoot::Type(info.name),
                            segments: self.arena.alloc_slice_copy(&[
                                InlineTypePathSegment::TaggedVariant(variant_info.name),
                            ]),
                        },
                        GraphStruct {
                            description: variant_struct.description,
                            fields: variant_struct.fields,
                            parents,
                        },
                    ));

                    inlines.push(VariantInline {
                        node,
                        variant_index,
                        parent_indices: parents,
                        name: variant.name,
                        aliases: variant.aliases,
                    });
                }
                if inlines.is_empty() {
                    return None;
                }

                Some(TaggedPlan {
                    tagged_index: index,
                    info,
                    tagged,
                    inlines,
                })
            })
            .collect_vec();

        // Apply the plans to the graph.
        for plan in plans {
            let mut new_variants = FxHashMap::default();

            // Add nodes and edges for the inline types.
            for entry in &plan.inlines {
                let node_index = self.graph.add_node(entry.node);

                // Reference the original variant so that the inline
                // inherits the original's transitive dependencies and
                // SCC membership, but not its inline subtree.
                self.graph
                    .add_edge(node_index, entry.variant_index, EdgeKind::Reference);

                // Add inheritance edges back to the inline's parents.
                for &parent_index in entry.parent_indices {
                    self.graph
                        .add_edge(node_index, parent_index, EdgeKind::Inherits);
                }

                new_variants.insert(
                    entry.variant_index,
                    GraphTaggedVariant {
                        name: entry.name,
                        aliases: entry.aliases,
                        ty: node_index,
                    },
                );
            }

            // Retarget reference edges from the tagged union to point to
            // the new inline variants. We only update edges targeting a
            // replaced variant; other edges stay.
            let edges_to_retarget = self
                .graph
                .edges_directed(plan.tagged_index, Direction::Outgoing)
                .filter(|e| {
                    matches!(e.weight(), EdgeKind::Reference)
                        && new_variants.contains_key(&e.target())
                })
                .map(|e| (e.id(), new_variants[&e.target()].ty))
                .collect_vec();
            for (edge_id, new_target) in edges_to_retarget {
                self.graph.remove_edge(edge_id);
                self.graph
                    .add_edge(plan.tagged_index, new_target, EdgeKind::Reference);
            }

            // Replace the node for the tagged union itself.
            let modified_tagged = GraphTagged {
                description: plan.tagged.description,
                tag: plan.tagged.tag,
                variants: self.arena.alloc_slice_exact(
                    plan.tagged
                        .variants
                        .iter()
                        .map(|&v| new_variants.get(&v.ty).copied().unwrap_or(v)),
                ),
                fields: plan.tagged.fields,
            };
            self.graph[plan.tagged_index] =
                GraphType::Schema(GraphSchemaType::Tagged(plan.info, modified_tagged));
        }

        self
    }

    /// Builds an immutable [`CookedGraph`] from this mutable raw graph.
    #[inline]
    pub fn cook(&self) -> CookedGraph<'a> {
        CookedGraph::new(self)
    }
}

/// The final dependency graph of all the types in a [`Spec`],
/// backed by a dense [`DiGraph`].
///
/// This graph has all transformations applied, and is ready for
/// code generation.
#[derive(Debug)]
pub struct CookedGraph<'a> {
    pub(super) graph: CookedDiGraph<'a>,
    info: &'a Info,
    ops: &'a [&'a GraphOperation<'a>],
    /// Additional metadata for each node.
    pub(super) metadata: CookedGraphMetadata<'a>,
}

impl<'a> CookedGraph<'a> {
    fn new(raw: &RawGraph<'a>) -> Self {
        // Assign a cooked node index to each raw node index.
        let indices: FxHashMap<_, _> = raw
            .graph
            .node_indices()
            .enumerate()
            .map(|(cooked, raw)| (raw, NodeIndex::new(cooked)))
            .collect();

        // Map sparse graph indices to dense cooked indices.
        let mapper = TypeMapper::new(raw.arena, |index| indices[&index]);

        // Build a dense graph.
        let mut graph = CookedDiGraph::with_capacity(indices.len(), raw.graph.edge_count());
        for index in raw.graph.node_indices() {
            let node = raw.graph[index];
            let mapped = match node {
                GraphType::Schema(ty) => GraphType::Schema(mapper.schema(&ty)),
                GraphType::Inline(ty) => GraphType::Inline(mapper.inline(&ty)),
            };
            let cooked = graph.add_node(mapped);
            debug_assert_eq!(indices[&index], cooked);
        }

        // Add edges, preserving original insertion order.
        for index in raw.graph.node_indices() {
            let from = indices[&index];
            // `RawDiGraph::edges` yields edges in reverse insertion
            // order; collect and reverse to preserve the original order.
            let edges = raw
                .graph
                .edges(index)
                .map(|e| (indices[&e.target()], *e.weight()))
                .collect_vec();
            for (to, kind) in edges.into_iter().rev() {
                graph.add_edge(from, to, kind);
            }
        }

        let sccs = TopoSccs::new(&graph);

        let (metadata, ops) = {
            let mut metadata = CookedGraphMetadata {
                scc_indices: {
                    // Precompute SCC indices, using just the reference edges.
                    // Inheritance edges don't contribute to cycles.
                    let refs = EdgeFiltered::from_fn(&graph, |e| {
                        matches!(e.weight(), EdgeKind::Reference)
                    });
                    let mut scc = TarjanScc::new();
                    scc.run(&refs, |_| ());
                    graph
                        .node_indices()
                        .map(|node| scc.node_component_index(&refs, node))
                        .collect()
                },
                // `GraphNodeMeta` can't implement `Clone` because it contains
                // an `AtomicRefCell`, so we use this idiom instead of `vec!`.
                schemas: std::iter::repeat_with(GraphNodeMeta::default)
                    .take(graph.node_count())
                    .collect(),
                operations: FxHashMap::default(),
            };

            // Remap schema type references in operations.
            let ops: &_ = raw
                .arena
                .alloc_slice_exact(raw.ops.iter().map(|&op| mapper.operation(op)));

            // Precompute the set of type indices that each operation
            // references directly.
            for &&op in ops {
                metadata.operations.entry(op).or_default().types =
                    op.types().map(|node| node.index()).collect();
            }

            // Forward propagation: for each type, compute all the types
            // that it depends on, directly and transitively.
            {
                // Condense each of the original graph's strongly connected components
                // into a single node, forming a DAG.
                let condensation = sccs.condensation();

                // Compute the transitive closure; discard the reduction.
                let (_, closure) = tred::dag_transitive_reduction_closure(&condensation);

                // Compute dependencies between SCCs.
                let mut scc_deps =
                    vec![FixedBitSet::with_capacity(graph.node_count()); sccs.scc_count()];
                for (scc, deps) in scc_deps.iter_mut().enumerate() {
                    // Include the SCC itself, so that cycle members appear
                    // in each other's dependencies; and its
                    // transitive neighbors.
                    deps.extend(
                        std::iter::once(scc)
                            .chain(closure.neighbors(scc))
                            .flat_map(|scc| sccs.sccs[scc].ones()),
                    );
                }

                // Expand SCC dependencies to node dependencies, and
                // transpose dependencies to build dependents.
                let mut node_dependents =
                    vec![FixedBitSet::with_capacity(graph.node_count()); graph.node_count()];
                for node in graph.node_indices() {
                    let mut deps = scc_deps[sccs.topo_index(node)].clone();
                    deps.remove(node.index()); // We don't depend on ourselves.
                    for dep in deps.ones() {
                        node_dependents[dep].insert(node.index());
                    }
                    metadata.schemas[node.index()].dependencies = deps;
                }
                for (index, dependents) in node_dependents.into_iter().enumerate() {
                    metadata.schemas[index].dependents = dependents;
                }
            }

            // Backward propagation: propagate each operation to all the
            // types that it uses, directly and transitively.
            for &&op in ops {
                let meta = &metadata.operations[&op];

                // Collect all the types that this operation depends on.
                let mut transitive_deps = FixedBitSet::with_capacity(graph.node_count());
                for node in meta.types.ones() {
                    transitive_deps.insert(node);
                    transitive_deps.union_with(&metadata.schemas[node].dependencies);
                }

                // Mark each type as being used by this operation.
                for node in transitive_deps.ones() {
                    metadata.schemas[node].used_by.insert(op);
                }
            }

            (metadata, ops)
        };

        Self {
            graph,
            info: raw.spec.info,
            ops,
            metadata,
        }
    }

    /// Returns [`Info`] from the [`Document`][crate::parse::Document]
    /// used to build this graph.
    #[inline]
    pub fn info(&self) -> &'a Info {
        self.info
    }

    /// Returns an iterator over all the named schemas in this graph.
    #[inline]
    pub fn schemas(&self) -> impl Iterator<Item = SchemaTypeView<'_>> {
        self.graph
            .node_indices()
            .filter_map(|index| match self.graph[index] {
                GraphType::Schema(ty) => Some(SchemaTypeView::new(self, index, ty)),
                _ => None,
            })
    }

    /// Returns an iterator over all primitive type nodes in this graph.
    #[inline]
    pub fn primitives(&self) -> impl Iterator<Item = PrimitiveView<'_>> {
        self.graph
            .node_indices()
            .filter_map(|index| match self.graph[index] {
                GraphType::Schema(GraphSchemaType::Primitive(_, p))
                | GraphType::Inline(GraphInlineType::Primitive(_, p)) => {
                    Some(PrimitiveView::new(self, index, p))
                }
                _ => None,
            })
    }

    /// Returns an iterator over all the operations in this graph.
    #[inline]
    pub fn operations(&self) -> impl Iterator<Item = OperationView<'_>> {
        self.ops.iter().map(move |&op| OperationView::new(self, op))
    }
}

/// An edge between two types in the type graph.
#[derive(Clone, Copy, Debug, Enum, Eq, Hash, PartialEq)]
pub enum EdgeKind {
    /// The source type contains or references the target type.
    Reference,
    /// The source type inherits from the target type.
    Inherits,
}

/// Precomputed metadata for schema types and operations in the graph.
#[derive(Debug, Default)]
pub(super) struct CookedGraphMetadata<'a> {
    /// Maps each node index to its strongly connected component index.
    /// Nodes in the same SCC form a cycle.
    pub scc_indices: Vec<usize>,
    pub schemas: Vec<GraphNodeMeta<'a>>,
    pub operations: FxHashMap<GraphOperation<'a>, GraphOperationMeta>,
}

/// Precomputed metadata for an operation that references
/// types in the graph.
#[derive(Debug, Default)]
pub(super) struct GraphOperationMeta {
    /// Indices of all the types that this operation directly depends on:
    /// parameters, request body, and response body.
    pub types: FixedBitSet,
}

/// Precomputed metadata for a schema type in the graph.
#[derive(Default)]
pub(super) struct GraphNodeMeta<'a> {
    /// Operations that use this type.
    pub used_by: FxHashSet<GraphOperation<'a>>,
    /// Indices of other types that this type transitively depends on.
    pub dependencies: FixedBitSet,
    /// Indices of other types that transitively depend on this type.
    pub dependents: FixedBitSet,
    /// Opaque extended data for this type.
    pub extensions: AtomicRefCell<ExtensionMap>,
}

impl Debug for GraphNodeMeta<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphNodeMeta")
            .field("used_by", &self.used_by)
            .field("dependencies", &self.dependencies)
            .field("dependents", &self.dependents)
            .finish_non_exhaustive()
    }
}

/// Visits all the types and references contained within a [`SpecType`].
#[derive(Debug)]
struct SpecTypeVisitor<'a> {
    stack: Vec<(Option<&'a SpecType<'a>>, EdgeKind, &'a SpecType<'a>)>,
}

impl<'a> SpecTypeVisitor<'a> {
    /// Creates a visitor with `roots` on the stack of types to visit.
    #[inline]
    fn new(roots: impl Iterator<Item = &'a SpecType<'a>>) -> Self {
        let mut stack = roots
            .map(|root| (None, EdgeKind::Reference, root))
            .collect_vec();
        stack.reverse();
        Self { stack }
    }
}

impl<'a> Iterator for SpecTypeVisitor<'a> {
    type Item = (Option<&'a SpecType<'a>>, EdgeKind, &'a SpecType<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        let (parent, kind, top) = self.stack.pop()?;
        match top {
            SpecType::Schema(SpecSchemaType::Struct(_, ty))
            | SpecType::Inline(SpecInlineType::Struct(_, ty)) => {
                self.stack.extend(
                    itertools::chain!(
                        ty.fields
                            .iter()
                            .map(|field| (EdgeKind::Reference, field.ty)),
                        ty.parents
                            .iter()
                            .map(|parent| (EdgeKind::Inherits, *parent)),
                    )
                    .map(|(kind, ty)| (Some(top), kind, ty))
                    .rev(),
                );
            }
            SpecType::Schema(SpecSchemaType::Untagged(_, ty))
            | SpecType::Inline(SpecInlineType::Untagged(_, ty)) => {
                self.stack.extend(
                    itertools::chain!(
                        ty.fields
                            .iter()
                            .map(|field| (EdgeKind::Reference, field.ty)),
                        ty.variants.iter().filter_map(|variant| match variant {
                            SpecUntaggedVariant::Some(_, ty) => {
                                Some((EdgeKind::Reference, *ty))
                            }
                            _ => None,
                        }),
                    )
                    .map(|(kind, ty)| (Some(top), kind, ty))
                    .rev(),
                );
            }
            SpecType::Schema(SpecSchemaType::Tagged(_, ty))
            | SpecType::Inline(SpecInlineType::Tagged(_, ty)) => {
                self.stack.extend(
                    itertools::chain!(
                        ty.fields
                            .iter()
                            .map(|field| (EdgeKind::Reference, field.ty)),
                        ty.variants
                            .iter()
                            .map(|variant| (EdgeKind::Reference, variant.ty)),
                    )
                    .map(|(kind, ty)| (Some(top), kind, ty))
                    .rev(),
                );
            }
            SpecType::Schema(SpecSchemaType::Container(_, container))
            | SpecType::Inline(SpecInlineType::Container(_, container)) => {
                self.stack
                    .push((Some(top), EdgeKind::Reference, container.inner().ty));
            }
            SpecType::Schema(
                SpecSchemaType::Enum(..) | SpecSchemaType::Primitive(..) | SpecSchemaType::Any(_),
            )
            | SpecType::Inline(
                SpecInlineType::Enum(..) | SpecInlineType::Primitive(..) | SpecInlineType::Any(_),
            ) => (),
            SpecType::Ref(_) => (),
        }
        Some((parent, kind, top))
    }
}

/// A map that can store one value for each type.
pub(super) type ExtensionMap = FxHashMap<TypeId, Box<dyn Extension>>;

pub trait Extension: Any + Send + Sync {
    fn into_inner(self: Box<Self>) -> Box<dyn Any>;
}

impl dyn Extension {
    #[inline]
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref::<T>()
    }
}

impl<T: Send + Sync + 'static> Extension for T {
    #[inline]
    fn into_inner(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// Strongly connected components (SCCs) in topological order.
///
/// [`TopoSccs`] uses Tarjan's single-pass algorithm to find all SCCs,
/// and provides topological ordering, efficient membership testing, and
/// condensation for computing the transitive closure. These are
/// building blocks for cycle detection and dependency propagation.
struct TopoSccs<'a, N, E> {
    graph: &'a DiGraph<N, E, usize>,
    tarjan: TarjanScc<NodeIndex<usize>>,
    sccs: Vec<FixedBitSet>,
}

impl<'a, N, E> TopoSccs<'a, N, E> {
    fn new(graph: &'a DiGraph<N, E, usize>) -> Self {
        let mut sccs = Vec::new();
        let mut tarjan = TarjanScc::new();
        tarjan.run(graph, |scc_nodes| {
            sccs.push(scc_nodes.iter().map(|node| node.index()).collect());
        });
        // Tarjan's algorithm returns SCCs in reverse topological order;
        // reverse them to get the topological order.
        sccs.reverse();
        Self {
            graph,
            tarjan,
            sccs,
        }
    }

    #[inline]
    fn scc_count(&self) -> usize {
        self.sccs.len()
    }

    /// Returns the topological index of the SCC that contains the given node.
    #[inline]
    fn topo_index(&self, node: NodeIndex<usize>) -> usize {
        // Tarjan's algorithm returns indices in reverse topological order;
        // inverting the component index gets us the topological index.
        self.sccs.len() - 1 - self.tarjan.node_component_index(self.graph, node)
    }

    /// Iterates over the SCCs in topological order.
    #[cfg(test)]
    fn iter(&self) -> std::slice::Iter<'_, FixedBitSet> {
        self.sccs.iter()
    }

    /// Builds a condensed DAG of SCCs.
    ///
    /// The condensed graph is represented as an adjacency list, where both
    /// the node indices and the neighbors of each node are stored in
    /// topological order. This specific ordering is required by
    /// [`tred::dag_transitive_reduction_closure`].
    fn condensation(&self) -> UnweightedList<usize> {
        let mut dag = UnweightedList::with_capacity(self.scc_count());
        for to in 0..self.scc_count() {
            dag.add_node();
            for index in self.sccs[to].ones().map(NodeIndex::new) {
                for neighbor in self.graph.neighbors_directed(index, Direction::Incoming) {
                    let from = self.topo_index(neighbor);
                    if from != to {
                        dag.update_edge(from, to, ());
                    }
                }
            }
        }
        dag
    }
}

/// Controls how to continue traversing the graph when at a node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Traversal {
    /// Yield this node, then explore its neighbors.
    Visit,
    /// Yield this node, but skip its neighbors.
    Stop,
    /// Don't yield this node, but explore its neighbors.
    Skip,
    /// Don't yield this node, and skip its neighbors.
    Ignore,
}

/// Edge-kind-aware breadth-first traversal of the type graph.
///
/// [`Traverse`] tracks discovered nodes separately per [`EdgeKind`],
/// so a node that's reachable via both reference and inheritance edges
/// is visited once for each kind.
///
/// Use [`Traverse::run`] with a filter to control which nodes are
/// yielded and explored.
pub struct Traverse<'a> {
    graph: &'a CookedDiGraph<'a>,
    stack: VecDeque<(EdgeKind, NodeIndex<usize>)>,
    discovered: EnumMap<EdgeKind, FixedBitSet>,
    direction: Direction,
}

impl<'a> Traverse<'a> {
    /// Starts a breadth-first traversal at a `root` node,
    /// including `root` in the traversal.
    pub fn at_root(
        graph: &'a CookedDiGraph<'a>,
        root: NodeIndex<usize>,
        direction: Direction,
    ) -> Self {
        let mut discovered = enum_map!(_ => graph.visit_map());
        discovered[EdgeKind::Reference].grow_and_insert(root.index());
        Self {
            graph,
            stack: VecDeque::from([(EdgeKind::Reference, root)]),
            discovered,
            direction,
        }
    }

    /// Starts a breadth-first traversal at multiple `roots`,
    /// including each root in the traversal.
    pub fn at_roots(
        graph: &'a CookedDiGraph<'a>,
        roots: &FixedBitSet,
        direction: Direction,
    ) -> Self {
        let mut stack = VecDeque::new();
        let mut discovered = enum_map!(_ => graph.visit_map());
        stack.extend(
            roots
                .ones()
                .map(|index| (EdgeKind::Reference, NodeIndex::new(index))),
        );
        discovered[EdgeKind::Reference].union_with(roots);
        Self {
            graph,
            stack,
            discovered,
            direction,
        }
    }

    /// Starts a breadth-first traversal from the immediate neighbors of `root`,
    /// excluding `root` itself from the traversal.
    pub fn from_neighbors(
        graph: &'a CookedDiGraph<'a>,
        root: NodeIndex<usize>,
        direction: Direction,
    ) -> Self {
        let mut stack = VecDeque::new();
        let mut discovered = enum_map! {
            _ => {
                let mut map = graph.visit_map();
                map.visit(root);
                map
            }
        };
        for (kind, neighbors) in neighbors(graph, root, direction) {
            stack.extend(
                neighbors
                    .difference(&discovered[kind])
                    .map(|index| (kind, NodeIndex::new(index))),
            );
            discovered[kind].union_with(&neighbors);
        }
        Self {
            graph,
            stack,
            discovered,
            direction,
        }
    }

    pub fn run<F>(
        mut self,
        filter: F,
    ) -> impl Iterator<Item = (EdgeKind, NodeIndex<usize>)> + use<'a, F>
    where
        F: Fn(EdgeKind, NodeIndex<usize>) -> Traversal,
    {
        std::iter::from_fn(move || {
            while let Some((kind, index)) = self.stack.pop_front() {
                let traversal = filter(kind, index);

                if matches!(traversal, Traversal::Visit | Traversal::Skip) {
                    for (kind, neighbors) in neighbors(self.graph, index, self.direction) {
                        for neighbor in neighbors.difference(&self.discovered[kind]) {
                            self.stack.push_back((kind, NodeIndex::new(neighbor)));
                        }
                        self.discovered[kind].union_with(&neighbors);
                    }
                }

                if matches!(traversal, Traversal::Visit | Traversal::Stop) {
                    return Some((kind, index));
                }

                // `Skip` and `Ignore` continue the loop without yielding.
            }
            None
        })
    }
}

/// Returns the neighbors of `node` in the given `direction`,
/// grouped by their [`EdgeKind`].
fn neighbors(
    graph: &CookedDiGraph<'_>,
    node: NodeIndex<usize>,
    direction: Direction,
) -> EnumMap<EdgeKind, FixedBitSet> {
    let mut neighbors = enum_map!(_ => graph.visit_map());
    for edge in graph.edges_directed(node, direction) {
        let neighbor = match direction {
            Direction::Outgoing => edge.target(),
            Direction::Incoming => edge.source(),
        };
        neighbors[*edge.weight()].insert(neighbor.index());
    }
    neighbors
}

#[cfg(test)]
mod tests {
    use super::*;

    use petgraph::visit::NodeCount;

    use crate::tests::assert_matches;

    /// Creates a simple graph: `A -> B -> C`.
    fn linear_graph() -> DiGraph<(), (), usize> {
        let mut g = DiGraph::default();
        let a = g.add_node(());
        let b = g.add_node(());
        let c = g.add_node(());
        g.extend_with_edges([(a, b), (b, c)]);
        g
    }

    /// Creates a cyclic graph: `A -> B -> C -> A`, with `D -> A`.
    fn cyclic_graph() -> DiGraph<(), (), usize> {
        let mut g = DiGraph::default();
        let a = g.add_node(());
        let b = g.add_node(());
        let c = g.add_node(());
        let d = g.add_node(());
        g.extend_with_edges([(a, b), (b, c), (c, a), (d, a)]);
        g
    }

    // MARK: SCC detection

    #[test]
    fn test_linear_graph_has_singleton_sccs() {
        let g = linear_graph();
        let sccs = TopoSccs::new(&g);
        let sizes = sccs.iter().map(|scc| scc.count_ones(..)).collect_vec();
        assert_matches!(&*sizes, [1, 1, 1]);
    }

    #[test]
    fn test_cyclic_graph_has_one_multi_node_scc() {
        let g = cyclic_graph();
        let sccs = TopoSccs::new(&g);

        // A-B-C form one SCC; D is its own SCC. Since D has an edge to
        // the cycle, D must precede the cycle in topological order.
        let sizes = sccs.iter().map(|scc| scc.count_ones(..)).collect_vec();
        assert_matches!(&*sizes, [1, 3]);
    }

    // MARK: Topological ordering

    #[test]
    fn test_sccs_are_in_topological_order() {
        let g = cyclic_graph();
        let sccs = TopoSccs::new(&g);

        let d_topo = sccs.topo_index(3.into());
        let a_topo = sccs.topo_index(0.into());
        assert!(
            d_topo < a_topo,
            "D should precede A-B-C in topological order"
        );
    }

    #[test]
    fn test_topo_index_consistent_within_scc() {
        let g = cyclic_graph();
        let sccs = TopoSccs::new(&g);

        // A, B, C are in the same SCC, so they should have
        // the same topological index.
        let a_topo = sccs.topo_index(0.into());
        let b_topo = sccs.topo_index(1.into());
        let c_topo = sccs.topo_index(2.into());

        assert_eq!(a_topo, b_topo);
        assert_eq!(b_topo, c_topo);
    }

    // MARK: Condensation

    #[test]
    fn test_condensation_has_correct_node_count() {
        let g = cyclic_graph();
        let sccs = TopoSccs::new(&g);
        let dag = sccs.condensation();

        assert_eq!(dag.node_count(), 2);
    }

    #[test]
    fn test_condensation_has_correct_edges() {
        let g = cyclic_graph();
        let sccs = TopoSccs::new(&g);
        let dag = sccs.condensation();

        // D should have an edge to the A-B-C SCC, and
        // A-B-C shouldn't create a self-loop.
        let d_topo = sccs.topo_index(3.into());
        let abc_topo = sccs.topo_index(0.into());

        let d_neighbors = dag.neighbors(d_topo).collect_vec();
        assert_eq!(&*d_neighbors, [abc_topo]);

        let abc_neighbors = dag.neighbors(abc_topo).collect_vec();
        assert!(abc_neighbors.is_empty());
    }

    #[test]
    fn test_condensation_neighbors_in_topological_order() {
        // Matches Petgraph's `dag_to_toposorted_adjacency_list` example:
        // edges added as `(top, second), (top, first)`, but neighbors should be
        // `[first, second]` in topological order.
        let mut g = DiGraph::<(), (), usize>::default();
        let second = g.add_node(());
        let top = g.add_node(());
        let first = g.add_node(());
        g.extend_with_edges([(top, second), (top, first), (first, second)]);

        let sccs = TopoSccs::new(&g);
        let dag = sccs.condensation();

        let top_topo = sccs.topo_index(top);
        assert_eq!(top_topo, 0);

        let first_topo = sccs.topo_index(first);
        assert_eq!(first_topo, 1);

        let second_topo = sccs.topo_index(second);
        assert_eq!(second_topo, 2);

        let neighbors = dag.neighbors(top_topo).collect_vec();
        assert_eq!(&*neighbors, [first_topo, second_topo]);
    }
}
