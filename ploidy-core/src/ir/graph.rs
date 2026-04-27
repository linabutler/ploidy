use std::{
    any::{Any, TypeId},
    collections::VecDeque,
    fmt::Debug,
};

use atomic_refcell::AtomicRefCell;
use fixedbitset::FixedBitSet;
use itertools::Itertools;
use petgraph::{
    Direction,
    adj::UnweightedList,
    algo::{TarjanScc, tred},
    data::Build,
    graph::{DiGraph, NodeIndex},
    stable_graph::StableDiGraph,
    visit::{
        DfsPostOrder, EdgeFiltered, EdgeRef, IntoNeighbors, IntoNeighborsDirected,
        IntoNodeIdentifiers, NodeCount, NodeIndexable,
    },
};
use rustc_hash::{FxBuildHasher, FxHashMap};

use crate::{
    arena::Arena,
    ir::{SchemaTypeInfo, UntaggedVariantMeta},
    parse::Info,
};

use super::{
    spec::{ResolvedSpecType, Spec},
    types::{
        FieldMeta, GraphContainer, GraphInlineType, GraphOperation, GraphSchemaType, GraphStruct,
        GraphTagged, GraphType, InlineTypePath, InlineTypePathRoot, InlineTypePathSegment,
        PrimitiveType, SpecInlineType, SpecSchemaType, SpecType, SpecUntaggedVariant,
        StructFieldName, TaggedVariantMeta, VariantMeta,
        shape::{Operation, Parameter, ParameterInfo, Request, Response},
    },
    views::{operation::OperationView, primitive::PrimitiveView, schema::SchemaTypeView},
};

/// The mutable, sparse graph used for transformations.
type RawDiGraph<'a> = StableDiGraph<GraphType<'a>, GraphEdge<'a>, usize>;

/// The immutable, dense graph used for code generation.
type CookedDiGraph<'a> = DiGraph<GraphType<'a>, GraphEdge<'a>, usize>;

/// A mutable intermediate dependency graph of all the types in a [`Spec`],
/// backed by a sparse [`StableDiGraph`].
///
/// This graph is constructed directly from a [`Spec`], and represents
/// type relationships as they exist in the spec. Transformations like
/// [`inline_tagged_variants`][Self::inline_tagged_variants] rewrite this graph
/// in place.
///
/// After applying all transformations, call [`cook`][Self::cook] to
/// turn this graph into a [`CookedGraph`] that's ready for code generation.
#[derive(Debug)]
pub struct RawGraph<'a> {
    arena: &'a Arena,
    spec: &'a Spec<'a>,
    graph: RawDiGraph<'a>,
    schemas: FxHashMap<&'a str, NodeIndex<usize>>,
    ops: &'a [&'a GraphOperation<'a>],
}

impl<'a> RawGraph<'a> {
    /// Builds a raw type graph from the given spec.
    pub fn new(arena: &'a Arena, spec: &'a Spec<'a>) -> Self {
        // All roots (named schemas, parameters, request and response bodies),
        // and all the types within them (inline schemas and primitives).
        let tys = SpecTypeVisitor::new(
            spec.schemas
                .values()
                .chain(spec.operations.iter().flat_map(|op| op.types().copied())),
        );

        // Inflate a graph from the traversal.
        let mut indices = FxHashMap::default();
        let mut schemas = FxHashMap::default();
        let mut graph = RawDiGraph::default();
        for (parent, child) in tys {
            use std::collections::hash_map::Entry;

            let source = spec.resolve(child);
            let &mut to = match indices.entry(source) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    // We might see the same schema multiple times if it's
                    // referenced multiple times in the spec. Only add
                    // a new node for the schema if we haven't seen it before.
                    let index = graph.add_node(match *entry.key() {
                        ResolvedSpecType::Schema(&ty) => GraphType::Schema(ty.into()),
                        ResolvedSpecType::Inline(&ty) => GraphType::Inline(ty.into()),
                    });
                    if let ResolvedSpecType::Schema(ty) = source {
                        schemas.entry(ty.name()).or_insert(index);
                    }
                    entry.insert(index)
                }
            };

            if let Some((parent, edge)) = parent {
                let destination = spec.resolve(parent);
                let &mut from = match indices.entry(destination) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => {
                        let index = graph.add_node(match *entry.key() {
                            ResolvedSpecType::Schema(&ty) => GraphType::Schema(ty.into()),
                            ResolvedSpecType::Inline(&ty) => GraphType::Inline(ty.into()),
                        });
                        if let ResolvedSpecType::Schema(ty) = destination {
                            schemas.entry(ty.name()).or_insert(index);
                        }
                        entry.insert(index)
                    }
                };
                graph.add_edge(from, to, edge);
            }
        }

        // Map type references in operations to graph indices.
        let ops = arena.alloc_slice_exact(spec.operations.iter().map(|op| {
            let params = arena.alloc_slice_exact(op.params.iter().map(|param| match param {
                Parameter::Path(info) => Parameter::Path(ParameterInfo {
                    name: info.name,
                    ty: match info.ty {
                        SpecType::Schema(s) => indices[&ResolvedSpecType::Schema(s)],
                        SpecType::Inline(i) => indices[&ResolvedSpecType::Inline(i)],
                        SpecType::Ref(r) => schemas[&*r.name()],
                    },
                    required: info.required,
                    description: info.description,
                    style: info.style,
                }),
                Parameter::Query(info) => Parameter::Query(ParameterInfo {
                    name: info.name,
                    ty: match info.ty {
                        SpecType::Schema(s) => indices[&ResolvedSpecType::Schema(s)],
                        SpecType::Inline(i) => indices[&ResolvedSpecType::Inline(i)],
                        SpecType::Ref(r) => schemas[&*r.name()],
                    },
                    required: info.required,
                    description: info.description,
                    style: info.style,
                }),
            }));

            let request = op.request.as_ref().map(|r| match r {
                Request::Json(ty) => Request::Json(match ty {
                    SpecType::Schema(s) => indices[&ResolvedSpecType::Schema(s)],
                    SpecType::Inline(i) => indices[&ResolvedSpecType::Inline(i)],
                    SpecType::Ref(r) => schemas[&*r.name()],
                }),
                Request::Multipart => Request::Multipart,
            });

            let response = op.response.as_ref().map(|r| match r {
                Response::Json(ty) => Response::Json(match ty {
                    SpecType::Schema(s) => indices[&ResolvedSpecType::Schema(s)],
                    SpecType::Inline(i) => indices[&ResolvedSpecType::Inline(i)],
                    SpecType::Ref(r) => schemas[&*r.name()],
                }),
            });

            &*arena.alloc(Operation {
                id: op.id,
                method: op.method,
                path: op.path,
                resource: op.resource,
                description: op.description,
                params,
                request,
                response,
            })
        }));

        Self {
            arena,
            spec,
            graph,
            schemas,
            ops,
        }
    }

    /// Inlines schema types used as variants of multiple tagged unions
    /// with different tags.
    ///
    /// In OpenAPI's model of tagged unions, the tag always references a field
    /// that's defined on each variant struct. This model works well for Python
    /// and TypeScript, but not Rust; Serde doesn't allow variant structs to
    /// declare fields with the same name as the tag. The Rust generator
    /// excludes tag fields when generating structs, but this introduces a
    /// new problem: a struct can't appear as a variant of multiple unions
    /// with different tags [^1].
    ///
    /// This transformation finds and inlines these structs, so that
    /// the Rust generator can safely omit their tag fields.
    ///
    /// [^1]: If struct A has fields `foo` and `bar`, A is a variant of
    /// tagged unions C and D, C's tag is `foo`, and D's tag is `bar`...
    /// only `foo` should be excluded when A is used in C, and only `bar`
    /// should be excluded when A is used in D; but this can't be modeled
    /// in Serde without splitting A into two distinct types.
    pub fn inline_tagged_variants(&mut self) -> &mut Self {
        // Collect all inlining decisions before mutating the graph,
        // so that we can check inlinability per variant.
        let inlinables = self.inlinable_tagged_variants().collect_vec();

        let mut retargets = FxHashMap::default();
        retargets.reserve(inlinables.len());

        // Add nodes for the inlined variant structs,
        // and their outgoing edges.
        for InlinableVariant { tagged, variant } in inlinables {
            // Duplicate the variant struct as an inline type,
            // with its original metadata.
            let index = self
                .graph
                .add_node(GraphType::Inline(GraphInlineType::Struct(
                    InlineTypePath {
                        root: InlineTypePathRoot::Type(tagged.info.name),
                        segments: self.arena.alloc_slice_copy(&[
                            InlineTypePathSegment::TaggedVariant(variant.info.name),
                        ]),
                    },
                    variant.ty,
                )));

            // Create shadow edges to the original variant struct's fields.
            // These serve two purposes:
            //
            // 1. If a field is recursive, the duplicate joins the field's SCC,
            //    not the original's SCC, so field edges to the original type
            //    won't be treated as cyclic.
            // 2. Hiding the originals' inlines from the duplicate's inlines.
            //
            // `fields()` yields edges in reverse order of addition;
            // we collect and reverse to add them in their original order.
            let original_field_edges = self.fields(variant.index).collect_vec();
            for edge in original_field_edges.into_iter().rev() {
                self.graph.add_edge(
                    index,
                    edge.target,
                    GraphEdge::Field {
                        meta: edge.meta,
                        shadow: true,
                    },
                );
            }

            // Inherit from the tagged union (to pick up its own fields)
            // and the original variant struct (to pick up its ancestors).
            // The union is added first so that its fields appear first _and_
            // can be overridden by the variant's fields.
            self.graph
                .add_edge(index, tagged.index, GraphEdge::Inherits { shadow: true });
            self.graph
                .add_edge(index, variant.index, GraphEdge::Inherits { shadow: true });

            retargets.insert((tagged.index, variant.index), index);
        }

        // Retarget every tagged union's variant edges to the new structs.
        let taggeds: FixedBitSet = retargets
            .keys()
            .map(|&(tagged, _)| tagged.index())
            .collect();
        for index in taggeds.ones().map(NodeIndex::new) {
            let old_edges = self
                .graph
                .edges_directed(index, Direction::Outgoing)
                .filter(|e| matches!(e.weight(), GraphEdge::Variant(_)))
                .map(|e| (e.id(), *e.weight(), e.target()))
                .collect_vec();
            for &(id, _, _) in &old_edges {
                self.graph.remove_edge(id);
            }
            // Re-add edges. `edges_directed` yields edges in reverse order
            // of addition; reversing them adds edges in their original order.
            for (_, weight, target) in old_edges.into_iter().rev() {
                let new_target = retargets.get(&(index, target)).copied().unwrap_or(target);
                self.graph.add_edge(index, new_target, weight);
            }
        }

        self
    }

    /// Builds an immutable [`CookedGraph`] from this mutable raw graph.
    #[inline]
    pub fn cook(&self) -> CookedGraph<'a> {
        CookedGraph::new(self)
    }

    /// Returns an iterator over all the fields of a struct or union type,
    /// in reverse insertion order.
    fn fields(&self, node: NodeIndex<usize>) -> impl Iterator<Item = OutgoingEdge<FieldMeta<'a>>> {
        self.graph
            .edges_directed(node, Direction::Outgoing)
            .filter_map(|e| match e.weight() {
                &GraphEdge::Field { meta, .. } => {
                    let target = e.target();
                    Some(OutgoingEdge { meta, target })
                }
                _ => None,
            })
    }

    /// Returns an iterator over all the tagged union variant structs
    /// that should be inlined.
    fn inlinable_tagged_variants(&self) -> impl Iterator<Item = InlinableVariant<'a>> {
        // Compute the set of types used by all operations.
        // Operations don't participate in the graph, but
        // still need to be considered when deciding
        // whether to inline a variant struct.
        //
        // Otherwise, a struct that's used by same-tag unions
        // _and_ an operation wouldn't be inlined, incorrectly
        // removing its tag field.
        let used_by_ops: FixedBitSet = self
            .ops
            .iter()
            .flat_map(|op| op.types())
            .map(|index| index.index())
            .collect();

        self.graph
            .node_indices()
            .filter_map(|index| match self.graph[index] {
                GraphType::Schema(GraphSchemaType::Tagged(info, ty)) => {
                    Some(Node { index, info, ty })
                }
                _ => None,
            })
            .flat_map(move |tagged| {
                self.graph
                    .edges_directed(tagged.index, Direction::Outgoing)
                    .filter(|e| matches!(e.weight(), GraphEdge::Variant(_)))
                    .filter_map(move |e| match self.graph[e.target()] {
                        GraphType::Schema(GraphSchemaType::Struct(info, ty)) => {
                            let index = e.target();
                            Some((tagged, Node { index, info, ty }))
                        }
                        _ => None,
                    })
            })
            .filter_map(move |(tagged, variant)| {
                // A variant struct only needs inlining if it has multiple
                // distinct uses. Skip if (1) no operation uses the struct,
                // _and_ (2) every incoming edge is from a tagged union with
                // the same tag and fields. If both hold, all uses agree, so
                // the struct can be used directly without inlining.
                if used_by_ops[variant.index.index()] {
                    return Some((tagged, variant));
                }

                // Check that all the variant's inbound edges are from
                // tagged unions, and that all their tags and field
                // edges match the first union we found.
                let first_tagged = self
                    .graph
                    .neighbors_directed(variant.index, Direction::Incoming)
                    .find_map(|index| match self.graph[index] {
                        GraphType::Schema(GraphSchemaType::Tagged(info, ty)) => {
                            Some(Node { index, info, ty })
                        }
                        _ => None,
                    })?;
                let all_agree = self
                    .graph
                    .neighbors_directed(variant.index, Direction::Incoming)
                    .all(|index| match self.graph[index] {
                        GraphType::Schema(GraphSchemaType::Tagged(_, ty)) => {
                            ty.tag == first_tagged.ty.tag
                                && self.fields(index).eq(self.fields(first_tagged.index))
                        }
                        _ => false,
                    });
                if all_agree {
                    return None;
                }
                Some((tagged, variant))
            })
            .filter_map(|(tagged, variant)| {
                // Skip inlining when the inline copy would be identical
                // to the original. This happens when the variant
                // doesn't declare the tag as a field _and_ either
                // (a) the union has no own fields, or (b) the variant
                // inherits from the union.
                let ancestors = EdgeFiltered::from_fn(&self.graph, |e| {
                    matches!(e.weight(), GraphEdge::Inherits { .. })
                });
                let mut dfs = DfsPostOrder::new(&ancestors, variant.index);
                let has_tag_field = std::iter::from_fn(|| dfs.next(&ancestors))
                    .filter(|&n| {
                        matches!(
                            self.graph[n],
                            GraphType::Schema(GraphSchemaType::Struct(..))
                                | GraphType::Inline(GraphInlineType::Struct(..))
                        )
                    })
                    .any(|n| {
                        self.fields(n).any(|f| {
                            matches!(f.meta.name, StructFieldName::Name(name)
                                if name == tagged.ty.tag)
                        })
                    });

                // If the variant declares or inherits the tag field,
                // we must inline, so that the inline copy can safely
                // omit the tag.
                if has_tag_field {
                    return Some(InlinableVariant { tagged, variant });
                }

                // If the DFS visited the union, the variant already inherits
                // its fields; the inline copy would be identical.
                if dfs.discovered[tagged.index.index()] {
                    return None;
                }

                // If the variant doesn't inherit from the union, but the union
                // has no fields of its own, the inline copy would be identical.
                self.fields(tagged.index).next()?;

                Some(InlinableVariant { tagged, variant })
            })
    }
}

/// The final dependency graph of all the types in a [`Spec`],
/// backed by a dense [`DiGraph`].
///
/// This graph has all transformations applied, and is ready for
/// code generation.
#[derive(Debug)]
pub struct CookedGraph<'a> {
    pub(super) graph: CookedDiGraph<'a>,
    info: &'a Info,
    schemas: FxHashMap<&'a str, NodeIndex<usize>>,
    ops: &'a [&'a GraphOperation<'a>],
    /// Additional metadata for each node.
    pub(super) metadata: CookedGraphMetadata<'a>,
}

impl<'a> CookedGraph<'a> {
    fn new(raw: &RawGraph<'a>) -> Self {
        // Build a dense graph, mapping sparse raw node indices to
        // dense cooked node indices.
        let mut graph =
            CookedDiGraph::with_capacity(raw.graph.node_count(), raw.graph.edge_count());
        let mut indices =
            FxHashMap::with_capacity_and_hasher(raw.graph.node_count(), FxBuildHasher);
        for raw_index in raw.graph.node_indices() {
            let cooked_index = graph.add_node(raw.graph[raw_index]);
            indices.insert(raw_index, cooked_index);
        }

        // Copy edges.
        //
        // `raw.graph.edges()` yields edges in reverse order of addition.
        // The raw graph adds edges in declaration order, so `edges()`
        // yields them reversed. Re-adding them to the cooked graph in that
        // reversed order means they're now stored in reverse-declaration order,
        // letting the cooked graph's accessors yield edges in declaration order
        // without any extra work.
        for index in raw.graph.node_indices() {
            let from = indices[&index];
            let edges = raw
                .graph
                .edges(index)
                .map(|e| (indices[&e.target()], *e.weight()));
            for (to, kind) in edges {
                graph.add_edge(from, to, kind);
            }
        }

        // Remap schema type references in operations.
        let ops: &_ = raw.arena.alloc_slice_exact(raw.ops.iter().map(|&op| {
            &*raw.arena.alloc(Operation {
                id: op.id,
                method: op.method,
                path: op.path,
                resource: op.resource,
                description: op.description,
                params: raw
                    .arena
                    .alloc_slice_exact(op.params.iter().map(|p| match p {
                        Parameter::Path(info) => Parameter::Path(ParameterInfo {
                            name: info.name,
                            ty: indices[&info.ty],
                            required: info.required,
                            description: info.description,
                            style: info.style,
                        }),
                        Parameter::Query(info) => Parameter::Query(ParameterInfo {
                            name: info.name,
                            ty: indices[&info.ty],
                            required: info.required,
                            description: info.description,
                            style: info.style,
                        }),
                    })),
                request: op.request.as_ref().map(|r| match r {
                    Request::Json(ty) => Request::Json(indices[ty]),
                    Request::Multipart => Request::Multipart,
                }),
                response: op.response.as_ref().map(|r| match r {
                    Response::Json(ty) => Response::Json(indices[ty]),
                }),
            })
        }));

        let metadata = MetadataBuilder::new(&graph, ops).build();

        Self {
            graph,
            info: raw.spec.info,
            schemas: raw
                .schemas
                .iter()
                .map(|(&name, index)| (name, indices[index]))
                .collect(),
            ops,
            metadata,
        }
    }

    /// Returns [`Info`] from the [`Document`][crate::parse::Document]
    /// used to build this graph.
    #[inline]
    pub fn info(&self) -> &'a Info {
        self.info
    }

    /// Returns an iterator over all the named schemas in this graph.
    #[inline]
    pub fn schemas(&self) -> impl Iterator<Item = SchemaTypeView<'_, 'a>> + use<'_, 'a> {
        self.graph
            .node_indices()
            .filter_map(|index| match self.graph[index] {
                GraphType::Schema(ty) => Some(SchemaTypeView::new(self, index, ty)),
                _ => None,
            })
    }

    /// Looks up and returns a schema by name.
    #[inline]
    pub fn schema(&self, name: &str) -> Option<SchemaTypeView<'_, 'a>> {
        self.schemas
            .get(name)
            .and_then(|&index| match self.graph[index] {
                GraphType::Schema(ty) => Some(SchemaTypeView::new(self, index, ty)),
                _ => None,
            })
    }

    /// Returns an iterator over all primitive type nodes in this graph.
    #[inline]
    pub fn primitives(&self) -> impl Iterator<Item = PrimitiveView<'_, 'a>> + use<'_, 'a> {
        self.graph
            .node_indices()
            .filter_map(|index| match self.graph[index] {
                GraphType::Schema(GraphSchemaType::Primitive(_, p))
                | GraphType::Inline(GraphInlineType::Primitive(_, p)) => {
                    Some(PrimitiveView::new(self, index, p))
                }
                _ => None,
            })
    }

    /// Returns an iterator over all the operations in this graph.
    #[inline]
    pub fn operations(&self) -> impl Iterator<Item = OperationView<'_, 'a>> + use<'_, 'a> {
        self.ops.iter().map(|&op| OperationView::new(self, op))
    }

    #[inline]
    pub(super) fn inherits(
        &self,
        node: NodeIndex<usize>,
    ) -> impl Iterator<Item = OutgoingEdge<()>> {
        self.graph
            .edges_directed(node, Direction::Outgoing)
            .filter(|e| matches!(e.weight(), GraphEdge::Inherits { .. }))
            .map(|e| OutgoingEdge {
                meta: (),
                target: e.target(),
            })
    }

    #[inline]
    pub(super) fn fields(
        &self,
        node: NodeIndex<usize>,
    ) -> impl Iterator<Item = OutgoingEdge<FieldMeta<'a>>> {
        self.graph
            .edges_directed(node, Direction::Outgoing)
            .filter_map(|e| match e.weight() {
                &GraphEdge::Field { meta, .. } => {
                    let target = e.target();
                    Some(OutgoingEdge { meta, target })
                }
                _ => None,
            })
    }

    #[inline]
    pub(super) fn variants(
        &self,
        node: NodeIndex<usize>,
    ) -> impl Iterator<Item = OutgoingEdge<VariantMeta<'a>>> {
        self.graph
            .edges_directed(node, Direction::Outgoing)
            .filter_map(|e| match e.weight() {
                &GraphEdge::Variant(meta) => {
                    let target = e.target();
                    Some(OutgoingEdge { meta, target })
                }
                _ => None,
            })
    }
}

/// A variant that should be inlined into its tagged union.
struct InlinableVariant<'a> {
    /// The tagged union that owns this variant.
    tagged: Node<'a, GraphTagged<'a>>,
    /// The original variant struct node.
    variant: Node<'a, GraphStruct<'a>>,
}

/// An edge between two types in the type graph.
///
/// Edges describe the relationship between their source and target types.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GraphEdge<'a> {
    /// The source type inherits from the target type.
    Inherits { shadow: bool },
    /// The source struct, tagged union, or untagged union
    /// has the target type as a field.
    Field { shadow: bool, meta: FieldMeta<'a> },
    /// The source union has the target type as a variant.
    Variant(VariantMeta<'a>),
    /// The source type is an array, map, or optional that contains
    /// the target type.
    Contains,
}

impl GraphEdge<'_> {
    /// Returns `true` if the target type should be excluded from
    /// the source type's [inlines], but still considered a dependency.
    ///
    /// Shadow edges prevent inlined variant structs from claiming
    /// their originals' inlines.
    ///
    /// [inlines]: crate::ir::views::View::inlines
    #[inline]
    pub fn shadow(&self) -> bool {
        matches!(
            self,
            GraphEdge::Field { shadow: true, .. } | GraphEdge::Inherits { shadow: true }
        )
    }
}

/// Metadata describing an edge from a source to a target type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutgoingEdge<T> {
    pub meta: T,
    pub target: NodeIndex<usize>,
}

#[derive(Clone, Copy)]
struct Node<'a, Ty> {
    index: NodeIndex<usize>,
    info: SchemaTypeInfo<'a>,
    ty: Ty,
}

/// Precomputed metadata for schema types and operations in the graph.
pub(super) struct CookedGraphMetadata<'a> {
    /// Transitive closure over the type graph.
    pub closure: Closure,
    /// Maps each type to its SCC equivalence class for boxing decisions.
    /// Two types in the same class form a cycle that requires `Box<T>`.
    pub box_sccs: Vec<usize>,
    /// Whether each type can implement `Eq` and `Hash`.
    pub hashable: FixedBitSet,
    /// Whether each type can implement `Default`.
    pub defaultable: FixedBitSet,
    /// Maps each type to the operations that use it.
    pub used_by: Vec<Vec<GraphOperation<'a>>>,
    /// Maps each operation to the types that it uses.
    pub uses: FxHashMap<GraphOperation<'a>, FixedBitSet>,
    /// Opaque extended data for each type.
    pub extensions: Vec<AtomicRefCell<ExtensionMap>>,
}

impl Debug for CookedGraphMetadata<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookedGraphMetadata")
            .field("closure", &self.closure)
            .field("box_sccs", &self.box_sccs)
            .field("hashable", &self.hashable)
            .field("defaultable", &self.defaultable)
            .field("used_by", &self.used_by)
            .field("uses", &self.uses)
            .finish_non_exhaustive()
    }
}

/// Precomputed bitsets indicating which types can derive
/// `Eq` / `Hash` and `Default`.
struct HashDefault {
    hashable: FixedBitSet,
    defaultable: FixedBitSet,
}

/// Precomputed metadata for an operation that references
/// types in the graph.
struct Operations<'a> {
    /// All the types that each operation depends on, directly and transitively.
    pub uses: FxHashMap<GraphOperation<'a>, FixedBitSet>,
    /// All the operations that use each type, directly and transitively.
    pub used_by: Vec<Vec<GraphOperation<'a>>>,
}

struct MetadataBuilder<'graph, 'a> {
    graph: &'graph CookedDiGraph<'a>,
    ops: &'graph [&'graph GraphOperation<'a>],
    /// The full transitive closure of each type's dependencies.
    closure: Closure,
}

impl<'graph, 'a> MetadataBuilder<'graph, 'a> {
    fn new(graph: &'graph CookedDiGraph<'a>, ops: &'graph [&'graph GraphOperation<'a>]) -> Self {
        Self {
            graph,
            ops,
            closure: Closure::new(graph),
        }
    }

    fn build(self) -> CookedGraphMetadata<'a> {
        let operations = self.operations();
        let HashDefault {
            hashable,
            defaultable,
        } = self.hash_default();
        let box_sccs = self.box_sccs();
        CookedGraphMetadata {
            closure: self.closure,
            box_sccs,
            hashable,
            defaultable,
            used_by: operations.used_by,
            uses: operations.uses,
            // `AtomicRefCell` doesn't implement `Clone`,
            // so we use this idiom instead of `vec!`.
            extensions: std::iter::repeat_with(AtomicRefCell::default)
                .take(self.graph.node_count())
                .collect(),
        }
    }

    fn operations(&self) -> Operations<'a> {
        let mut operations = Operations {
            uses: FxHashMap::default(),
            used_by: vec![vec![]; self.graph.node_count()],
        };

        for &&op in self.ops {
            // Forward propagation: start from the direct types, then
            // expand to the full transitive dependency set.
            let mut dependencies = FixedBitSet::with_capacity(self.graph.node_count());
            for &node in op.types() {
                dependencies.extend(self.closure.dependencies_of(node).map(|n| n.index()));
            }
            operations.uses.entry(op).insert_entry(dependencies);
        }

        // Backward propagation: mark types as used by their operations.
        for (op, deps) in &operations.uses {
            for node in deps.ones() {
                operations.used_by[node].push(*op);
            }
        }

        operations
    }

    fn box_sccs(&self) -> Vec<usize> {
        let box_edges = EdgeFiltered::from_fn(self.graph, |e| match e.weight() {
            // Inheritance edges don't contribute to cycles;
            // a type can't inherit from itself.
            GraphEdge::Inherits { .. } => false,
            GraphEdge::Contains => match self.graph[e.source()] {
                GraphType::Schema(GraphSchemaType::Container(_, c))
                | GraphType::Inline(GraphInlineType::Container(_, c)) => {
                    // Array and map containers are heap-allocated,
                    // cycles through these edges don't need `Box`.
                    !matches!(c, GraphContainer::Array { .. } | GraphContainer::Map { .. })
                }
                _ => true,
            },
            _ => true,
        });
        let mut scc = TarjanScc::new();
        scc.run(&box_edges, |_| ());
        self.graph
            .node_indices()
            .map(|node| scc.node_component_index(&box_edges, node))
            .collect()
    }

    fn hash_default(&self) -> HashDefault {
        // Mark all leaf types that can't derive `Eq` / `Hash` or `Default`.
        let n = self.graph.node_count();
        let mut unhashable = FixedBitSet::with_capacity(n);
        let mut undefaultable = FixedBitSet::with_capacity(n);
        for node in self.graph.node_indices() {
            use {GraphType::*, PrimitiveType::*};
            match &self.graph[node] {
                Schema(GraphSchemaType::Primitive(_, F32 | F64))
                | Inline(GraphInlineType::Primitive(_, F32 | F64)) => {
                    unhashable.insert(node.index());
                }
                Schema(
                    GraphSchemaType::Primitive(_, Url)
                    | GraphSchemaType::Tagged(_, _)
                    | GraphSchemaType::Untagged(_, _),
                )
                | Inline(
                    GraphInlineType::Primitive(_, Url)
                    | GraphInlineType::Tagged(_, _)
                    | GraphInlineType::Untagged(_, _),
                ) => {
                    undefaultable.insert(node.index());
                }
                _ => (),
            }
        }

        // Compute the transitive closure over the inheritance subgraph.
        let inherits = Closure::new(&EdgeFiltered::from_fn(self.graph, |e| {
            matches!(e.weight(), GraphEdge::Inherits { .. })
        }));

        // Propagate unhashability backward, from leaves to roots.
        //
        // This is conservative: if a descendant overrides an inherited
        // unhashable or undefaultable field with a different hashable or
        // defaultable type, that descendant is still marked.
        let mut queue: VecDeque<_> = unhashable.ones().map(NodeIndex::new).collect();
        while let Some(node) = queue.pop_front() {
            for edge in self.graph.edges_directed(node, Direction::Incoming) {
                let source = edge.source();
                match edge.weight() {
                    GraphEdge::Contains | GraphEdge::Variant(_) => {
                        if !unhashable.put(source.index()) {
                            queue.push_back(source);
                        }
                    }
                    GraphEdge::Field { .. } => {
                        if !unhashable.put(source.index()) {
                            queue.push_back(source);
                        }
                        // Every type that inherits from `source` also
                        // inherits this unhashable field, so mark all
                        // descendants of `source` as unhashable.
                        for desc in inherits.dependents_of(source).filter(|&d| d != source) {
                            if !unhashable.put(desc.index()) {
                                queue.push_back(desc);
                            }
                        }
                    }
                    // Don't follow inheritance edges: a parent's intrinsic
                    // unhashability (e.g., being a tagged union) doesn't
                    // make its children unhashable, because children only
                    // inherit the parent's fields, not its shape.
                    GraphEdge::Inherits { .. } => {}
                }
            }
        }

        // Propagate undefaultability backward.
        let mut queue: VecDeque<_> = undefaultable.ones().map(NodeIndex::new).collect();
        while let Some(node) = queue.pop_front() {
            for edge in self.graph.edges_directed(node, Direction::Incoming) {
                if !matches!(
                    edge.weight(),
                    GraphEdge::Field { meta, .. } if meta.required
                ) {
                    // Optional fields become `AbsentOr<T>`,
                    // which is always `Default`.
                    continue;
                }
                let source = edge.source();
                if !undefaultable.put(source.index()) {
                    queue.push_back(source);
                }
                // Every type that inherits from `source` also
                // inherits this undefaultable field, so mark all
                // descendants of `source` as undefaultable.
                for desc in inherits.dependents_of(source).filter(|&d| d != source) {
                    if !undefaultable.put(desc.index()) {
                        queue.push_back(desc);
                    }
                }
            }
        }

        HashDefault {
            hashable: invert(unhashable),
            defaultable: invert(undefaultable),
        }
    }
}

/// Inverts every bit in the bitset.
fn invert(mut bits: FixedBitSet) -> FixedBitSet {
    bits.toggle_range(..);
    bits
}

/// Visits all the types and references contained within a [`SpecType`].
#[derive(Debug)]
struct SpecTypeVisitor<'a> {
    stack: Vec<(Option<(&'a SpecType<'a>, GraphEdge<'a>)>, &'a SpecType<'a>)>,
}

impl<'a> SpecTypeVisitor<'a> {
    /// Creates a visitor with `roots` on the stack of types to visit.
    #[inline]
    fn new(roots: impl Iterator<Item = &'a SpecType<'a>>) -> Self {
        let mut stack = roots.map(|root| (None, root)).collect_vec();
        stack.reverse();
        Self { stack }
    }
}

impl<'a> Iterator for SpecTypeVisitor<'a> {
    type Item = (Option<(&'a SpecType<'a>, GraphEdge<'a>)>, &'a SpecType<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        let (parent, top) = self.stack.pop()?;
        if matches!(
            parent,
            Some((
                _,
                GraphEdge::Variant(VariantMeta::Untagged(UntaggedVariantMeta::Null))
            ))
        ) {
            // Unit variants form self-edges; skip them
            // to avoid an infinite loop.
            return Some((parent, top));
        }
        match top {
            SpecType::Schema(SpecSchemaType::Struct(_, ty))
            | SpecType::Inline(SpecInlineType::Struct(_, ty)) => {
                self.stack.extend(
                    itertools::chain!(
                        ty.fields.iter().map(|field| (
                            GraphEdge::Field {
                                shadow: false,
                                meta: FieldMeta {
                                    name: field.name,
                                    required: field.required,
                                    description: field.description,
                                    flattened: field.flattened,
                                },
                            },
                            field.ty
                        )),
                        ty.parents
                            .iter()
                            .map(|parent| (GraphEdge::Inherits { shadow: false }, *parent)),
                    )
                    .map(|(edge, ty)| (Some((top, edge)), ty))
                    .rev(),
                );
            }
            SpecType::Schema(SpecSchemaType::Untagged(_, ty))
            | SpecType::Inline(SpecInlineType::Untagged(_, ty)) => {
                self.stack.extend(
                    itertools::chain!(
                        ty.fields.iter().map(|field| (
                            GraphEdge::Field {
                                shadow: false,
                                meta: FieldMeta {
                                    name: field.name,
                                    required: field.required,
                                    description: field.description,
                                    flattened: field.flattened,
                                },
                            },
                            field.ty
                        )),
                        ty.variants.iter().map(|variant| match variant {
                            &SpecUntaggedVariant::Some(hint, ty) => {
                                let meta = UntaggedVariantMeta::Type { hint };
                                (GraphEdge::Variant(meta.into()), ty)
                            }
                            // `null` variants have no target type;
                            // we represent these variants as self-edges.
                            SpecUntaggedVariant::Null => {
                                (GraphEdge::Variant(UntaggedVariantMeta::Null.into()), top)
                            }
                        }),
                    )
                    .map(|(edge, ty)| (Some((top, edge)), ty))
                    .rev(),
                );
            }
            SpecType::Schema(SpecSchemaType::Tagged(_, ty))
            | SpecType::Inline(SpecInlineType::Tagged(_, ty)) => {
                self.stack.extend(
                    itertools::chain!(
                        ty.fields.iter().map(|field| (
                            GraphEdge::Field {
                                shadow: false,
                                meta: FieldMeta {
                                    name: field.name,
                                    required: field.required,
                                    description: field.description,
                                    flattened: field.flattened,
                                },
                            },
                            field.ty
                        )),
                        ty.variants.iter().map(|variant| (
                            GraphEdge::Variant(
                                TaggedVariantMeta {
                                    name: variant.name,
                                    aliases: variant.aliases,
                                }
                                .into()
                            ),
                            variant.ty
                        )),
                    )
                    .map(|(edge, ty)| (Some((top, edge)), ty))
                    .rev(),
                );
            }
            SpecType::Schema(SpecSchemaType::Container(_, container))
            | SpecType::Inline(SpecInlineType::Container(_, container)) => {
                self.stack
                    .push((Some((top, GraphEdge::Contains)), container.inner().ty));
            }
            SpecType::Schema(
                SpecSchemaType::Enum(..) | SpecSchemaType::Primitive(..) | SpecSchemaType::Any(_),
            )
            | SpecType::Inline(
                SpecInlineType::Enum(..) | SpecInlineType::Primitive(..) | SpecInlineType::Any(_),
            ) => (),
            SpecType::Ref(_) => (),
        }
        Some((parent, top))
    }
}

/// A map that can store one value for each type.
pub(super) type ExtensionMap = FxHashMap<TypeId, Box<dyn Extension>>;

pub trait Extension: Any + Send + Sync {
    fn into_inner(self: Box<Self>) -> Box<dyn Any>;
}

impl dyn Extension {
    #[inline]
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref::<T>()
    }
}

impl<T: Send + Sync + 'static> Extension for T {
    #[inline]
    fn into_inner(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// Strongly connected components (SCCs) in topological order.
///
/// [`TopoSccs`] uses Tarjan's single-pass algorithm to find all SCCs,
/// and provides topological ordering, efficient membership testing, and
/// condensation for computing the transitive closure. These are
/// building blocks for cycle detection and dependency propagation.
struct TopoSccs<G> {
    graph: G,
    tarjan: TarjanScc<NodeIndex<usize>>,
    sccs: Vec<Vec<usize>>,
}

impl<G> TopoSccs<G>
where
    G: Closable<NodeIndex<usize>> + Copy,
{
    fn new(graph: G) -> Self {
        let mut sccs = Vec::new();
        let mut tarjan = TarjanScc::new();
        tarjan.run(graph, |scc_nodes| {
            sccs.push(scc_nodes.iter().map(|node| node.index()).collect());
        });
        // Tarjan's algorithm returns SCCs in reverse topological order;
        // reverse them to get the topological order.
        sccs.reverse();
        Self {
            graph,
            tarjan,
            sccs,
        }
    }

    #[inline]
    fn scc_count(&self) -> usize {
        self.sccs.len()
    }

    /// Returns the topological index of the SCC that contains the given node.
    #[inline]
    fn topo_index(&self, node: NodeIndex<usize>) -> usize {
        // Tarjan's algorithm returns indices in reverse topological order;
        // inverting the component index gets us the topological index.
        self.sccs.len() - 1 - self.tarjan.node_component_index(self.graph, node)
    }

    /// Builds a condensed DAG of SCCs.
    ///
    /// The condensed graph is represented as an adjacency list, where both
    /// the node indices and the neighbors of each node are stored in
    /// topological order. This specific ordering is required by
    /// [`tred::dag_transitive_reduction_closure`].
    fn condensation(&self) -> UnweightedList<usize> {
        let mut dag = UnweightedList::with_capacity(self.scc_count());
        for to in 0..self.scc_count() {
            dag.add_node();
            for neighbor in self.sccs[to].iter().flat_map(|&index| {
                self.graph
                    .neighbors_directed(NodeIndex::new(index), Direction::Incoming)
            }) {
                let from = self.topo_index(neighbor);
                if from != to {
                    dag.update_edge(from, to, ());
                }
            }
        }
        dag
    }
}

/// The transitive closure of a graph.
#[derive(Debug)]
pub(super) struct Closure {
    /// Maps each node index to its SCC's topological index.
    scc_indices: Vec<usize>,
    /// Members of each SCC, indexed by topological SCC index.
    scc_members: Vec<Vec<usize>>,
    /// Maps each SCC to a list of all the SCCs that it transitively depends on,
    /// excluding itself.
    scc_deps: Vec<Vec<usize>>,
    /// Maps each SCC to a list of all the SCCs that transitively depend on it,
    /// excluding itself.
    scc_rdeps: Vec<Vec<usize>>,
}

impl Closure {
    /// Computes the transitive closure of a graph.
    fn new<G>(graph: G) -> Self
    where
        G: Closable<NodeIndex<usize>> + Copy,
    {
        let sccs = TopoSccs::new(graph);
        let condensation = sccs.condensation();
        let (_, closure) = tred::dag_transitive_reduction_closure(&condensation);

        // Build the forward and reverse adjacency lists
        // from the transitive closure graph.
        let scc_deps = (0..sccs.scc_count())
            .map(|scc| closure.neighbors(scc).collect_vec())
            .collect_vec();
        let mut scc_rdeps = vec![vec![]; sccs.scc_count()];
        for (scc, deps) in scc_deps.iter().enumerate() {
            for &dep in deps {
                scc_rdeps[dep].push(scc);
            }
        }

        let mut scc_indices = vec![0; graph.node_count()];
        for node in graph.node_identifiers() {
            scc_indices[node.index()] = sccs.topo_index(node);
        }

        Closure {
            scc_indices,
            scc_members: sccs.sccs.iter().cloned().collect_vec(),
            scc_deps,
            scc_rdeps,
        }
    }

    /// Returns the topological SCC index for the given node.
    #[inline]
    pub fn scc_index_of(&self, node: NodeIndex<usize>) -> usize {
        self.scc_indices[node.index()]
    }

    /// Iterates over all nodes that `node` transitively depends on,
    /// including `node` and all members of its SCC.
    pub fn dependencies_of(
        &self,
        node: NodeIndex<usize>,
    ) -> impl Iterator<Item = NodeIndex<usize>> {
        let scc = self.scc_index_of(node);
        std::iter::once(scc)
            .chain(self.scc_deps[scc].iter().copied())
            .flat_map(|s| self.scc_members[s].iter().copied()) // Expand SCCs to nodes.
            .map(NodeIndex::new)
    }

    /// Iterates over all nodes that transitively depend on `node`,
    /// including `node` and all members of its SCC.
    pub fn dependents_of(&self, node: NodeIndex<usize>) -> impl Iterator<Item = NodeIndex<usize>> {
        let scc = self.scc_index_of(node);
        std::iter::once(scc)
            .chain(self.scc_rdeps[scc].iter().copied())
            .flat_map(|s| self.scc_members[s].iter().copied())
            .map(NodeIndex::new)
    }

    /// Returns whether `node` transitively depends on `other`,
    /// or `false` when `node == other`.
    #[inline]
    pub fn depends_on(&self, node: NodeIndex<usize>, other: NodeIndex<usize>) -> bool {
        if node == other {
            return false;
        }
        let scc = self.scc_index_of(node);
        let other_scc = self.scc_index_of(other);
        scc == other_scc || self.scc_deps[scc].contains(&other_scc)
    }
}

/// Trait requirements for computing a transitive closure.
trait Closable<N>:
    NodeCount
    + IntoNodeIdentifiers<NodeId = N>
    + IntoNeighbors<NodeId = N>
    + IntoNeighborsDirected<NodeId = N>
    + NodeIndexable<NodeId = N>
{
}

impl<N, G> Closable<N> for G where
    G: NodeCount
        + IntoNodeIdentifiers<NodeId = N>
        + IntoNeighbors<NodeId = N>
        + IntoNeighborsDirected<NodeId = N>
        + NodeIndexable<NodeId = N>
{
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::tests::assert_matches;

    /// Creates a simple graph: `A -> B -> C`.
    fn linear_graph() -> DiGraph<(), (), usize> {
        let mut g = DiGraph::default();
        let a = g.add_node(());
        let b = g.add_node(());
        let c = g.add_node(());
        g.extend_with_edges([(a, b), (b, c)]);
        g
    }

    /// Creates a cyclic graph: `A -> B -> C -> A`, with `D -> A`.
    fn cyclic_graph() -> DiGraph<(), (), usize> {
        let mut g = DiGraph::default();
        let a = g.add_node(());
        let b = g.add_node(());
        let c = g.add_node(());
        let d = g.add_node(());
        g.extend_with_edges([(a, b), (b, c), (c, a), (d, a)]);
        g
    }

    // MARK: SCC detection

    #[test]
    fn test_linear_graph_has_singleton_sccs() {
        let g = linear_graph();
        let sccs = TopoSccs::new(&g);
        let sizes = sccs.sccs.iter().map(|scc| scc.len()).collect_vec();
        assert_matches!(&*sizes, [1, 1, 1]);
    }

    #[test]
    fn test_cyclic_graph_has_one_multi_node_scc() {
        let g = cyclic_graph();
        let sccs = TopoSccs::new(&g);

        // A-B-C form one SCC; D is its own SCC. Since D has an edge to
        // the cycle, D must precede the cycle in topological order.
        let sizes = sccs.sccs.iter().map(|scc| scc.len()).collect_vec();
        assert_matches!(&*sizes, [1, 3]);
    }

    // MARK: Topological ordering

    #[test]
    fn test_sccs_are_in_topological_order() {
        let g = cyclic_graph();
        let sccs = TopoSccs::new(&g);

        let d_topo = sccs.topo_index(3.into());
        let a_topo = sccs.topo_index(0.into());
        assert!(
            d_topo < a_topo,
            "D should precede A-B-C in topological order"
        );
    }

    #[test]
    fn test_topo_index_consistent_within_scc() {
        let g = cyclic_graph();
        let sccs = TopoSccs::new(&g);

        // A, B, C are in the same SCC, so they should have
        // the same topological index.
        let a_topo = sccs.topo_index(0.into());
        let b_topo = sccs.topo_index(1.into());
        let c_topo = sccs.topo_index(2.into());

        assert_eq!(a_topo, b_topo);
        assert_eq!(b_topo, c_topo);
    }

    // MARK: Condensation

    #[test]
    fn test_condensation_has_correct_node_count() {
        let g = cyclic_graph();
        let sccs = TopoSccs::new(&g);
        let dag = sccs.condensation();

        assert_eq!(dag.node_count(), 2);
    }

    #[test]
    fn test_condensation_has_correct_edges() {
        let g = cyclic_graph();
        let sccs = TopoSccs::new(&g);
        let dag = sccs.condensation();

        // D should have an edge to the A-B-C SCC, and
        // A-B-C shouldn't create a self-loop.
        let d_topo = sccs.topo_index(3.into());
        let abc_topo = sccs.topo_index(0.into());

        let d_neighbors = dag.neighbors(d_topo).collect_vec();
        assert_eq!(&*d_neighbors, [abc_topo]);

        let abc_neighbors = dag.neighbors(abc_topo).collect_vec();
        assert!(abc_neighbors.is_empty());
    }

    #[test]
    fn test_condensation_neighbors_in_topological_order() {
        // Matches Petgraph's `dag_to_toposorted_adjacency_list` example:
        // edges added as `(top, second), (top, first)`, but neighbors should be
        // `[first, second]` in topological order.
        let mut g = DiGraph::<(), (), usize>::default();
        let second = g.add_node(());
        let top = g.add_node(());
        let first = g.add_node(());
        g.extend_with_edges([(top, second), (top, first), (first, second)]);

        let sccs = TopoSccs::new(&g);
        let dag = sccs.condensation();

        let top_topo = sccs.topo_index(top);
        assert_eq!(top_topo, 0);

        let first_topo = sccs.topo_index(first);
        assert_eq!(first_topo, 1);

        let second_topo = sccs.topo_index(second);
        assert_eq!(second_topo, 2);

        let neighbors = dag.neighbors(top_topo).collect_vec();
        assert_eq!(&*neighbors, [first_topo, second_topo]);
    }
}
