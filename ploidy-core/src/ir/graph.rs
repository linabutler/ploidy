use std::{
    any::{Any, TypeId},
    borrow::Cow,
    collections::VecDeque,
    fmt::Debug,
};

use atomic_refcell::AtomicRefCell;
use by_address::ByAddress;
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
    cooker::Cooker,
    spec::IrSpec,
    types::{
        InlineIrType, InlineIrTypePath, InlineIrTypePathRoot, InlineIrTypePathSegment, IrOperation,
        IrStruct, IrStructFieldName, IrTagged, IrTaggedVariant, IrType, IrUntaggedVariant,
        SchemaIrType, SchemaTypeInfo,
    },
    views::{operation::IrOperationView, primitive::IrPrimitiveView, schema::SchemaIrTypeView},
};

/// The mutable, sparse graph used for transformations.
type RawDiGraph<'a> = StableDiGraph<GraphNode<'a>, EdgeKind, usize>;

/// The immutable, dense graph used for code generation.
type CookedDiGraph<'a> = DiGraph<GraphNode<'a, NodeIndex<usize>>, EdgeKind, usize>;

/// An operation with all type references resolved to graph indices.
pub type CookedOperation<'a> = &'a IrOperation<'a, NodeIndex<usize>>;

/// A mutable intermediate dependency graph of all the types in an [`IrSpec`],
/// backed by a sparse [`StableDiGraph`].
///
/// This graph is constructed directly from an [`IrSpec`], and represents
/// type relationships as they exist in the spec. Transformations like
/// [`inline_tagged_variants`][Self::inline_tagged_variants] rewrite this graph
/// in place.
///
/// After applying all transformations, call [`cook`][Self::cook] to
/// turn this graph into a [`CookedGraph`] that's ready for code generation.
#[derive(Debug)]
pub struct RawGraph<'a> {
    arena: &'a Arena,
    spec: &'a IrSpec<'a>,
    graph: RawDiGraph<'a>,
    indices: FxHashMap<GraphNode<'a>, NodeIndex<usize>>,
    /// Maps schema names to their graph indices.
    schemas: FxHashMap<&'a str, NodeIndex<usize>>,
}

impl<'a> RawGraph<'a> {
    pub fn new(arena: &'a Arena, spec: &'a IrSpec<'a>) -> Self {
        let mut graph = RawDiGraph::default();
        let mut indices = FxHashMap::default();
        let mut schemas = FxHashMap::default();

        // All roots (named schemas, parameters, request and response bodies),
        // and all the types within them (inline schemas and primitives).
        let tys = IrTypeVisitor::new(
            spec.schemas
                .values()
                .chain(spec.operations.iter().flat_map(|op| op.types().copied())),
        );

        // Add nodes for all types, and edges for references between them.
        for (parent, kind, child) in tys {
            use std::collections::hash_map::Entry;
            let source = spec.resolve(child);
            let &mut to = match indices.entry(source) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    // We might see the same schema multiple times if it's
                    // referenced multiple times in the spec. Only add
                    // a new node for the schema if we haven't seen it before.
                    let index = graph.add_node(*entry.key());
                    entry.insert(index)
                }
            };
            // Track schema names for later lookup.
            if let GraphNode::Schema(ty) = source {
                schemas.entry(ty.name()).or_insert(to);
            }
            if let Some(parent) = parent {
                let destination = spec.resolve(parent);
                let &mut from = match indices.entry(destination) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => {
                        let index = graph.add_node(*entry.key());
                        entry.insert(index)
                    }
                };
                if let GraphNode::Schema(ty) = destination {
                    schemas.entry(ty.name()).or_insert(from);
                }
                graph.add_edge(from, to, kind);
            }
        }

        Self {
            arena,
            spec,
            graph,
            indices,
            schemas,
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
            modified_tagged: IrTagged<'a>,
            inlines: Vec<VariantInline<'a>>,
        }
        struct VariantInline<'a> {
            ty: &'a IrType<'a>,
            variant_index: NodeIndex<usize>,
            parent_indices: Vec<NodeIndex<usize>>,
        }

        // Compute the set of types used (as query params, request and response
        // bodies, etc.) by operations. Operations don't create graph edges,
        // but still need to be considered when deciding whether to inline a
        // struct variant. Otherwise, a struct that's used by same-tag unions
        // _and_ an operation wouldn't be inlined, causing the Rust generator to
        // incorrectly exclude the tag field from the struct.
        let used_by_ops: FixedBitSet = self
            .spec
            .operations
            .iter()
            .flat_map(|op| op.types())
            .filter_map(|ty| {
                let node = self.resolve(ty);
                self.indices.get(&node).map(|node| node.index())
            })
            .collect();

        // Collect all inlining decisions before mutating the graph,
        // so that we can check inlinability per variant.
        let plans = self
            .graph
            .node_indices()
            .filter_map(|index| {
                let GraphNode::Schema(SchemaIrType::Tagged(info, tagged)) = self.graph[index]
                else {
                    return None;
                };
                let mut new_variants = Cow::Borrowed(tagged.variants);
                let mut inlines = vec![];

                for (at, variant) in tagged.variants.iter().enumerate() {
                    let variant_node = self.resolve(variant.ty);
                    let GraphNode::Schema(SchemaIrType::Struct(variant_info, variant_struct)) =
                        variant_node
                    else {
                        continue;
                    };
                    let variant_index = self.indices[&variant_node];

                    // A struct variant only needs inlining if it has multiple
                    // distinct uses. Skip if (1) no operation uses the struct,
                    // _and_ (2) every incoming edge is from a tagged union with
                    // the same tag. If both hold, all uses agree, so the
                    // struct can be used directly without inlining.
                    if !used_by_ops.contains(variant_index.index()) {
                        let Some(first) = ({
                            self.graph
                                .neighbors_directed(variant_index, Direction::Incoming)
                                .find_map(|neighbor| match self.graph[neighbor] {
                                    GraphNode::Schema(SchemaIrType::Tagged(_, t)) => Some(t),
                                    _ => None,
                                })
                        }) else {
                            continue;
                        };
                        // Check that all the variant's inbound edges are from
                        // tagged unions, and that all their tags match the tag
                        // of the first union we found.
                        let all_tags_match = self
                            .graph
                            .neighbors_directed(variant_index, Direction::Incoming)
                            .all(|neighbor| matches!(
                                self.graph[neighbor],
                                GraphNode::Schema(SchemaIrType::Tagged(_, t)) if t.tag == first.tag,
                            ));
                        if all_tags_match {
                            continue;
                        }
                    }

                    // Skip inlining the struct variant if it doesn't declare
                    // the tag as a field. Inlining these struct variants
                    // would just produce identical inline structs.
                    let has_tag_field = {
                        let inherits = EdgeFiltered::from_fn(&self.graph, |edge| {
                            matches!(edge.weight(), EdgeKind::Inherits)
                        });
                        let mut dfs = DfsPostOrder::new(&inherits, variant_index);
                        std::iter::from_fn(|| dfs.next(&inherits))
                            .flat_map(|ancestor| match self.graph[ancestor] {
                                GraphNode::Schema(SchemaIrType::Struct(_, s))
                                | GraphNode::Inline(InlineIrType::Struct(_, s)) => s.fields,
                                _ => &[],
                            })
                            .any(|f| {
                                // Check own and inherited fields; OpenAPI 3.2
                                // clarifies that the tag can be inherited.
                                matches!(f.name, IrStructFieldName::Name(n) if n == tagged.tag)
                            })
                    };
                    if !has_tag_field {
                        continue;
                    }

                    // Build our new inline type, with the same attributes
                    // as the schema type, but a distinct inline type path.
                    let ty: &_ = self.arena.alloc(IrType::Inline(InlineIrType::Struct(
                        InlineIrTypePath {
                            root: InlineIrTypePathRoot::Type(info.name),
                            segments: self.arena.alloc_slice_copy(&[
                                InlineIrTypePathSegment::TaggedVariant(variant_info.name),
                            ]),
                        },
                        IrStruct {
                            description: variant_struct.description,
                            fields: variant_struct.fields,
                            parents: variant_struct.parents,
                        },
                    )));

                    let parent_indices = variant_struct
                        .parents
                        .iter()
                        .map(|parent| {
                            let parent_node = self.resolve(parent);
                            self.indices[&parent_node]
                        })
                        .collect_vec();

                    inlines.push(VariantInline {
                        ty,
                        variant_index,
                        parent_indices,
                    });

                    new_variants.to_mut()[at] = IrTaggedVariant {
                        name: variant.name,
                        aliases: variant.aliases,
                        ty,
                    };
                }
                if new_variants == tagged.variants {
                    // No variants to rewrite.
                    return None;
                }

                Some(TaggedPlan {
                    tagged_index: index,
                    info: *info,
                    modified_tagged: IrTagged {
                        description: tagged.description,
                        tag: tagged.tag,
                        variants: self.arena.alloc_slice_copy(&new_variants),
                    },
                    inlines,
                })
            })
            .collect_vec();

        // Apply the plans to the graph.
        for plan in plans {
            // Add nodes for the inlined types, and connect them to
            // the original schema variants, so that they'll inherit
            // the same transitive dependencies and SCC membership.
            for entry in &plan.inlines {
                let node = self.resolve(entry.ty);
                let node_index = self.graph.add_node(node);
                self.indices.insert(node, node_index);
                self.graph
                    .add_edge(node_index, entry.variant_index, EdgeKind::Reference);
                for &parent_index in &entry.parent_indices {
                    self.graph
                        .add_edge(node_index, parent_index, EdgeKind::Inherits);
                }
            }

            // Rewrite reference edges from the tagged union
            // to point to its new variants.
            let edges_to_remove = self
                .graph
                .edges_directed(plan.tagged_index, Direction::Outgoing)
                .filter(|e| matches!(e.weight(), EdgeKind::Reference))
                .map(|e| e.id())
                .collect_vec();
            for edge_id in edges_to_remove {
                self.graph.remove_edge(edge_id);
            }
            for variant in plan.modified_tagged.variants {
                let variant_node = self.resolve(variant.ty);
                let variant_index = self.indices[&variant_node];
                self.graph
                    .add_edge(plan.tagged_index, variant_index, EdgeKind::Reference);
            }

            // ...And replace the node for the tagged union itself.
            let new_node = GraphNode::Schema(
                self.arena
                    .alloc(SchemaIrType::Tagged(plan.info, plan.modified_tagged)),
            );
            let old_node = std::mem::replace(&mut self.graph[plan.tagged_index], new_node);
            self.indices.remove(&old_node);
            self.indices.insert(new_node, plan.tagged_index);
        }

        self
    }

    /// Builds an immutable [`CookedGraph`] from this mutable raw graph.
    #[inline]
    pub fn cook(&self) -> CookedGraph<'a> {
        CookedGraph::new(self)
    }

    /// Resolves an [`IrType`] to a [`GraphNode`], following
    /// [`IrType::Ref`]s through the spec.
    #[inline]
    fn resolve(&self, ty: &'a IrType<'a>) -> GraphNode<'a> {
        match ty {
            IrType::Schema(ty) => GraphNode::Schema(ty),
            IrType::Inline(ty) => GraphNode::Inline(ty),
            IrType::Ref(r) => self.graph[self.schemas[r.name()]],
        }
    }
}

/// The final dependency graph of all the types in an [`IrSpec`],
/// backed by a dense [`DiGraph`].
///
/// This graph has all transformations applied, and is ready for
/// code generation.
#[derive(Debug)]
pub struct CookedGraph<'a> {
    pub(super) graph: CookedDiGraph<'a>,
    info: &'a Info,
    ops: &'a [CookedOperation<'a>],
    /// Additional metadata for each node.
    pub(super) metadata: CookedGraphMetadata<'a>,
}

impl<'a> CookedGraph<'a> {
    fn new(raw: &RawGraph<'a>) -> Self {
        // Assign a cooked node index to each raw node index.
        let indices: FxHashMap<_, _> = raw
            .graph
            .node_indices()
            .zip(0..)
            .map(|(raw, cooked)| (raw, NodeIndex::new(cooked)))
            .collect();

        // Cook each node, translating raw types to their
        // assigned cooked indices.
        let cooker = Cooker::new(raw.arena, |ty| {
            let raw = match ty {
                IrType::Schema(s) => raw.indices[&GraphNode::Schema(s)],
                IrType::Inline(i) => raw.indices[&GraphNode::Inline(i)],
                IrType::Ref(r) => raw.schemas[r.name()],
            };
            indices[&raw]
        });

        // Build a dense graph.
        let mut graph = CookedDiGraph::with_capacity(indices.len(), raw.graph.edge_count());
        for index in raw.graph.node_indices() {
            let node = raw.graph[index];
            let cooked = graph.add_node(cooker.node(node));
            assert_eq!(indices[&index], cooked);
        }

        // Add edges, preserving original insertion order.
        for index in raw.graph.node_indices() {
            let from = indices[&index];
            // `RawDiGraph::edges` yields edges in reverse insertion order;
            // collect and reverse to preserve the original order.
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

        let (metadata, operations) = {
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

            // Cook each operation.
            let operations: &_ = raw
                .arena
                .alloc_slice_exact(raw.spec.operations.iter().map(|op| cooker.operation(op)));

            // Precompute the set of type indices that each operation
            // references directly.
            for &op in operations {
                metadata.operations.entry(ByAddress(op)).or_default().types =
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
            for &op in operations {
                let meta = &metadata.operations[&ByAddress(op)];

                // Collect all the types that this operation depends on.
                let mut transitive_deps = FixedBitSet::with_capacity(graph.node_count());
                for node in meta.types.ones() {
                    transitive_deps.insert(node);
                    transitive_deps.union_with(&metadata.schemas[node].dependencies);
                }

                // Mark each type as being used by this operation.
                for node in transitive_deps.ones() {
                    metadata.schemas[node].used_by.insert(ByAddress(op));
                }
            }

            (metadata, operations)
        };

        Self {
            graph,
            info: raw.spec.info,
            ops: operations,
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
    pub fn schemas(&self) -> impl Iterator<Item = SchemaIrTypeView<'_>> {
        self.graph
            .node_indices()
            .filter_map(|index| match self.graph[index] {
                GraphNode::Schema(ty) => Some(SchemaIrTypeView::new(self, index, ty)),
                _ => None,
            })
    }

    /// Returns an iterator over all primitive type nodes in this graph.
    #[inline]
    pub fn primitives(&self) -> impl Iterator<Item = IrPrimitiveView<'_>> {
        self.graph
            .node_indices()
            .filter_map(|index| match self.graph[index] {
                GraphNode::Schema(SchemaIrType::Primitive(_, p))
                | GraphNode::Inline(InlineIrType::Primitive(_, p)) => {
                    Some(IrPrimitiveView::new(self, index, *p))
                }
                _ => None,
            })
    }

    /// Returns an iterator over all the operations in this graph.
    #[inline]
    pub fn operations(&self) -> impl Iterator<Item = IrOperationView<'_>> {
        self.ops
            .iter()
            .map(move |&op| IrOperationView::new(self, op))
    }
}

/// A node in the type graph.
///
/// The derived [`Hash`][std::hash::Hash] and [`Eq`] implementations
/// work on the underlying values, so structurally identical types
/// will be equal. This is important: all types in an [`IrSpec`] are
/// distinct in memory, but can refer to the same logical type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GraphNode<'a, Ty = &'a IrType<'a>> {
    Schema(&'a SchemaIrType<'a, Ty>),
    Inline(&'a InlineIrType<'a, Ty>),
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
pub struct CookedGraphMetadata<'a> {
    /// Maps each node index to its strongly connected component index.
    /// Nodes in the same SCC form a cycle.
    pub scc_indices: Vec<usize>,
    pub schemas: Vec<GraphNodeMeta<'a>>,
    pub operations: FxHashMap<ByAddress<CookedOperation<'a>>, GraphOperationMeta>,
}

/// Precomputed metadata for an operation that references
/// types in the graph.
#[derive(Debug, Default)]
pub struct GraphOperationMeta {
    /// Indices of all the types that this operation directly depends on:
    /// parameters, request body, and response body.
    pub types: FixedBitSet,
}

/// Precomputed metadata for a schema type in the graph.
#[derive(Default)]
pub(super) struct GraphNodeMeta<'a> {
    /// Operations that use this type.
    pub used_by: FxHashSet<ByAddress<CookedOperation<'a>>>,
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

/// Visits all the types and references contained within a type.
#[derive(Debug)]
struct IrTypeVisitor<'a> {
    stack: Vec<(Option<&'a IrType<'a>>, EdgeKind, &'a IrType<'a>)>,
}

impl<'a> IrTypeVisitor<'a> {
    /// Creates a visitor with `root` on the stack of types to visit.
    #[inline]
    fn new(roots: impl Iterator<Item = &'a IrType<'a>>) -> Self {
        let mut stack = roots
            .map(|root| (None, EdgeKind::Reference, root))
            .collect_vec();
        stack.reverse();
        Self { stack }
    }
}

impl<'a> Iterator for IrTypeVisitor<'a> {
    type Item = (Option<&'a IrType<'a>>, EdgeKind, &'a IrType<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        let (parent, kind, top) = self.stack.pop()?;
        match top {
            IrType::Schema(SchemaIrType::Struct(_, ty))
            | IrType::Inline(InlineIrType::Struct(_, ty)) => {
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
            IrType::Schema(SchemaIrType::Untagged(_, ty))
            | IrType::Inline(InlineIrType::Untagged(_, ty)) => {
                self.stack.extend(
                    ty.variants
                        .iter()
                        .filter_map(|variant| match variant {
                            IrUntaggedVariant::Some(_, ty) => {
                                Some((Some(top), EdgeKind::Reference, *ty))
                            }
                            _ => None,
                        })
                        .rev(),
                );
            }
            IrType::Schema(SchemaIrType::Tagged(_, ty))
            | IrType::Inline(InlineIrType::Tagged(_, ty)) => {
                self.stack.extend(
                    ty.variants
                        .iter()
                        .map(|variant| (Some(top), EdgeKind::Reference, variant.ty))
                        .rev(),
                );
            }
            IrType::Schema(SchemaIrType::Container(_, container))
            | IrType::Inline(InlineIrType::Container(_, container)) => {
                self.stack
                    .push((Some(top), EdgeKind::Reference, container.inner().ty));
            }
            IrType::Schema(
                SchemaIrType::Enum(..) | SchemaIrType::Primitive(..) | SchemaIrType::Any(_),
            )
            | IrType::Inline(
                InlineIrType::Enum(..) | InlineIrType::Primitive(..) | InlineIrType::Any(_),
            ) => (),
            IrType::Ref(_) => (),
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
    pub fn from_roots(
        graph: &'a CookedDiGraph<'a>,
        roots: EnumMap<EdgeKind, FixedBitSet>,
        direction: Direction,
    ) -> Self {
        let mut stack = VecDeque::new();
        let mut discovered = enum_map!(_ => graph.visit_map());
        for (kind, indices) in roots {
            stack.extend(indices.ones().map(|index| (kind, NodeIndex::new(index))));
            discovered[kind].union_with(&indices);
        }
        Self {
            graph,
            stack,
            discovered,
            direction,
        }
    }

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

    pub fn run<F>(mut self, filter: F) -> impl Iterator<Item = NodeIndex<usize>> + use<'a, F>
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
                    return Some(index);
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
