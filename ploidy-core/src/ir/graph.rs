use std::collections::BTreeSet;
use std::ops::Deref;

use by_address::ByAddress;
use indexmap::{IndexMap, IndexSet};
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::{Bfs, EdgeFiltered, EdgeRef, VisitMap, Visitable};

use crate::ir::IrTypeRef;

use super::{
    spec::IrSpec,
    types::{InlineIrType, IrOperation, IrType, IrUntaggedVariant, PrimitiveIrType, SchemaIrType},
};

/// The reference graph.
type Refs<'a> = DiGraph<IrGraphNode<'a>, ()>;

/// A node in the reference graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IrGraphNode<'a> {
    Schema(&'a SchemaIrType<'a>),
    Inline(&'a InlineIrType<'a>),
    Array(&'a IrType<'a>),
    Map(&'a IrType<'a>),
    Nullable(&'a IrType<'a>),
    Primitive(PrimitiveIrType),
    Any,
}

impl<'a> IrGraphNode<'a> {
    /// Converts an [`IrType`] to an [`IrGraphNode`],
    /// recursively resolving referenced schemas.
    pub fn from_ty(spec: &'a IrSpec<'a>, ty: IrTypeRef<'a>) -> Self {
        match ty {
            IrTypeRef::Schema(ty) => IrGraphNode::Schema(ty),
            IrTypeRef::Inline(ty) => IrGraphNode::Inline(ty),
            IrTypeRef::Array(ty) => IrGraphNode::Array(ty),
            IrTypeRef::Map(ty) => IrGraphNode::Map(ty),
            IrTypeRef::Nullable(ty) => IrGraphNode::Nullable(ty),
            IrTypeRef::Ref(r) => Self::from_ty(spec, spec.schemas[r.name()].as_ref()),
            IrTypeRef::Primitive(ty) => IrGraphNode::Primitive(ty),
            IrTypeRef::Any => IrGraphNode::Any,
        }
    }

    pub fn to_ref(self) -> IrTypeRef<'a> {
        match self {
            Self::Schema(ty) => IrTypeRef::Schema(ty),
            Self::Inline(ty) => IrTypeRef::Inline(ty),
            Self::Array(ty) => IrTypeRef::Array(ty),
            Self::Map(ty) => IrTypeRef::Map(ty),
            Self::Nullable(ty) => IrTypeRef::Nullable(ty),
            Self::Primitive(ty) => IrTypeRef::Primitive(ty),
            Self::Any => IrTypeRef::Any,
        }
    }
}

/// A graph of all types in an [`IrSpec`], where each arc
/// is a reference from one type to another.
#[derive(Debug)]
pub struct IrGraph<'a> {
    spec: &'a IrSpec<'a>,
    refs: Refs<'a>,
    /// An inverted mapping of nodes to graph indices.
    nodes: IndexMap<IrGraphNode<'a>, NodeIndex>,
    /// Edges that are part of a cycle.
    circular_refs: BTreeSet<(NodeIndex, NodeIndex)>,
    /// An inverted mapping of nodes to the set of operations
    /// that transitively use them.
    ops: IndexMap<NodeIndex, IndexSet<ByAddress<&'a IrOperation<'a>>>>,
}

impl<'a> IrGraph<'a> {
    pub fn new(spec: &'a IrSpec<'a>) -> Self {
        let mut nodes = IndexMap::new();
        let mut refs = DiGraph::new();

        // All root components: named schemas, parameters,
        // request and response bodies.
        let tys = itertools::chain!(
            spec.schemas.values(),
            spec.operations.iter().flat_map(|op| op.types()),
        );

        // Visit all types within those root components:
        // inline schemas, wrappers, and leaf types.
        for (parent, child) in tys.flat_map(Visitor::new) {
            use indexmap::map::Entry;
            let &mut to = match nodes.entry(IrGraphNode::from_ty(spec, child.as_ref())) {
                // We might see the same schema multiple times,
                // if it's referenced multiple times in the spec.
                // Only add a new node if we haven't seen it yet.
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let index = refs.add_node(*entry.key());
                    entry.insert(index)
                }
            };
            if let Some(parent) = parent {
                let &mut from = match nodes.entry(IrGraphNode::from_ty(spec, parent.as_ref())) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => {
                        let index = refs.add_node(*entry.key());
                        entry.insert(index)
                    }
                };
                // Add a directed edge from parent to child.
                refs.add_edge(from, to, ());
            }
        }

        // Find all circular reference edges in this graph, where each edge
        // forms a cycle that requires indirection to break. It's much better
        // to run Tarjan's algorithm over the graph once, than a DFS for
        // every field of every struct, to detect cyclic edges.
        let circular_refs = {
            let mut edges = BTreeSet::new();
            for scc in tarjan_scc(&refs) {
                let scc = BTreeSet::from_iter(scc);
                for &node in &scc {
                    edges.extend(
                        refs.neighbors(node)
                            .filter(|neighbor| scc.contains(neighbor))
                            .map(|neighbor| (node, neighbor)),
                    );
                }
            }
            edges
        };

        // Build a reverse index of types to all the operations that use them,
        // for faster lookup.
        let mut ops = IndexMap::<_, IndexSet<_>>::new();
        for op in spec.operations.iter() {
            let stack = op
                .types()
                .map(|ty| IrGraphNode::from_ty(spec, ty.as_ref()))
                .map(|node| nodes[&node])
                .collect();
            let mut discovered = refs.visit_map();
            for &index in &stack {
                discovered.visit(index);
            }
            let mut bfs = Bfs { stack, discovered };
            while let Some(index) = bfs.next(&refs) {
                ops.entry(index).or_default().insert(ByAddress(op));
            }
        }

        Self {
            spec,
            nodes,
            refs,
            circular_refs,
            ops,
        }
    }

    pub fn spec(&self) -> &'a IrSpec<'a> {
        self.spec
    }

    /// Looks up a schema by name, and returns a view for that schema.
    #[inline]
    pub fn lookup(&self, ty: IrTypeRef<'a>) -> Option<IrTypeView<'_>> {
        let ty = IrGraphNode::from_ty(self.spec, ty);
        self.nodes
            .get(&ty)
            .map(|&index| IrTypeView { graph: self, index })
    }

    /// Returns an iterator over all named schemas in this graph.
    #[inline]
    pub fn schemas(&self) -> impl Iterator<Item = (&'a str, IrTypeView<'_>)> {
        self.spec.schemas.iter().map(|(&name, ty)| {
            let ty = IrGraphNode::from_ty(self.spec, ty.as_ref());
            let index = self.nodes[&ty];
            (name, IrTypeView { graph: self, index })
        })
    }

    /// Returns an iterator over all operations in this graph.
    #[inline]
    pub fn operations(&self) -> impl Iterator<Item = IrOperationView<'_>> {
        self.spec
            .operations
            .iter()
            .map(move |op| IrOperationView { graph: self, op })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct IrOperationView<'a> {
    graph: &'a IrGraph<'a>,
    op: &'a IrOperation<'a>,
}

impl<'a> IrOperationView<'a> {
    #[inline]
    pub fn op(self) -> &'a IrOperation<'a> {
        self.op
    }

    /// Returns an iterator over all the inline types that are
    /// contained within this operation's referenced types.
    pub fn inlines(self) -> impl Iterator<Item = &'a InlineIrType<'a>> {
        // Exclude edges that reference other schemas.
        let filtered = EdgeFiltered::from_fn(&self.graph.refs, |r| {
            !matches!(self.graph.refs[r.target()], IrGraphNode::Schema(_))
        });
        let mut bfs = self.bfs();
        std::iter::from_fn(move || bfs.next(&filtered)).filter_map(move |index| {
            match self.graph.refs[index] {
                IrGraphNode::Inline(ty) => Some(ty),
                _ => None,
            }
        })
    }

    fn bfs(self) -> Bfs<NodeIndex, <Refs<'a> as Visitable>::Map> {
        // `Bfs::new()` starts with just one root on the stack,
        // but operations aren't roots; they reference types that are roots,
        // so we construct our own visitor with all those roots on the stack.
        let stack = self
            .op
            .types()
            .map(|ty| IrGraphNode::from_ty(self.graph.spec, ty.as_ref()))
            .map(|node| self.graph.nodes[&node])
            .collect();
        let mut discovered = self.graph.refs.visit_map();
        for &index in &stack {
            discovered.visit(index);
        }
        Bfs { stack, discovered }
    }
}

/// A view into any node in the type graph
#[derive(Clone, Copy, Debug)]
pub struct IrTypeView<'a> {
    graph: &'a IrGraph<'a>,
    index: NodeIndex,
}

impl<'a> Deref for IrTypeView<'a> {
    type Target = IrGraphNode<'a>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.graph.refs[self.index]
    }
}

impl<'a> IrTypeView<'a> {
    /// Extracts a named schema type from this view, if it's a [`SchemaIrType`].
    #[inline]
    pub fn as_schema(self) -> Option<&'a SchemaIrType<'a>> {
        match self.graph.refs[self.index] {
            IrGraphNode::Schema(s) => Some(s),
            _ => None,
        }
    }

    /// Extracts an inline type from this view, if it's an [`InlineIrType`].
    #[inline]
    pub fn as_inline(self) -> Option<&'a InlineIrType<'a>> {
        match self.graph.refs[self.index] {
            IrGraphNode::Inline(i) => Some(i),
            _ => None,
        }
    }

    /// Returns `true` if a reference from this node to the `other` node
    /// requires indirection (with [`Box`], [`Vec`], etc.)
    #[inline]
    pub fn requires_indirection_to(&self, other: &IrTypeView<'_>) -> bool {
        self.graph
            .circular_refs
            .contains(&(self.index, other.index))
    }

    /// Returns an iterator over all the inline types that are
    /// contained within this type.
    #[inline]
    pub fn inlines(self) -> impl Iterator<Item = &'a InlineIrType<'a>> {
        // Exclude edges that reference other schemas.
        let filtered = EdgeFiltered::from_fn(&self.graph.refs, |r| {
            !matches!(self.graph.refs[r.target()], IrGraphNode::Schema(_))
        });
        let mut bfs = Bfs::new(&self.graph.refs, self.index);
        std::iter::from_fn(move || bfs.next(&filtered)).filter_map(move |index| {
            match self.graph.refs[index] {
                IrGraphNode::Inline(ty) => Some(ty),
                _ => None,
            }
        })
    }

    /// Returns an iterator over all the types that are reachable from this type.
    #[inline]
    pub fn reachable(self) -> impl Iterator<Item = IrTypeView<'a>> {
        let mut bfs = Bfs::new(&self.graph.refs, self.index);
        std::iter::from_fn(move || bfs.next(&self.graph.refs)).map(|index| IrTypeView {
            graph: self.graph,
            index,
        })
    }

    /// Returns an iterator over all the operations that directly or transitively
    /// use this type.
    #[inline]
    pub fn used_by(self) -> impl Iterator<Item = IrOperationView<'a>> {
        self.graph
            .ops
            .get(&self.index)
            .into_iter()
            .flatten()
            .map(move |op| IrOperationView {
                graph: self.graph,
                op,
            })
    }
}

#[derive(Debug)]
pub struct Visitor<'a> {
    stack: Vec<(Option<&'a IrType<'a>>, &'a IrType<'a>)>,
}

impl<'a> Visitor<'a> {
    #[inline]
    pub fn new(root: &'a IrType<'a>) -> Self {
        Self {
            stack: vec![(None, root)],
        }
    }
}

impl<'a> Iterator for Visitor<'a> {
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
            IrType::Nullable(ty) => {
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
                self.stack
                    .extend(ty.variants.iter().map(|name| (Some(top), &name.ty)).rev());
            }
            IrType::Schema(SchemaIrType::Enum(..)) => (),
            IrType::Any => (),
            &IrType::Primitive(_) => (),
            IrType::Inline(ty) => match ty {
                InlineIrType::Enum(..) => (),
                InlineIrType::Untagged(_, ty) => self.stack.extend(
                    ty.variants
                        .iter()
                        .filter_map(|variant| match variant {
                            IrUntaggedVariant::Some(_, ty) => Some((Some(top), ty)),
                            _ => None,
                        })
                        .rev(),
                ),
                InlineIrType::Struct(_, ty) => self
                    .stack
                    .extend(ty.fields.iter().map(|field| (Some(top), &field.ty)).rev()),
            },
            &IrType::Ref(_) => (),
        }
        Some((parent, top))
    }
}
