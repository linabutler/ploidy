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
    pub(crate) fn alloc<T>(&self, value: T) -> &mut T {
        self.0.alloc(value)
    }

    /// Copies and returns a mutable reference to a slice.
    #[inline]
    pub(crate) fn alloc_slice_copy<T: Copy>(&self, slice: &[T]) -> &mut [T] {
        self.0.alloc_slice_copy(slice)
    }

    /// Clones and returns a mutable reference to a slice.
    /// [`alloc_slice_copy`][Self::alloc_slice_copy] is more efficient if
    /// `T` is `Copy`.
    #[inline]
    pub(crate) fn alloc_slice_clone<T: Clone>(&self, slice: &[T]) -> &mut [T] {
        self.0.alloc_slice_clone(slice)
    }

    /// Allocates and fills a slice with items from an iterator of known length.
    #[inline]
    pub(crate) fn alloc_slice_exact<I>(&self, iter: I) -> &mut [I::Item]
    where
        I: IntoIterator,
        I::IntoIter: ExactSizeIterator,
    {
        self.0.alloc_slice_fill_iter(iter)
    }

    /// Allocates and fills a slice with items from an iterator. Unlike
    /// [`alloc_slice_exact`][Self::alloc_slice_exact], the iterator
    /// doesn't need to know its exact length.
    #[inline]
    pub(crate) fn alloc_slice<I: IntoIterator>(&self, iter: I) -> &mut [I::Item] {
        iter.into_iter()
            .collect_in::<BumpVec<_>>(&self.0)
            .into_bump_slice_mut()
    }

    /// Allocates and returns a mutable reference to a string slice.
    #[inline]
    pub(crate) fn alloc_str(&self, s: &str) -> &mut str {
        self.0.alloc_str(s)
    }
}
