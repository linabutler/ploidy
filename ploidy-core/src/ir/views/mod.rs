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
//! * [`View::traverse()`] traverses the graph breadth-first with a filter that
//!   controls which nodes to yield and explore.
//!
//! # Extensions
//!
//! [`ExtendableView`] attaches a type-erased extension map to each view node.
//! Codegen backends use this to store and retrieve arbitrary metadata.

use std::{any::TypeId, fmt::Debug};

use atomic_refcell::{AtomicRef, AtomicRefMut};
use petgraph::{
    Direction,
    graph::NodeIndex,
    visit::{Bfs, EdgeFiltered, EdgeRef},
};
use ref_cast::{RefCastCustom, ref_cast_custom};

use super::{
    graph::{CookedGraph, EdgeKind, Extension, ExtensionMap, Traversal, Traverse},
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

    /// Traverses this type's dependencies or dependents breadth-first,
    /// using `filter` to control which nodes are yielded and explored.
    ///
    /// The filter receives the [`EdgeKind`] describing how the node
    /// was reached, and returns a [`Traversal`].
    ///
    /// A node reachable via multiple edge kinds may be yielded more
    /// than once, once per distinct edge kind.
    ///
    /// Complexity: O(V + E) over the visited subgraph.
    fn traverse<F>(
        &self,
        reach: Reach,
        filter: F,
    ) -> impl Iterator<Item = (EdgeKind, TypeView<'a>)> + use<'a, Self, F>
    where
        F: Fn(EdgeKind, &TypeView<'a>) -> Traversal;
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
        // Only include edges to other inline schemas.
        let filtered = EdgeFiltered::from_fn(&cooked.graph, |e| {
            matches!(cooked.graph[e.target()], GraphType::Inline(_))
        });
        let mut bfs = Bfs::new(&cooked.graph, self.index());
        std::iter::from_fn(move || bfs.next(&filtered)).filter_map(|index| {
            match cooked.graph[index] {
                GraphType::Inline(ty) => Some(InlineTypeView::new(cooked, index, ty)),
                _ => None,
            }
        })
    }

    #[inline]
    fn used_by(&self) -> impl Iterator<Item = OperationView<'a>> + use<'a, T> {
        let cooked = self.cooked();
        let meta = &cooked.metadata.schemas[self.index().index()];
        meta.used_by.iter().map(|op| OperationView::new(cooked, op))
    }

    #[inline]
    fn dependencies(&self) -> impl Iterator<Item = TypeView<'a>> + use<'a, T> {
        let cooked = self.cooked();
        let meta = &cooked.metadata.schemas[self.index().index()];
        meta.dependencies
            .ones()
            .map(NodeIndex::new)
            .map(|index| TypeView::new(cooked, index))
    }

    #[inline]
    fn dependents(&self) -> impl Iterator<Item = TypeView<'a>> + use<'a, T> {
        let cooked = self.cooked();
        let meta = &cooked.metadata.schemas[self.index().index()];
        meta.dependents
            .ones()
            .map(NodeIndex::new)
            .map(move |index| TypeView::new(cooked, index))
    }

    #[inline]
    fn traverse<F>(
        &self,
        reach: Reach,
        filter: F,
    ) -> impl Iterator<Item = (EdgeKind, TypeView<'a>)> + use<'a, T, F>
    where
        F: Fn(EdgeKind, &TypeView<'a>) -> Traversal,
    {
        let cooked = self.cooked();
        let t = Traverse::from_neighbors(
            &cooked.graph,
            self.index(),
            match reach {
                Reach::Dependencies => Direction::Outgoing,
                Reach::Dependents => Direction::Incoming,
            },
        );
        t.run(move |kind, index| {
            let view = TypeView::new(cooked, index);
            filter(kind, &view)
        })
        .map(|(kind, index)| (kind, TypeView::new(cooked, index)))
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
        self.cooked().metadata.schemas[self.index().index()]
            .extensions
            .borrow()
    }

    #[inline]
    fn ext_mut<'b>(&'b mut self) -> AtomicRefMut<'b, ExtensionMap>
    where
        'graph: 'b,
    {
        self.cooked().metadata.schemas[self.index().index()]
            .extensions
            .borrow_mut()
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

/// Selects which edge direction to follow during traversal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reach {
    /// Traverse out toward types that this node depends on.
    Dependencies,
    /// Traverse in toward types that depend on this node.
    Dependents,
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
