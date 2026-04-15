//! Graph-aware views of IR types.
//!
//! Views are cheap, read-only types that pair a node with its cooked graph,
//! so they can answer questions about an IR type and its relationships.
//! The submodules document the OpenAPI concept that each view represents.
//!
//! # The `View` trait
//!
//! All view types implement [`View`], which provides graph traversal methods:
//!
//! * [`View::inlines()`] iterates over inline types nested within this type.
//!   Use this to emit inline type definitions alongside their parent.
//! * [`View::used_by()`] iterates over operations that reference this type.
//!   Useful for generating per-operation imports or feature gates.
//! * [`View::dependencies()`] iterates over all types that this type
//!   transitively depends on. Use this for import lists, feature gates,
//!   and topological ordering.
//! * [`View::dependents()`] iterates over all types that transitively depend on
//!   this type. Useful for impact analysis or invalidation.
//!
//! These methods answer Rust-specific questions:
//!
//! * [`View::hashable()`] returns whether this type can implement `Eq` and `Hash`.
//! * [`View::defaultable()`] returns whether this type can implement `Default`.
//!
//! # Extensions
//!
//! [`ExtendableView`] attaches a type-erased extension map to each view node.
//! Codegen backends use this to store and retrieve arbitrary metadata.

use std::{any::TypeId, fmt::Debug};

use atomic_refcell::{AtomicRef, AtomicRefMut};
use petgraph::{
    graph::NodeIndex,
    visit::{Bfs, EdgeFiltered, EdgeRef},
};
use ref_cast::{RefCastCustom, ref_cast_custom};

use super::{
    graph::{CookedGraph, Extension, ExtensionMap},
    types::GraphType,
};

pub mod any;
pub mod container;
pub mod enum_;
pub mod inline;
pub mod ir;
pub mod operation;
pub mod primitive;
pub mod schema;
pub mod struct_;
pub mod tagged;
pub mod untagged;

use self::{inline::InlineTypeView, ir::TypeView, operation::OperationView};

/// A view of a type in the graph.
pub trait View<'a> {
    /// Returns an iterator over all the inline types that are
    /// contained within this type.
    fn inlines(&self) -> impl Iterator<Item = InlineTypeView<'a>> + use<'a, Self>;

    /// Returns an iterator over the operations that use this type.
    ///
    /// This is backward propagation: each operation depends on this type.
    fn used_by(&self) -> impl Iterator<Item = OperationView<'a>> + use<'a, Self>;

    /// Returns an iterator over all the types that this type transitively depends on.
    /// This is forward propagation: this type depends on each reachable type.
    ///
    /// Complexity: O(n), where `n` is the number of dependency types.
    fn dependencies(&self) -> impl Iterator<Item = TypeView<'a>> + use<'a, Self>;

    /// Returns an iterator over all the types that transitively depend on this type.
    /// This is backward propagation: each returned type depends on this type.
    ///
    /// Complexity: O(n), where `n` is the number of dependent types.
    fn dependents(&self) -> impl Iterator<Item = TypeView<'a>> + use<'a, Self>;

    /// Returns `true` if this type can implement `Eq` and `Hash`.
    fn hashable(&self) -> bool;

    /// Returns `true` if this type can implement `Default`.
    fn defaultable(&self) -> bool;
}

/// A view of a graph type with extended data.
///
/// Codegen backends use extended data to decorate types with extra information.
/// For example, Rust codegen stores a unique identifier on each schema type,
/// so that names never collide after case conversion.
pub trait ExtendableView<'a>: View<'a> {
    /// Returns a reference to this type's extended data.
    fn extensions(&self) -> &ViewExtensions<Self>
    where
        Self: Sized;

    /// Returns a mutable reference to this type's extended data.
    fn extensions_mut(&mut self) -> &mut ViewExtensions<Self>
    where
        Self: Sized;
}

impl<'a, T> View<'a> for T
where
    T: ViewNode<'a>,
{
    #[inline]
    fn inlines(&self) -> impl Iterator<Item = InlineTypeView<'a>> + use<'a, T> {
        let cooked = self.cooked();
        // Follow edges to inline schemas, skipping shadow edges.
        // See `GraphEdge::shadow()` for an explanation.
        let filtered = EdgeFiltered::from_fn(&cooked.graph, move |e| {
            !e.weight().shadow() && matches!(cooked.graph[e.target()], GraphType::Inline(_))
        });
        let mut bfs = Bfs::new(&cooked.graph, self.index());
        std::iter::from_fn(move || bfs.next(&filtered))
            .skip(1) // Skip the starting node.
            .filter_map(|index| match cooked.graph[index] {
                GraphType::Inline(ty) => Some(InlineTypeView::new(cooked, index, ty)),
                _ => None,
            })
    }

    #[inline]
    fn used_by(&self) -> impl Iterator<Item = OperationView<'a>> + use<'a, T> {
        let cooked = self.cooked();
        cooked.metadata.used_by[self.index().index()]
            .iter()
            .map(|op| OperationView::new(cooked, op))
    }

    #[inline]
    fn dependencies(&self) -> impl Iterator<Item = TypeView<'a>> + use<'a, T> {
        let cooked = self.cooked();
        let start = self.index();
        cooked
            .metadata
            .closure
            .dependencies_of(start)
            .filter(move |&index| index != start)
            .map(|index| TypeView::new(cooked, index))
    }

    #[inline]
    fn dependents(&self) -> impl Iterator<Item = TypeView<'a>> + use<'a, T> {
        let cooked = self.cooked();
        let start = self.index();
        cooked
            .metadata
            .closure
            .dependents_of(start)
            .filter(move |&index| index != start)
            .map(|index| TypeView::new(cooked, index))
    }

    #[inline]
    fn hashable(&self) -> bool {
        self.cooked().metadata.hashable[self.index().index()]
    }

    #[inline]
    fn defaultable(&self) -> bool {
        self.cooked().metadata.defaultable[self.index().index()]
    }
}

impl<'a, T> ExtendableView<'a> for T
where
    T: ViewNode<'a>,
{
    #[inline]
    fn extensions(&self) -> &ViewExtensions<Self> {
        ViewExtensions::new(self)
    }

    #[inline]
    fn extensions_mut(&mut self) -> &mut ViewExtensions<Self> {
        ViewExtensions::new_mut(self)
    }
}

pub(crate) trait ViewNode<'a> {
    fn cooked(&self) -> &'a CookedGraph<'a>;
    fn index(&self) -> NodeIndex<usize>;
}

impl<'graph, T> internal::Extendable<'graph> for T
where
    T: ViewNode<'graph>,
{
    #[inline]
    fn ext<'view>(&'view self) -> AtomicRef<'view, ExtensionMap>
    where
        'graph: 'view,
    {
        self.cooked().metadata.extensions[self.index().index()].borrow()
    }

    #[inline]
    fn ext_mut<'b>(&'b mut self) -> AtomicRefMut<'b, ExtensionMap>
    where
        'graph: 'b,
    {
        self.cooked().metadata.extensions[self.index().index()].borrow_mut()
    }
}

/// Extended data attached to a graph type.
#[derive(RefCastCustom)]
#[repr(transparent)]
pub struct ViewExtensions<X>(X);

impl<X> ViewExtensions<X> {
    #[ref_cast_custom]
    fn new(view: &X) -> &Self;

    #[ref_cast_custom]
    fn new_mut(view: &mut X) -> &mut Self;
}

impl<'a, X: internal::Extendable<'a>> ViewExtensions<X> {
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

impl<X> Debug for ViewExtensions<X> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ViewExtensions").finish_non_exhaustive()
    }
}

mod internal {
    use atomic_refcell::{AtomicRef, AtomicRefMut};

    use super::ExtensionMap;

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
}
