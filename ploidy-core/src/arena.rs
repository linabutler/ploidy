use bumpalo::Bump;

/// An allocation arena.
///
/// Objects allocated in an arena live as long as the arena itself.
/// The underlying allocator is an implementation detail.
#[derive(Debug, Default)]
pub struct Arena(Bump);

impl Arena {
    /// Creates a new, empty arena.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a reference to the underlying [`Bump`] allocator.
    pub(crate) fn inner(&self) -> &Bump {
        &self.0
    }
}
