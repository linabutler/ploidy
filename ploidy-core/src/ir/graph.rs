use std::{
    any::{Any, TypeId},
    fmt::Debug,
};

use atomic_refcell::AtomicRefCell;
use by_address::ByAddress;
use itertools::Itertools;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::{Bfs, VisitMap, Visitable};
use rustc_hash::{FxHashMap, FxHashSet};

use super::{
    spec::IrSpec,
    types::{
        InlineIrType, IrOperation, IrType, IrTypeRef, IrUntaggedVariant, PrimitiveIrType,
        SchemaIrType,
    },
    views::{operation::IrOperationView, schema::SchemaIrTypeView, wrappers::IrPrimitiveView},
};

/// The type graph.
pub type IrGraphG<'a> = DiGraph<IrGraphNode<'a>, ()>;

/// A graph of all the types in an [`IrSpec`], where each edge
/// is a reference from one type to another.
#[derive(Debug)]
pub struct IrGraph<'a> {
    pub(super) spec: &'a IrSpec<'a>,
    pub(super) g: IrGraphG<'a>,
    /// An inverted index of nodes to graph indices.
    pub(super) indices: FxHashMap<IrGraphNode<'a>, NodeIndex>,
    /// Edges that are part of a cycle.
    pub(super) circular_refs: FxHashSet<(NodeIndex, NodeIndex)>,
    /// Additional metadata for each node.
    pub(super) metadata: FxHashMap<NodeIndex, IrGraphNodeMeta<'a>>,
}

impl<'a> IrGraph<'a> {
    pub fn new(spec: &'a IrSpec<'a>) -> Self {
        let mut g = DiGraph::new();
        let mut indices = FxHashMap::default();

        // All roots (named schemas, parameters, request and response bodies),
        // and all the types within them (inline schemas, wrappers, primitives).
        let tys = IrTypeVisitor::new(
            spec.schemas
                .values()
                .chain(spec.operations.iter().flat_map(|op| op.types())),
        );

        // Add nodes for all types, and edges for references between them.
        for (parent, child) in tys {
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
                g.add_edge(from, to, ());
            }
        }

        // Precompute all circular reference edges, where each edge forms a cycle
        // that requires indirection to break. This speeds up `needs_indirection_to()`:
        // Tarjan's algorithm runs in O(V + E) time over the entire graph; a naive DFS
        // in `needs_indirection_to()` would run in O(N * (V + E)) time, where N is
        // the total number of fields in all structs.
        let circular_refs = {
            let mut edges = FxHashSet::default();
            for scc in tarjan_scc(&g) {
                let scc = FxHashSet::from_iter(scc);
                for &node in &scc {
                    edges.extend(
                        g.neighbors(node)
                            .filter(|neighbor| scc.contains(neighbor))
                            .map(|neighbor| (node, neighbor)),
                    );
                }
            }
            edges
        };

        // Create empty metadata slots for all types.
        let mut metadata = g
            .node_indices()
            .map(|index| (index, IrGraphNodeMeta::default()))
            .collect::<FxHashMap<_, _>>();

        // Precompute a mapping of types to all the operations that use them.
        // This speeds up `used_by()`: precomputing runs in O(P * (V + E)) time,
        // where P is the number of operations; a BFS in `used_by()` would
        // run in O(C * P * (V + E)) time, where C is the number of calls to
        // `used_by()`.
        for op in spec.operations.iter() {
            let stack = op
                .types()
                .map(|ty| IrGraphNode::from_ref(spec, ty.as_ref()))
                .map(|node| indices[&node])
                .collect();
            let mut discovered = g.visit_map();
            for &index in &stack {
                discovered.visit(index);
            }
            let mut bfs = Bfs { stack, discovered };
            while let Some(index) = bfs.next(&g) {
                let meta = metadata.get_mut(&index).unwrap();
                meta.operations.insert(ByAddress(op));
            }
        }

        Self {
            spec,
            indices,
            g,
            circular_refs,
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
    Array(&'a IrType<'a>),
    Map(&'a IrType<'a>),
    Optional(&'a IrType<'a>),
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
            IrTypeRef::Array(ty) => IrGraphNode::Array(ty),
            IrTypeRef::Map(ty) => IrGraphNode::Map(ty),
            IrTypeRef::Optional(ty) => IrGraphNode::Optional(ty),
            IrTypeRef::Ref(r) => Self::from_ref(spec, spec.schemas[r.name()].as_ref()),
            IrTypeRef::Primitive(ty) => IrGraphNode::Primitive(ty),
            IrTypeRef::Any => IrGraphNode::Any,
        }
    }
}

#[derive(Default)]
pub(super) struct IrGraphNodeMeta<'a> {
    /// The set of operations that transitively use this type.
    pub operations: FxHashSet<ByAddress<&'a IrOperation<'a>>>,
    /// Opaque extended data for this type.
    pub extensions: AtomicRefCell<ExtensionMap>,
}

impl Debug for IrGraphNodeMeta<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IrGraphNodeMeta")
            .field("operations", &self.operations)
            .finish_non_exhaustive()
    }
}

/// Visits all the types and references contained within a type.
#[derive(Debug)]
struct IrTypeVisitor<'a> {
    stack: Vec<(Option<&'a IrType<'a>>, &'a IrType<'a>)>,
}

impl<'a> IrTypeVisitor<'a> {
    /// Creates a visitor with `root` on the stack of types to visit.
    #[inline]
    fn new(roots: impl Iterator<Item = &'a IrType<'a>>) -> Self {
        let mut stack = roots.map(|root| (None, root)).collect_vec();
        stack.reverse();
        Self { stack }
    }
}

impl<'a> Iterator for IrTypeVisitor<'a> {
    type Item = (Option<&'a IrType<'a>>, &'a IrType<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        let (parent, top) = self.stack.pop()?;
        match top {
            IrType::Array(ty) => {
                self.stack.push((Some(top), ty.as_ref()));
            }
            IrType::Map(ty) => {
                self.stack.push((Some(top), ty.as_ref()));
            }
            IrType::Optional(ty) => {
                self.stack.push((Some(top), ty.as_ref()));
            }
            IrType::Schema(SchemaIrType::Struct(_, ty)) => {
                self.stack
                    .extend(ty.fields.iter().map(|field| (Some(top), &field.ty)).rev());
            }
            IrType::Schema(SchemaIrType::Untagged(_, ty)) => {
                self.stack.extend(
                    ty.variants
                        .iter()
                        .filter_map(|variant| match variant {
                            IrUntaggedVariant::Some(_, ty) => Some((Some(top), ty)),
                            _ => None,
                        })
                        .rev(),
                );
            }
            IrType::Schema(SchemaIrType::Tagged(_, ty)) => {
                self.stack.extend(
                    ty.variants
                        .iter()
                        .map(|variant| (Some(top), &variant.ty))
                        .rev(),
                );
            }
            IrType::Schema(SchemaIrType::Enum(..)) => (),
            IrType::Any => (),
            IrType::Primitive(_) => (),
            IrType::Inline(ty) => match ty {
                InlineIrType::Enum(..) => (),
                InlineIrType::Tagged(_, ty) => {
                    self.stack.extend(
                        ty.variants
                            .iter()
                            .map(|variant| (Some(top), &variant.ty))
                            .rev(),
                    );
                }
                InlineIrType::Untagged(_, ty) => {
                    self.stack.extend(
                        ty.variants
                            .iter()
                            .filter_map(|variant| match variant {
                                IrUntaggedVariant::Some(_, ty) => Some((Some(top), ty)),
                                _ => None,
                            })
                            .rev(),
                    );
                }
                InlineIrType::Struct(_, ty) => {
                    self.stack
                        .extend(ty.fields.iter().map(|field| (Some(top), &field.ty)).rev());
                }
            },
            IrType::Ref(_) => (),
        }
        Some((parent, top))
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
