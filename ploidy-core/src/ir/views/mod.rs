//! Graph-aware views of IR types.
//!
//! These views provide a representation of schema and inline types
//! for code generation. Each view decorates an IR type with
//! additional information from the type graph.

use std::{any::TypeId, collections::VecDeque, fmt::Debug};

use atomic_refcell::{AtomicRef, AtomicRefMut};
use petgraph::{
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
    fn inlines(&self) -> impl Iterator<Item = InlineIrTypeView<'a>>;

    /// Returns an iterator over all the operations that directly or transitively
    /// use this type.
    fn used_by(&self) -> impl Iterator<Item = IrOperationView<'a>>;

    /// Returns an iterator over all the types that are reachable from this type.
    fn reachable(&self) -> impl Iterator<Item = IrTypeView<'a>>;

    /// Returns an iterator over all reachable types, with a `filter` function
    /// to control the traversal.
    fn reachable_if<F>(&self, filter: F) -> impl Iterator<Item = IrTypeView<'a>>
    where
        F: Fn(&IrTypeView<'a>) -> Traversal;

    /// Returns a reference to this type's extended data.
    fn extensions(&self) -> &IrViewExtensions<Self>
    where
        Self: Extendable<'a> + Sized;

    /// Returns a mutable reference to this type's extended data.
    fn extensions_mut(&mut self) -> &mut IrViewExtensions<Self>
    where
        Self: Extendable<'a> + Sized;
}

impl<'a, T> View<'a> for T
where
    T: ViewNode<'a>,
{
    #[inline]
    fn inlines(&self) -> impl Iterator<Item = InlineIrTypeView<'a>> {
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
    fn used_by(&self) -> impl Iterator<Item = IrOperationView<'a>> {
        self.graph()
            .metadata
            .get(&self.index())
            .into_iter()
            .flat_map(|meta| &meta.operations)
            .map(|op| IrOperationView::new(self.graph(), op))
    }

    #[inline]
    fn reachable(&self) -> impl Iterator<Item = IrTypeView<'a>> {
        let graph = self.graph();
        let mut bfs = Bfs::new(&graph.g, self.index());
        std::iter::from_fn(move || bfs.next(&graph.g)).map(|index| IrTypeView::new(graph, index))
    }

    #[inline]
    fn reachable_if<F>(&self, filter: F) -> impl Iterator<Item = IrTypeView<'a>>
    where
        F: Fn(&IrTypeView<'a>) -> Traversal,
    {
        let graph = self.graph();
        let mut stack = VecDeque::new();
        let mut discovered = graph.g.visit_map();

        stack.push_back(self.index());
        discovered.visit(self.index());

        std::iter::from_fn(move || {
            while let Some(index) = stack.pop_front() {
                let view = IrTypeView::new(graph, index);
                let traversal = filter(&view);

                if matches!(traversal, Traversal::Visit | Traversal::Skip) {
                    // Add the neighbors to the stack of nodes to visit.
                    for neighbor in graph.g.neighbors(index) {
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
    fn index(&self) -> NodeIndex;
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
        self.graph().metadata[&self.index()].extensions.borrow()
    }

    #[inline]
    fn ext_mut<'b>(&'b mut self) -> AtomicRefMut<'b, ExtensionMap>
    where
        'graph: 'b,
    {
        self.graph().metadata[&self.index()].extensions.borrow_mut()
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
