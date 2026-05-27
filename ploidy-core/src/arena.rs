use std::sync::atomic::AtomicUsize;

use bumpalo::{
    Bump,
    collections::{CollectIn, Vec as BumpVec},
};

/// An allocation arena.
///
/// Objects allocated in an arena live as long as the arena itself.
/// The underlying allocator is an implementation detail.
#[derive(Debug, Default)]
pub struct Arena(Bump);

impl Arena {
    /// Creates a new, empty arena.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates and returns a mutable reference to a value.
    #[inline]
    pub(crate) fn alloc<T: Copy>(&self, value: T) -> &mut T {
        self.0.alloc(value)
    }

    /// Allocates and returns a mutable reference to an atomic primitive.
    #[inline]
    pub(crate) fn alloc_atomic<T: ToAtomic>(&self, value: T) -> &mut T::Atomic {
        self.0.alloc(value.to_atomic())
    }

    /// Copies and returns a mutable reference to a slice.
    #[inline]
    pub(crate) fn alloc_slice_copy<T: Copy>(&self, slice: &[T]) -> &mut [T] {
        self.0.alloc_slice_copy(slice)
    }

    /// Allocates and fills a slice with items from an iterator of known length.
    #[inline]
    pub(crate) fn alloc_slice_exact<I>(&self, iter: I) -> &mut [I::Item]
    where
        I: IntoIterator,
        I::IntoIter: ExactSizeIterator,
        I::Item: Copy,
    {
        self.0.alloc_slice_fill_iter(iter)
    }

    /// Allocates and fills a slice with items from an iterator. Unlike
    /// [`alloc_slice_exact`][Self::alloc_slice_exact], the iterator
    /// doesn't need to know its exact length.
    #[inline]
    pub(crate) fn alloc_slice<I: IntoIterator>(&self, iter: I) -> &mut [I::Item]
    where
        I::Item: Copy,
    {
        iter.into_iter()
            .collect_in::<BumpVec<_>>(&self.0)
            .into_bump_slice_mut()
    }

    /// Allocates and returns a mutable reference to a string slice.
    #[inline]
    pub(crate) fn alloc_str(&self, s: &str) -> &mut str {
        self.0.alloc_str(s)
    }

    /// Allocates and returns a reference to a formatted string.
    #[inline]
    pub(crate) fn alloc_fmt(&self, f: std::fmt::Arguments<'_>) -> &str {
        bumpalo::format!(in &self.0, "{}", f).into_bump_str()
    }
}

// Atomics aren't `Copy`, but are trivially droppable, and so OK to allocate
// in an arena directly. We can replace this trait with `Atomic<T>` once
// the `generic_atomic` feature stabilizes.
pub(crate) trait ToAtomic {
    type Atomic;

    fn to_atomic(self) -> Self::Atomic;
}

impl ToAtomic for usize {
    type Atomic = AtomicUsize;

    #[inline]
    fn to_atomic(self) -> Self::Atomic {
        AtomicUsize::new(self)
    }
}
