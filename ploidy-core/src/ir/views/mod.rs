//! Graph-aware views of IR types.
//!
//! These views provide a representation of schema and inline types
//! for code generation. Each view decorates an IR type with
//! additional information from the type graph.

use std::{any::TypeId, fmt::Debug, ops::Deref};

use atomic_refcell::{AtomicRef, AtomicRefMut};
use petgraph::{
    graph::NodeIndex,
    visit::{Bfs, EdgeFiltered, EdgeRef},
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

    /// Returns a read-only view of this type's extended data.
    fn extensions(&self) -> &IrViewExtensions<Self>
    where
        Self: Extendable<'a> + Sized;

    /// Returns a read-write view of this type's extended data.
    fn extensions_mut(&mut self) -> &mut IrViewExtensionsMut<Self>
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
    fn extensions(&self) -> &IrViewExtensions<Self> {
        IrViewExtensions::new(self)
    }

    #[inline]
    fn extensions_mut(&mut self) -> &mut IrViewExtensionsMut<Self> {
        IrViewExtensionsMut::new(self)
    }
}

pub trait ViewNode<'a>: private::Sealed {
    fn graph(&self) -> &'a IrGraph<'a>;
    fn index(&self) -> NodeIndex;
}

pub trait Extendable<'graph>: private::Sealed {
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

impl<'a, T> Extendable<'a> for T
where
    T: ViewNode<'a>,
{
    #[inline]
    fn ext<'b>(&'b self) -> AtomicRef<'b, ExtensionMap>
    where
        'a: 'b,
    {
        self.graph().metadata[&self.index()].extensions.borrow()
    }

    #[inline]
    fn ext_mut<'b>(&'b mut self) -> AtomicRefMut<'b, ExtensionMap>
    where
        'a: 'b,
    {
        self.graph().metadata[&self.index()].extensions.borrow_mut()
    }
}

/// A view of the extended data attached to a type.
///
/// Generators can use extended data to decorate types with
/// additional information, like name mappings.
#[derive(RefCastCustom)]
#[repr(transparent)]
pub struct IrViewExtensions<X>(X);

impl<'a, X: Extendable<'a>> IrViewExtensions<X> {
    #[ref_cast_custom]
    fn new(view: &X) -> &Self;

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
}

impl<X> Debug for IrViewExtensions<X> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("IrViewExtensions").finish_non_exhaustive()
    }
}

/// A mutable view of the extended data attached to a type.
#[derive(RefCastCustom)]
#[repr(transparent)]
pub struct IrViewExtensionsMut<X>(X);

impl<'a, X: Extendable<'a>> IrViewExtensionsMut<X> {
    #[ref_cast_custom]
    fn new(view: &mut X) -> &mut Self;

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

impl<X> Debug for IrViewExtensionsMut<X> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("IrViewExtensionsMut").finish_non_exhaustive()
    }
}

impl<'a, X: Extendable<'a>> Deref for IrViewExtensionsMut<X> {
    type Target = IrViewExtensions<X>;

    fn deref(&self) -> &Self::Target {
        IrViewExtensions::new(&self.0)
    }
}

mod private {
    use super::*;
    use super::{
        enum_::IrEnumView,
        inline::InlineIrTypeView,
        ir::IrTypeView,
        operation::IrParameterView,
        schema::SchemaIrTypeView,
        struct_::{IrStructFieldView, IrStructView},
        tagged::{IrTaggedVariantView, IrTaggedView},
        untagged::{IrUntaggedVariantView, IrUntaggedView},
        wrappers::{IrArrayView, IrMapView, IrNullableView},
    };

    pub trait Sealed {}

    impl<'a> Sealed for (&'a IrGraph<'a>, NodeIndex) {}
    impl Sealed for IrArrayView<'_> {}
    impl Sealed for IrMapView<'_> {}
    impl Sealed for IrNullableView<'_> {}
    impl<T> Sealed for IrParameterView<'_, T> {}
    impl Sealed for IrTypeView<'_> {}
    impl Sealed for IrTaggedView<'_> {}
    impl Sealed for IrTaggedVariantView<'_> {}
    impl Sealed for InlineIrTypeView<'_> {}
    impl Sealed for IrStructView<'_> {}
    impl Sealed for IrStructFieldView<'_, '_> {}
    impl Sealed for IrUntaggedView<'_> {}
    impl Sealed for IrUntaggedVariantView<'_, '_> {}
    impl Sealed for IrEnumView<'_> {}
    impl Sealed for SchemaIrTypeView<'_> {}
}
