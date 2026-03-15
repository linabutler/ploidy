//! Intermediate representation for OpenAPI code generation.
//!
//! The IR has three layers:
//!
//! * **Types** define the data types that all other layers share.
//!   Each type shape is parameterized over _how it references other types_:
//!   spec types hold unresolved JSON Pointer references, while graph types
//!   hold resolved node indices.
//!
//! * A [`Spec`] is a tree of those data types, lowered from
//!   a parsed [`Document`], with references still unresolved.
//!
//! * The **graph** resolves those references into a dependency graph.
//!   [`RawGraph`] is the mutable form used for in-place transformations;
//!   [`CookedGraph`] is the frozen form used for traversal and codegen.
//!
//! The [`views`] module wraps cooked graph nodes in read-only view types
//! that expose traversal and metadata. See that module and the
//! [crate root](crate) for usage.
//!
//! [`Document`]: crate::parse::Document

mod error;
mod graph;
mod spec;
mod transform;
mod types;
pub mod views;

#[cfg(test)]
mod tests;

pub use graph::{CookedGraph, EdgeKind, RawGraph, Traversal};
pub use spec::Spec;
pub use types::*;

pub use views::{
    ExtendableView, Reach, View, any::*, container::*, enum_::*, inline::*, ir::*, operation::*,
    primitive::*, schema::*, struct_::*, tagged::*, untagged::*,
};
