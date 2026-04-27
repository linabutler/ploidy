//! Language-agnostic intermediate representation and type graph
//! for the Ploidy OpenAPI compiler.
//!
//! **ploidy-core** transforms a parsed OpenAPI document into a
//! typed dependency graph that codegen backends traverse to emit
//! code.
//!
//! # Pipeline
//!
//! ```rust
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let source = indoc::indoc! {"
//! #     openapi: 3.0.0
//! #     info:
//! #       title: Test API
//! #       version: 1.0
//! # "};
//! use ploidy_core::{arena::Arena, ir::{RawGraph, Spec}, parse::Document};
//!
//! let doc = Document::from_yaml(&source)?;
//!
//! let arena = Arena::new();
//! let spec = Spec::from_doc(&arena, &doc)?;
//! let mut raw = RawGraph::new(&arena, &spec);
//! raw.inline_tagged_variants();
//! let graph = raw.cook();
//!
//! for view in graph.schemas() { /* ... */ }
//! for view in graph.operations() { /* ... */ }
//! # Ok(())
//! # }
//! ```
//!
//! # Arena
//!
//! An [`Arena`] is a bump allocator that owns all long-lived data.
//! Types throughout the pipeline hold borrowed references to other
//! arena-allocated types, making them cheaply copyable. Callers
//! create an arena at the start, and pass it to each constructor.
//!
//! # Named vs. inline types
//!
//! The IR distinguishes two kinds of types:
//!
//! - **Named schema types** originate from `components/schemas`
//!   in the OpenAPI document. Each carries a [`SchemaTypeInfo`] with
//!   the schema name and additional metadata.
//! - **Inline types** are anonymous schemas nested inside other types.
//!   Each carries an [`InlineTypeId`](ir::InlineTypeId) for identity
//!   and an [`InlineTrace`](ir::InlineTrace) that encodes the type's
//!   position in the graph.
//!
//! The two kinds carry different metadata, but share the same structural
//! shapes: [any], [containers], [enums], [primitives], [structs],
//! [tagged unions], and [untagged unions].
//!
//! # Using the graph
//!
//! A [`RawGraph`] represents types and references as they exist in the
//! OpenAPI document. Transformations on this graph rewrite it in-place.
//! The transformed graph is then "cooked" into a [`CookedGraph`] that's
//! ready for codegen.
//!
//! [`CookedGraph::schemas()`] yields [`SchemaTypeView`]s. Match on the variant
//! to get the specific shape (e.g., `SchemaTypeView::Struct`) for generating
//! type models.
//!
//! [`CookedGraph::operations()`] yields [`OperationView`]s. Use these to
//! access paths, methods, query parameters, and request and response types
//! for generating client endpoints.
//!
//! See the [`ir::views`] module for all view types and traversal methods.
//!
//! [`Arena`]: arena::Arena
//! [`SchemaTypeInfo`]: ir::SchemaTypeInfo
//! [any]: ir::views::any
//! [containers]: ir::views::container
//! [enums]: ir::views::enum_
//! [primitives]: ir::views::primitive
//! [structs]: ir::views::struct_
//! [tagged unions]: ir::views::tagged
//! [untagged unions]: ir::views::untagged
//! [`RawGraph`]: ir::RawGraph
//! [`CookedGraph`]: ir::CookedGraph
//! [`CookedGraph::schemas()`]: ir::CookedGraph::schemas
//! [`SchemaTypeView`]: ir::SchemaTypeView
//! [`CookedGraph::operations()`]: ir::CookedGraph::operations
//! [`OperationView`]: ir::OperationView

#[macro_use]
mod macros;

pub mod arena;
pub mod codegen;
pub mod error;
pub mod ir;
pub mod parse;

#[cfg(test)]
mod tests;
