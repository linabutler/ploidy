mod error;
mod graph;
mod spec;
mod transform;
mod types;
mod views;

#[cfg(test)]
mod tests;

pub use graph::{EdgeKind, IrGraph, SccId, Traversal};
pub use spec::IrSpec;
pub use types::*;

pub use views::{
    ExtendableView, Reach, View, ViewNode, any::*, container::*, enum_::*, inline::*, ir::*,
    operation::*, primitive::*, schema::*, struct_::*, tagged::*, untagged::*,
};
