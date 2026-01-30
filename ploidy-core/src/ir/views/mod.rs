//! Graph-aware views of IR types.
//!
//! These views provide a representation of schema and inline types
//! for code generation. Each view decorates an IR type with
//! additional information from the type graph.

use std::{any::TypeId, collections::VecDeque, fmt::Debug};

use atomic_refcell::{AtomicRef, AtomicRefMut};
use petgraph::{
    Direction,
    graph::NodeIndex,
    visit::{Bfs, EdgeFiltered, EdgeRef, VisitMap, Visitable},
};
use ref_cast::{RefCastCustom, ref_cast_custom};

use super::graph::{Extension, ExtensionMap, IrGraph, IrGraphNode};

pub mod enum_;
pub mod inline;
pub mod ir;
pub mod operation;
pub mod schema;
pub mod struct_;
pub mod tagged;
pub mod untagged;
pub mod wrappers;

use self::{inline::InlineIrTypeView, ir::IrTypeView, operation::IrOperationView};

/// A view of a type in the graph.
pub trait View<'a> {
    /// Returns an iterator over all the inline types that are
    /// contained within this type.
    fn inlines(&self) -> impl Iterator<Item = InlineIrTypeView<'a>> + use<'a, Self>;

    /// Returns an iterator over the operations that use this type.
    ///
    /// This is backward propagation: each operation depends on this type.
    fn used_by(&self) -> impl Iterator<Item = IrOperationView<'a>> + use<'a, Self>;

    /// Returns an iterator over all the types that this type transitively depends on.
    /// This is forward propagation: this type depends on each reachable type.
    ///
    /// Complexity: O(n), where `n` is the number of dependency types.
    fn dependencies(&self) -> impl Iterator<Item = IrTypeView<'a>> + use<'a, Self>;

    /// Returns an iterator over all the types that transitively depend on this type.
    /// This is backward propagation: each returned type depends on this type.
    ///
    /// Complexity: O(n), where `n` is the number of dependent types.
    fn dependents(&self) -> impl Iterator<Item = IrTypeView<'a>> + use<'a, Self>;

    /// Walks the type graph toward dependencies or dependents, using a
    /// `filter` function to control the traversal.
    ///
    /// Complexity: O(V + E), where `V` and `E` are the number of
    /// nodes and edges in the visited subgraph.
    fn traverse<F>(
        &self,
        reach: Reach,
        filter: F,
    ) -> impl Iterator<Item = IrTypeView<'a>> + use<'a, Self, F>
    where
        F: Fn(&IrTypeView<'a>) -> Traversal;
}

pub trait ExtendableView<'a>: View<'a> {
    /// Returns a reference to this type's extended data.
    fn extensions(&self) -> &IrViewExtensions<Self>
    where
        Self: Sized;

    /// Returns a mutable reference to this type's extended data.
    fn extensions_mut(&mut self) -> &mut IrViewExtensions<Self>
    where
        Self: Sized;
}

impl<'a, T> View<'a> for T
where
    T: ViewNode<'a>,
{
    #[inline]
    fn inlines(&self) -> impl Iterator<Item = InlineIrTypeView<'a>> + use<'a, T> {
        let graph = self.graph();
        // Exclude edges that reference other schemas.
        let filtered = EdgeFiltered::from_fn(&graph.g, |r| {
            !matches!(graph.g[r.target()], IrGraphNode::Schema(_))
        });
        let mut bfs = Bfs::new(&graph.g, self.index());
        std::iter::from_fn(move || bfs.next(&filtered)).filter_map(|index| match graph.g[index] {
            IrGraphNode::Inline(ty) => Some(InlineIrTypeView::new(graph, index, ty)),
            _ => None,
        })
    }

    #[inline]
    fn used_by(&self) -> impl Iterator<Item = IrOperationView<'a>> + use<'a, T> {
        let graph = self.graph();
        graph
            .metadata
            .schemas
            .get(&self.index())
            .into_iter()
            .flat_map(|meta| {
                meta.used_by
                    .iter()
                    .map(|op| IrOperationView::new(graph, op.0))
            })
    }

    #[inline]
    fn dependencies(&self) -> impl Iterator<Item = IrTypeView<'a>> + use<'a, T> {
        let graph = self.graph();
        graph
            .metadata
            .schemas
            .get(&self.index())
            .into_iter()
            .flat_map(|meta| meta.dependencies.ones())
            .map(NodeIndex::new)
            .map(|index| IrTypeView::new(graph, index))
    }

    #[inline]
    fn dependents(&self) -> impl Iterator<Item = IrTypeView<'a>> + use<'a, T> {
        let graph = self.graph();
        graph
            .metadata
            .schemas
            .get(&self.index())
            .into_iter()
            .flat_map(|meta| meta.dependents.ones())
            .map(NodeIndex::new)
            .map(move |index| IrTypeView::new(graph, index))
    }

    #[inline]
    fn traverse<F>(
        &self,
        reach: Reach,
        filter: F,
    ) -> impl Iterator<Item = IrTypeView<'a>> + use<'a, T, F>
    where
        F: Fn(&IrTypeView<'a>) -> Traversal,
    {
        let graph = self.graph();
        let mut stack = VecDeque::new();
        let mut discovered = graph.g.visit_map();
        let direction = match reach {
            Reach::Dependencies => Direction::Outgoing,
            Reach::Dependents => Direction::Incoming,
        };

        // Mark ourselves as discovered, but don't add ourselves to the queue,
        // since our dependencies and dependents shouldn't include ourselves.
        discovered.visit(self.index());
        for neighbor in graph.g.neighbors_directed(self.index(), direction) {
            if discovered.visit(neighbor) {
                stack.push_back(neighbor);
            }
        }

        std::iter::from_fn(move || {
            while let Some(index) = stack.pop_front() {
                let view = IrTypeView::new(graph, index);
                let traversal = filter(&view);

                if matches!(traversal, Traversal::Visit | Traversal::Skip) {
                    // Add the neighbors to the stack of nodes to visit.
                    for neighbor in graph.g.neighbors_directed(index, direction) {
                        if discovered.visit(neighbor) {
                            stack.push_back(neighbor);
                        }
                    }
                }

                if matches!(traversal, Traversal::Visit | Traversal::Stop) {
                    // Yield this node.
                    return Some(view);
                }

                // (`Skip` and `Ignore` continue the loop without yielding).
            }
            None
        })
    }
}

impl<'a, T> ExtendableView<'a> for T
where
    T: ViewNode<'a>,
{
    #[inline]
    fn extensions(&self) -> &IrViewExtensions<Self> {
        IrViewExtensions::new(self)
    }

    #[inline]
    fn extensions_mut(&mut self) -> &mut IrViewExtensions<Self> {
        IrViewExtensions::new_mut(self)
    }
}

pub trait ViewNode<'a> {
    fn graph(&self) -> &'a IrGraph<'a>;
    fn index(&self) -> NodeIndex<usize>;
}

pub trait Extendable<'graph> {
    // These lifetime requirements might look redundant, but they're not:
    // we're shortening the lifetime of the `AtomicRef` from `'graph` to `'view`,
    // to prevent overlapping mutable borrows of the underlying `AtomicRefCell`
    // at compile time.
    //
    // (`AtomicRefCell` panics on these illegal borrows at runtime, which is
    // always memory-safe; we just want some extra type safety).
    //
    // This approach handles the obvious case of overlapping borrows from
    // `ext()` and `ext_mut()`, and the `AtomicRefCell` avoids plumbing
    // mutable references to the graph through every IR layer.

    fn ext<'view>(&'view self) -> AtomicRef<'view, ExtensionMap>
    where
        'graph: 'view;

    fn ext_mut<'view>(&'view mut self) -> AtomicRefMut<'view, ExtensionMap>
    where
        'graph: 'view;
}

impl<'graph, T> Extendable<'graph> for T
where
    T: ViewNode<'graph>,
{
    #[inline]
    fn ext<'view>(&'view self) -> AtomicRef<'view, ExtensionMap>
    where
        'graph: 'view,
    {
        self.graph().metadata.schemas[&self.index()]
            .extensions
            .borrow()
    }

    #[inline]
    fn ext_mut<'b>(&'b mut self) -> AtomicRefMut<'b, ExtensionMap>
    where
        'graph: 'b,
    {
        self.graph().metadata.schemas[&self.index()]
            .extensions
            .borrow_mut()
    }
}

/// Extended data attached to a type in the graph.
///
/// Generators can use extended data to decorate types with extra information,
/// like name mappings. For example, the Rust generator stores a normalized,
/// deduplicated identifier name on every named schema type.
#[derive(RefCastCustom)]
#[repr(transparent)]
pub struct IrViewExtensions<X>(X);

impl<X> IrViewExtensions<X> {
    #[ref_cast_custom]
    fn new(view: &X) -> &Self;

    #[ref_cast_custom]
    fn new_mut(view: &mut X) -> &mut Self;
}

impl<'a, X: Extendable<'a>> IrViewExtensions<X> {
    /// Returns a reference to a value of an arbitrary type that was
    /// previously inserted into this extended data.
    #[inline]
    pub fn get<'b, T: Send + Sync + 'static>(&'b self) -> Option<AtomicRef<'b, T>>
    where
        'a: 'b,
    {
        AtomicRef::filter_map(self.0.ext(), |ext| {
            Some(
                ext.get(&TypeId::of::<T>())?
                    .as_ref()
                    .downcast_ref::<T>()
                    .unwrap(),
            )
        })
    }

    /// Inserts a value of an arbitrary type into this extended data,
    /// and returns the previous value for that type.
    #[inline]
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) -> Option<T> {
        self.0
            .ext_mut()
            .insert(TypeId::of::<T>(), Box::new(value))
            .and_then(|old| *Extension::into_inner(old).downcast().unwrap())
    }
}

impl<X> Debug for IrViewExtensions<X> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("IrViewExtensions").finish_non_exhaustive()
    }
}

/// Selects which edge direction to follow during traversal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reach {
    /// Traverse out toward types that this node depends on.
    Dependencies,
    /// Traverse in toward types that depend on this node.
    Dependents,
}

/// Controls how to continue traversing the graph when at a node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Traversal {
    /// Yield this node, then continue into its neighbors.
    Visit,
    /// Yield this node, but don't continue into its neighbors.
    Stop,
    /// Don't yield this node, but continue into its neighbors.
    Skip,
    /// Don't yield this node, and don't continue into its neighbors.
    Ignore,
}
