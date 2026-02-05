use std::{
    any::{Any, TypeId},
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
    visit::{EdgeFiltered, EdgeRef, IntoNeighbors, NodeCount, VisitMap, Visitable},
};
use rustc_hash::{FxHashMap, FxHashSet};

use super::{
    spec::IrSpec,
    types::{
        InlineIrType, IrOperation, IrType, IrTypeRef, IrUntaggedVariant, PrimitiveIrType,
        SchemaIrType,
    },
    views::{operation::IrOperationView, primitive::IrPrimitiveView, schema::SchemaIrTypeView},
};

/// The type graph.
pub(super) type IrGraphG<'a> = DiGraph<IrGraphNode<'a>, EdgeKind, usize>;

/// A graph of all the types in an [`IrSpec`], where each edge
/// is a reference from one type to another.
#[derive(Debug)]
pub struct IrGraph<'a> {
    pub(super) spec: &'a IrSpec<'a>,
    pub(super) g: IrGraphG<'a>,
    /// An inverted index of nodes to graph indices.
    pub(super) indices: FxHashMap<IrGraphNode<'a>, NodeIndex<usize>>,
    /// Additional metadata for each node.
    pub(super) metadata: IrGraphMetadata<'a>,
}

impl<'a> IrGraph<'a> {
    pub fn new(spec: &'a IrSpec<'a>) -> Self {
        let mut g = IrGraphG::default();
        let mut indices = FxHashMap::default();

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
            let &mut to = match indices.entry(IrGraphNode::from_ref(spec, child.as_ref())) {
                // We might see the same schema multiple times, if it's
                // referenced multiple times in the spec. Only add a new node
                // for the schema if we haven't seen it before.
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let index = g.add_node(*entry.key());
                    entry.insert(index)
                }
            };
            if let Some(parent) = parent {
                let &mut from = match indices.entry(IrGraphNode::from_ref(spec, parent.as_ref())) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => {
                        let index = g.add_node(*entry.key());
                        entry.insert(index)
                    }
                };
                // Add a directed edge from parent to child.
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
                        indices
                            .get(&IrGraphNode::from_ref(spec, ty.as_ref()))
                            .map(|node| node.index())
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

                // Expand SCC-level dependencies to node-level: for each SCC,
                // form a union of all nodes from all the SCCs it depends on.
                let mut deps_by_scc =
                    vec![FixedBitSet::with_capacity(g.node_count()); condensation.node_count()];
                for scc_index in condensation.node_indices() {
                    for dep_scc_index in closure.neighbors(scc_index) {
                        deps_by_scc[scc_index].union_with(sccs.members(dep_scc_index));
                    }
                    // Include the other members of this SCC; these depend on
                    // each other because they're in a cycle.
                    deps_by_scc[scc_index].union_with(sccs.members(scc_index));
                }

                for node in g.node_indices() {
                    let topo_index = sccs.topo_index(node);
                    let mut deps = deps_by_scc[topo_index].clone();

                    // Exclude ourselves from our dependencies and dependents.
                    deps.remove(node.index());

                    metadata
                        .schemas
                        .entry(node)
                        .or_default()
                        .dependencies
                        .union_with(&deps);

                    // Add ourselves to the dependents of all the types
                    // that we depend on.
                    for index in deps.into_ones().map(NodeIndex::new) {
                        metadata
                            .schemas
                            .entry(index)
                            .or_default()
                            .dependents
                            .grow_and_insert(node.index());
                    }
                }
            }

            // Backward propagation: propagate each operation to all the types
            // that it uses, directly and transitively.
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
                for index in transitive_deps.ones().map(NodeIndex::new) {
                    metadata
                        .schemas
                        .entry(index)
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
            g,
            metadata,
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

    /// Returns an iterator over all the primitive types in this graph. Note that
    /// a graph contains at most one instance of each primitive type.
    #[inline]
    pub fn primitives(&self) -> impl Iterator<Item = IrPrimitiveView<'_>> {
        self.g
            .node_indices()
            .filter_map(|index| match self.g[index] {
                IrGraphNode::Primitive(ty) => Some(IrPrimitiveView::new(self, index, ty)),
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
    Primitive(PrimitiveIrType),
    Any,
}

impl<'a> IrGraphNode<'a> {
    /// Converts an [`IrTypeRef`] to an [`IrGraphNode`],
    /// recursively resolving referenced schemas.
    pub fn from_ref(spec: &'a IrSpec<'a>, ty: IrTypeRef<'a>) -> Self {
        match ty {
            IrTypeRef::Schema(ty) => IrGraphNode::Schema(ty),
            IrTypeRef::Inline(ty) => IrGraphNode::Inline(ty),
            IrTypeRef::Ref(r) => Self::from_ref(spec, spec.schemas[r.name()].as_ref()),
            IrTypeRef::Primitive(ty) => IrGraphNode::Primitive(ty),
            IrTypeRef::Any => IrGraphNode::Any,
        }
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
            IrType::Schema(SchemaIrType::Enum(..)) | IrType::Inline(InlineIrType::Enum(..)) => (),
            IrType::Any => (),
            IrType::Primitive(_) => (),
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

    /// Returns the topological index of the SCC that contains the given node.
    #[inline]
    fn topo_index(&self, node: NodeIndex<usize>) -> usize {
        // Tarjan's algorithm returns indices in reverse topological order;
        // inverting the component index gets us the topological index.
        self.sccs.len() - 1 - self.tarjan.node_component_index(self.graph, node)
    }

    /// Returns the members of the SCC at the given topological index.
    #[inline]
    fn members(&self, index: usize) -> &FixedBitSet {
        &self.sccs[index]
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
        let scc_count = self.sccs.len();
        let mut dag = UnweightedList::with_capacity(scc_count);
        for to in 0..scc_count {
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
