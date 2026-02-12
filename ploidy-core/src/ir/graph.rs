use std::{
    any::{Any, TypeId},
    borrow::Cow,
    collections::VecDeque,
    fmt::Debug,
};

use atomic_refcell::AtomicRefCell;
use bumpalo::Bump;
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
    stable_graph::StableGraph,
    visit::{DfsPostOrder, EdgeFiltered, EdgeRef, IntoNeighbors, VisitMap, Visitable},
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::parse::Document;

use super::{
    error::IrError,
    spec::IrSpec,
    types::{
        InlineIrType, InlineIrTypePath, InlineIrTypePathRoot, InlineIrTypePathSegment, IrOperation,
        IrStruct, IrStructFieldName, IrTagged, IrTaggedVariant, IrType, IrTypeRef,
        IrUntaggedVariant, SchemaIrType,
    },
    views::{operation::IrOperationView, primitive::IrPrimitiveView, schema::SchemaIrTypeView},
};

/// The type graph.
pub(super) type IrGraphG<'a> = DiGraph<IrGraphNode<'a>, EdgeKind, usize>;

/// The stable graph used during mutable transformation.
type RawGraphG<'a> = StableGraph<IrGraphNode<'a>, EdgeKind, petgraph::Directed, usize>;

/// Owns the [`IrSpec`] and an arena for graph transformations.
///
/// Created via [`Ir::from_doc`], then use [`Ir::graph`] to build a
/// [`RawGraph`] for transformation and finalization.
#[derive(Debug)]
pub struct Ir<'a> {
    spec: IrSpec<'a>,
    arena: Bump,
}

impl<'a> Ir<'a> {
    /// Parses an OpenAPI document into the IR, allocating an
    /// arena for any graph transformations.
    #[inline]
    pub fn from_doc(doc: &'a Document) -> Result<Self, IrError> {
        let spec = IrSpec::from_doc(doc)?;
        Ok(Self {
            spec,
            arena: Bump::new(),
        })
    }

    /// Builds a [`RawGraph`] that borrows from this `Ir`.
    #[inline]
    pub fn graph(&self) -> RawGraph<'_> {
        RawGraph::new(&self.spec, &self.arena)
    }
}

/// A mutable intermediate graph of all the types in an [`IrSpec`].
///
/// Original types are `IrGraphNode` references into the `IrSpec`;
/// transformations like [`lower_tagged_variants`](Self::lower_tagged_variants)
/// arena-allocate modified or new types, producing `IrGraphNode`
/// references with the same lifetime.
///
/// After transformation, call [`RawGraph::finalize`] to produce
/// a compact [`IrGraph`].
#[derive(Debug)]
pub struct RawGraph<'a> {
    spec: &'a IrSpec<'a>,
    arena: &'a Bump,
    g: RawGraphG<'a>,
    indices: FxHashMap<IrGraphNode<'a>, NodeIndex<usize>>,
    /// Maps schema names to their node indices in the graph.
    schemas: FxHashMap<&'a str, NodeIndex<usize>>,
}

impl<'a> RawGraph<'a> {
    /// Builds a raw graph from an [`IrSpec`], using `arena` for
    /// any allocations needed by later transformations.
    pub fn new(spec: &'a IrSpec<'a>, arena: &'a Bump) -> Self {
        let mut g = RawGraphG::default();
        let mut indices = FxHashMap::default();
        let mut schemas = FxHashMap::default();

        // All roots (named schemas, parameters, request and response bodies),
        // and all the types within them (inline schemas and primitives).
        let tys = IrTypeVisitor::new(
            spec.schemas
                .values()
                .chain(spec.operations.iter().flat_map(|op| op.types())),
        );

        // Add nodes for all types, and edges for references between them.
        for (parent, kind, child) in tys {
            use std::collections::hash_map::Entry;
            let child_node = resolve(spec, child.as_ref());
            let &mut to = match indices.entry(child_node) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    // We might see the same schema multiple times, if it's
                    // referenced multiple times in the spec. Only add a new node
                    // for the schema if we haven't seen it before.
                    let index = g.add_node(*entry.key());
                    entry.insert(index)
                }
            };
            // Track schema names for later lookup.
            if let IrGraphNode::Schema(ty) = child_node {
                schemas.entry(ty.name()).or_insert(to);
            }
            if let Some(parent) = parent {
                let parent_node = resolve(spec, parent.as_ref());
                let &mut from = match indices.entry(parent_node) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => {
                        let index = g.add_node(*entry.key());
                        entry.insert(index)
                    }
                };
                if let IrGraphNode::Schema(ty) = parent_node {
                    schemas.entry(ty.name()).or_insert(from);
                }
                g.add_edge(from, to, kind);
            }
        }

        Self {
            spec,
            arena,
            g,
            indices,
            schemas,
        }
    }

    /// Rewrites standalone tagged-union variants as inline types.
    ///
    /// A variant is "standalone" if its struct is used outside
    /// tagged unions (by operations, struct fields, containers,
    /// etc.). Standalone variants get their own inline struct;
    /// parents that contain the tag field are recursively inlined.
    ///
    /// Iterates tagged unions directly: each one is processed
    /// atomically by discovering standalone variants, creating
    /// their inline rewrites, and updating the node and edges
    /// in place.
    pub fn lower_tagged_variants(&mut self) -> &mut Self {
        // Snapshot node indices before iteration, which adds new inline nodes.
        let indices: FixedBitSet = self.g.node_indices().map(|index| index.index()).collect();

        // Pre-compute the set of node indices that operations reference
        // directly, so `is_standalone` can check membership in O(1)
        // instead of scanning all operations per node.
        let op_referenced: FixedBitSet = self
            .spec
            .operations
            .iter()
            .flat_map(|op| op.types())
            .filter_map(|ty| {
                let node = self.resolve_ref(ty.as_ref());
                self.indices.get(&node).map(|idx| idx.index())
            })
            .collect();

        // Pre-compute standalone status before any mutations, since
        // processing one tagged union's variants can remove edges that
        // would affect `is_standalone` for later tagged unions.
        let standalone: FixedBitSet = indices
            .ones()
            .filter(|&i| self.is_standalone(NodeIndex::new(i), &op_referenced))
            .collect();

        for index in indices.ones() {
            let index = NodeIndex::new(index);
            let IrGraphNode::Schema(SchemaIrType::Tagged(info, tagged)) = self.g[index] else {
                continue;
            };

            // Build modified variants, replacing standalone struct variants
            // with tag-stripped inline copies.
            let mut new_variants = Cow::Borrowed(&tagged.variants);

            for (pos, variant) in tagged.variants.iter().enumerate() {
                let variant_node = self.resolve_ref(variant.ty.as_ref());
                let IrGraphNode::Schema(variant_ty @ SchemaIrType::Struct(..)) = variant_node
                else {
                    continue;
                };
                let variant_index = self.indices[&variant_node];
                if !standalone.contains(variant_index.index()) {
                    continue;
                }

                // Only inline when the variant struct (or an ancestor)
                // actually has the tag field; otherwise the inline would
                // be identical to the original schema struct.
                if !self.has_tag_field(variant_index, tagged.tag) {
                    continue;
                }

                // Build inline path: Type(tagged_name) / TaggedVariant(schema_name).
                let path = InlineIrTypePath {
                    root: InlineIrTypePathRoot::Type(info.name),
                    segments: vec![InlineIrTypePathSegment::TaggedVariant(variant_ty.name())],
                };

                let inlines = self.rewrite_struct(variant_index, path);

                // Add all inline nodes to the graph and wire edges.
                // Instead of adding per-field edges (which would pull
                // the original schema's inline types into this tagged
                // union's `inlines()`), connect each inline to the
                // original schema variant. This gives the inline the
                // same transitive dependencies and SCC membership
                // without leaking foreign inline types.
                for &inline in &inlines {
                    let node = IrGraphNode::Inline(inline);
                    let node_index = self.g.add_node(node);
                    self.indices.insert(node, node_index);
                    self.g
                        .add_edge(node_index, variant_index, EdgeKind::Reference);
                    if let InlineIrType::Struct(_, ref s) = *inline {
                        for parent in &s.parents {
                            let parent_node = self.resolve_ref(parent.as_ref());
                            let parent_index = self.indices[&parent_node];
                            self.g
                                .add_edge(node_index, parent_index, EdgeKind::Inherits);
                        }
                    }
                }

                // The last inline is the start struct's rewrite.
                let start_inline = *inlines.last().unwrap();
                new_variants.to_mut()[pos] = IrTaggedVariant {
                    name: variant.name,
                    aliases: variant.aliases.clone(),
                    ty: IrType::Inline(start_inline.clone()),
                };
            }

            if *new_variants == tagged.variants {
                continue;
            }

            let modified_tagged = IrTagged {
                description: tagged.description,
                tag: tagged.tag,
                variants: new_variants.into_owned(),
            };
            let modified: &'a SchemaIrType<'a> = self
                .arena
                .alloc(SchemaIrType::Tagged(*info, modified_tagged));
            let old_weight = std::mem::replace(&mut self.g[index], IrGraphNode::Schema(modified));
            self.indices.remove(&old_weight);
            self.indices.insert(IrGraphNode::Schema(modified), index);

            // Rebuild Reference edges from the modified variants.
            let edges_to_remove: Vec<_> = self
                .g
                .edges_directed(index, Direction::Outgoing)
                .filter(|e| matches!(e.weight(), EdgeKind::Reference))
                .map(|e| e.id())
                .collect();
            for edge_id in edges_to_remove {
                self.g.remove_edge(edge_id);
            }
            let SchemaIrType::Tagged(_, modified_tagged) = modified else {
                unreachable!()
            };
            for variant in &modified_tagged.variants {
                let variant_node = self.resolve_ref(variant.ty.as_ref());
                let variant_index = self.indices[&variant_node];
                self.g.add_edge(index, variant_index, EdgeKind::Reference);
            }
        }

        self
    }

    /// Returns `true` if the struct at `index` is used outside
    /// tagged unions, or if incoming tagged unions disagree on
    /// their discriminator.
    fn is_standalone(&self, index: NodeIndex<usize>, op_referenced: &FixedBitSet) -> bool {
        // Check if any operation directly references this struct.
        if op_referenced.contains(index.index()) {
            return true;
        }

        // Standalone unless every incoming edge is from a tagged
        // union with the same discriminator.
        let mut neighbors = self.g.neighbors_directed(index, Direction::Incoming);
        if let Some(neighbor_index) = neighbors.next()
            && let IrGraphNode::Schema(SchemaIrType::Tagged(_, tagged)) = self.g[neighbor_index]
        {
            !neighbors.all(|neighbor_index| {
                let neighbor_node = self.g[neighbor_index];
                matches!(neighbor_node, IrGraphNode::Schema(SchemaIrType::Tagged(_, t))
                    if t.tag == tagged.tag)
            })
        } else {
            true
        }
    }

    /// Returns `true` if the struct at `index` or any of its
    /// `allOf` ancestors has a field named `tag`.
    fn has_tag_field(&self, index: NodeIndex<usize>, tag: &str) -> bool {
        self.inherited_structs(index).iter().any(|(_, s)| {
            s.fields
                .iter()
                .any(|f| matches!(f.name, IrStructFieldName::Name(n) if n == tag))
        })
    }

    /// Collects the struct at `index` and all its `allOf` ancestors
    /// in post-order (ancestors first, start struct last).
    fn inherited_structs(
        &self,
        index: NodeIndex<usize>,
    ) -> Vec<(NodeIndex<usize>, &'a IrStruct<'a>)> {
        let inherits = EdgeFiltered::from_fn(&self.g, |e| matches!(e.weight(), EdgeKind::Inherits));
        let mut dfs = DfsPostOrder::new(&inherits, index);
        let mut result = Vec::new();
        while let Some(nx) = dfs.next(&inherits) {
            let s = match self.g[nx] {
                IrGraphNode::Schema(SchemaIrType::Struct(_, s))
                | IrGraphNode::Inline(InlineIrType::Struct(_, s)) => s,
                _ => continue,
            };
            result.push((nx, s));
        }
        result
    }

    /// Resolves an `IrTypeRef` to an `IrGraphNode`, following `$ref`
    /// pointers through the spec.
    fn resolve_ref(&self, ty: IrTypeRef<'a>) -> IrGraphNode<'a> {
        match ty {
            IrTypeRef::Schema(ty) => IrGraphNode::Schema(ty),
            IrTypeRef::Inline(ty) => IrGraphNode::Inline(ty),
            IrTypeRef::Ref(r) => {
                let index = self.schemas[r.name()];
                self.g[index]
            }
        }
    }

    /// Creates inline copies of the struct at `start_index` and
    /// its ancestors, rewriting parent references to point at the
    /// new inlines. Returns the new inline types in
    /// graph-insertion order (ancestors first, start struct last).
    ///
    /// Uses [`DfsPostOrder`] on the `Inherits`-filtered subgraph so
    /// that ancestors are processed before descendants, and each
    /// struct's rewritten parents are available when it is
    /// constructed. Paths are assigned in a reverse pass (top-down)
    /// over the same post-order results.
    fn rewrite_struct(
        &self,
        start_index: NodeIndex<usize>,
        path: InlineIrTypePath<'a>,
    ) -> Vec<&'a InlineIrType<'a>> {
        let post_order = self.inherited_structs(start_index);

        // Assign paths top-down (reverse post-order). Each struct
        // derives its parents' paths from its own path + `Parent(idx)`.
        let mut paths: FxHashMap<NodeIndex<usize>, InlineIrTypePath<'a>> = FxHashMap::default();
        paths.insert(start_index, path);
        for &(nx, s) in post_order.iter().rev() {
            let Some(p) = paths.get(&nx).cloned() else {
                continue;
            };
            for (idx, parent) in s
                .parents
                .iter()
                .enumerate()
                .map(|(index, parent)| (index + 1, parent))
            {
                let parent_node = self.resolve_ref(parent.as_ref());
                let parent_index = self.indices[&parent_node];
                paths
                    .entry(parent_index)
                    .or_insert_with(|| InlineIrTypePath {
                        root: p.root,
                        segments: {
                            let mut segs = p.segments.clone();
                            segs.push(InlineIrTypePathSegment::Parent(idx));
                            segs
                        },
                    });
            }
        }

        // Process in post-order (ancestors first). Track rewrites by
        // their inline type directly, avoiding graph mutation.
        let mut rewrites: FxHashMap<NodeIndex<usize>, &'a InlineIrType<'a>> = FxHashMap::default();
        let mut result = Vec::new();
        for (nx, s) in post_order {
            // Build parents eagerly, replacing rewritten ones with
            // their inlines.
            let mut new_parents = Cow::Borrowed(&s.parents);
            for (index, parent) in s.parents.iter().enumerate() {
                let parent_node = self.resolve_ref(parent.as_ref());
                let parent_index = self.indices[&parent_node];
                if let Some(&inline_ty) = rewrites.get(&parent_index) {
                    new_parents.to_mut()[index] = IrType::Inline(inline_ty.clone());
                }
            }

            if nx != start_index && *new_parents == s.parents {
                continue;
            }

            let p = paths
                .remove(&nx)
                .expect("path must exist for rewritten node");

            let ir_struct = IrStruct {
                description: s.description,
                fields: s.fields.clone(),
                parents: new_parents.into_owned(),
            };

            let inline: &'a InlineIrType<'a> = self.arena.alloc(InlineIrType::Struct(p, ir_struct));
            rewrites.insert(nx, inline);
            result.push(inline);
        }

        result
    }

    /// Finalizes this raw graph into a compact [`IrGraph`].
    pub fn finalize(&self) -> IrGraph<'a> {
        IrGraph::new(self)
    }
}

// Resolve an IrTypeRef to an IrGraphNode, following $ref pointers
// through `spec.schemas`.
fn resolve<'a>(spec: &'a IrSpec<'a>, mut ty: IrTypeRef<'a>) -> IrGraphNode<'a> {
    loop {
        match ty {
            IrTypeRef::Schema(ty) => return IrGraphNode::Schema(ty),
            IrTypeRef::Inline(ty) => return IrGraphNode::Inline(ty),
            IrTypeRef::Ref(r) => {
                // Recursively resolve through the spec's schema map.
                ty = spec.schemas[r.name()].as_ref();
            }
        }
    }
}

/// A graph of all the types in an [`IrSpec`], where each edge
/// is a reference from one type to another.
#[derive(Debug)]
pub struct IrGraph<'a> {
    pub(super) spec: &'a IrSpec<'a>,
    pub(super) g: IrGraphG<'a>,
    /// An inverted index of nodes to graph indices.
    pub(super) indices: FxHashMap<IrGraphNode<'a>, NodeIndex<usize>>,
    /// Maps schema names to their resolved graph nodes.
    schemas: FxHashMap<&'a str, IrGraphNode<'a>>,
    /// Additional metadata for each node.
    pub(super) metadata: IrGraphMetadata<'a>,
}

impl<'a> IrGraph<'a> {
    /// Builds the final compact graph from a [`RawGraph`].
    ///
    /// Copies nodes and edges from the `RawGraph`'s `StableGraph`
    /// into a compact `DiGraph`, remapping indices in the process.
    /// Then computes metadata (SCCs, dependencies, operations).
    pub fn new(raw: &RawGraph<'a>) -> Self {
        let spec = raw.spec;
        let mut g = IrGraphG::default();
        let mut indices = FxHashMap::default();

        let mut schemas: FxHashMap<&'a str, IrGraphNode<'a>> = FxHashMap::default();

        for index in raw.g.node_indices() {
            use std::collections::hash_map::Entry;
            let source = raw.g[index];
            let &mut from = match indices.entry(source) {
                Entry::Occupied(from) => from.into_mut(),
                Entry::Vacant(entry) => {
                    let index = g.add_node(*entry.key());
                    entry.insert(index)
                }
            };
            if let IrGraphNode::Schema(ty) = source {
                schemas.entry(ty.name()).or_insert(source);
            }
            // `raw.g.edges(...)` yields edges in reverse order of addition;
            // reverse them so that they're added to the final graph in order.
            let mut edges = VecDeque::new();
            for edge in raw.g.edges(index) {
                let destination = raw.g[edge.target()];
                let &mut to = match indices.entry(destination) {
                    Entry::Occupied(to) => to.into_mut(),
                    Entry::Vacant(entry) => {
                        let index = g.add_node(*entry.key());
                        entry.insert(index)
                    }
                };
                if let IrGraphNode::Schema(ty) = destination {
                    schemas.entry(ty.name()).or_insert(destination);
                }
                edges.push_front((from, to, *edge.weight()));
            }
            for (from, to, kind) in edges {
                g.add_edge(from, to, kind);
            }
        }

        let sccs = TopoSccs::new(&g);

        let metadata = {
            let mut metadata = IrGraphMetadata {
                scc_indices: {
                    // Precompute SCC indices, using just the reference edges.
                    // Inheritance edges don't contribute to cycles.
                    let refs =
                        EdgeFiltered::from_fn(&g, |e| matches!(e.weight(), EdgeKind::Reference));
                    let mut scc = TarjanScc::new();
                    scc.run(&refs, |_| ());
                    g.node_indices()
                        .map(|node| scc.node_component_index(&refs, node))
                        .collect()
                },
                ..Default::default()
            };

            // Precompute the set of type indices that each operation
            // references directly.
            for op in &spec.operations {
                metadata.operations.entry(ByAddress(op)).or_default().types = op
                    .types()
                    .filter_map(|ty| {
                        let node = match ty.as_ref() {
                            IrTypeRef::Schema(ty) => IrGraphNode::Schema(ty),
                            IrTypeRef::Inline(ty) => IrGraphNode::Inline(ty),
                            IrTypeRef::Ref(r) => schemas[r.name()],
                        };
                        indices.get(&node).map(|node| node.index())
                    })
                    .collect();
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
                    vec![FixedBitSet::with_capacity(g.node_count()); sccs.scc_count()];
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
                    vec![FixedBitSet::with_capacity(g.node_count()); g.node_count()];
                for node in g.node_indices() {
                    let mut deps = scc_deps[sccs.topo_index(node)].clone();
                    deps.remove(node.index()); // We don't depend on ourselves.
                    for dep in deps.ones() {
                        node_dependents[dep].insert(node.index());
                    }
                    metadata.schemas.entry(node).or_default().dependencies = deps;
                }
                for (index, dependents) in node_dependents.into_iter().enumerate() {
                    metadata
                        .schemas
                        .entry(NodeIndex::new(index))
                        .or_default()
                        .dependents = dependents;
                }
            }

            // Backward propagation: propagate each operation to all the
            // types that it uses, directly and transitively.
            for op in &spec.operations {
                let meta = &metadata.operations[&ByAddress(op)];

                // Collect all the types that this operation depends on.
                let mut transitive_deps = FixedBitSet::with_capacity(g.node_count());
                for node in meta.types.ones().map(NodeIndex::new) {
                    transitive_deps.insert(node.index());
                    if let Some(meta) = metadata.schemas.get(&node) {
                        transitive_deps.union_with(&meta.dependencies);
                    }
                }

                // Mark each type as being used by this operation.
                for node in transitive_deps.ones().map(NodeIndex::new) {
                    metadata
                        .schemas
                        .entry(node)
                        .or_default()
                        .used_by
                        .insert(ByAddress(op));
                }
            }

            metadata
        };

        Self {
            spec,
            indices,
            schemas,
            g,
            metadata,
        }
    }

    /// Resolves an [`IrTypeRef`] to an [`IrGraphNode`], following
    /// `$ref` pointers through the schemas map.
    pub fn resolve_type(&self, ty: IrTypeRef<'a>) -> IrGraphNode<'a> {
        match ty {
            IrTypeRef::Schema(ty) => IrGraphNode::Schema(ty),
            IrTypeRef::Inline(ty) => IrGraphNode::Inline(ty),
            IrTypeRef::Ref(r) => self.schemas[r.name()],
        }
    }

    /// Returns the spec used to build this graph.
    #[inline]
    pub fn spec(&self) -> &'a IrSpec<'a> {
        self.spec
    }

    /// Returns an iterator over all the named schemas in this graph.
    #[inline]
    pub fn schemas(&self) -> impl Iterator<Item = SchemaIrTypeView<'_>> {
        self.g
            .node_indices()
            .filter_map(|index| match self.g[index] {
                IrGraphNode::Schema(ty) => Some(SchemaIrTypeView::new(self, index, ty)),
                _ => None,
            })
    }

    /// Returns an iterator over all primitive type nodes in this graph.
    #[inline]
    pub fn primitives(&self) -> impl Iterator<Item = IrPrimitiveView<'_>> {
        self.g
            .node_indices()
            .filter_map(|index| match self.g[index] {
                IrGraphNode::Schema(SchemaIrType::Primitive(_, p))
                | IrGraphNode::Inline(InlineIrType::Primitive(_, p)) => {
                    Some(IrPrimitiveView::new(self, index, *p))
                }
                _ => None,
            })
    }

    /// Returns an iterator over all the operations in this graph.
    #[inline]
    pub fn operations(&self) -> impl Iterator<Item = IrOperationView<'_>> {
        self.spec
            .operations
            .iter()
            .map(move |op| IrOperationView::new(self, op))
    }
}

/// A node in the type graph.
///
/// The derived [`Hash`][std::hash::Hash] and [`Eq`] implementations
/// work on the underlying values, so structurally identical types
/// will be equal. This is important: all types in an [`IrSpec`] are
/// distinct in memory, but can refer to the same logical type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IrGraphNode<'a> {
    Schema(&'a SchemaIrType<'a>),
    Inline(&'a InlineIrType<'a>),
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
pub struct IrGraphMetadata<'a> {
    /// Maps each node index to its strongly connected component index.
    /// Nodes in the same SCC form a cycle.
    pub scc_indices: Vec<usize>,
    pub schemas: FxHashMap<NodeIndex<usize>, IrGraphNodeMeta<'a>>,
    pub operations: FxHashMap<ByAddress<&'a IrOperation<'a>>, IrGraphOperationMeta>,
}

/// Precomputed metadata for an operation that references
/// types in the graph.
#[derive(Debug, Default)]
pub struct IrGraphOperationMeta {
    /// Indices of all the types that this operation directly depends on:
    /// parameters, request body, and response body.
    pub types: FixedBitSet,
}

/// Precomputed metadata for a schema type in the graph.
#[derive(Default)]
pub(super) struct IrGraphNodeMeta<'a> {
    /// Operations that use this type.
    pub used_by: FxHashSet<ByAddress<&'a IrOperation<'a>>>,
    /// Indices of other types that this type transitively depends on.
    pub dependencies: FixedBitSet,
    /// Indices of other types that transitively depend on this type.
    pub dependents: FixedBitSet,
    /// Opaque extended data for this type.
    pub extensions: AtomicRefCell<ExtensionMap>,
}

impl Debug for IrGraphNodeMeta<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IrGraphNodeMeta")
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
                            .map(|field| (EdgeKind::Reference, &field.ty)),
                        ty.parents.iter().map(|parent| (EdgeKind::Inherits, parent)),
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
                                Some((Some(top), EdgeKind::Reference, ty))
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
                        .map(|variant| (Some(top), EdgeKind::Reference, &variant.ty))
                        .rev(),
                );
            }
            IrType::Schema(SchemaIrType::Container(_, container))
            | IrType::Inline(InlineIrType::Container(_, container)) => {
                self.stack
                    .push((Some(top), EdgeKind::Reference, &container.inner().ty));
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
    graph: &'a IrGraphG<'a>,
    stack: VecDeque<(EdgeKind, NodeIndex<usize>)>,
    discovered: EnumMap<EdgeKind, FixedBitSet>,
    direction: Direction,
}

impl<'a> Traverse<'a> {
    pub fn from_roots(
        graph: &'a IrGraphG<'a>,
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
        graph: &'a IrGraphG<'a>,
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
    graph: &IrGraphG<'_>,
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
