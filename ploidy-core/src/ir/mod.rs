mod error;
mod fields;
mod graph;
mod spec;
mod transform;
mod types;
mod views;

#[cfg(test)]
mod tests;

pub use graph::IrGraph;
pub use spec::IrSpec;
pub use types::*;

pub use views::{
    ExtendableView, Reach, Traversal, View, container::*, enum_::*, inline::*, ir::*, operation::*,
    primitive::*, schema::*, struct_::*, tagged::*, untagged::*,
};
