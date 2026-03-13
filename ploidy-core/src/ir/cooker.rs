//! Raw-to-cooked IR type translation.

use petgraph::graph::NodeIndex;

use crate::arena::Arena;

use super::{
    graph::{CookedGraphNode, GraphNode, RawGraphNode},
    types::{
        CookedContainer, CookedInlineType, CookedOperation, CookedParameter, CookedSchemaType,
        CookedStruct, CookedTagged, CookedUntagged, RawContainer, RawInlineType, RawOperation,
        RawParameter, RawSchemaType, RawStruct, RawTagged, RawType, RawUntagged, shape,
    },
};

/// Turns raw types with [`RawType`] references into
/// cooked types with [`NodeIndex`] references.
pub struct Cooker<'a, F> {
    arena: &'a Arena,
    resolve: F,
}

impl<'a, F> Cooker<'a, F>
where
    F: Fn(&'a RawType<'a>) -> NodeIndex<usize>,
{
    /// Creates a new cooker with the given arena and reference resolver.
    pub fn new(arena: &'a Arena, resolve: F) -> Self {
        Self { arena, resolve }
    }

    /// Cooks a [`RawGraphNode`] by resolving all type references within it.
    pub fn node(&self, raw: RawGraphNode<'a>) -> CookedGraphNode<'a> {
        use GraphNode::*;
        match raw {
            Schema(ty) => Schema(self.arena.alloc(self.schema(ty))),
            Inline(ty) => Inline(self.arena.alloc(self.inline(ty))),
        }
    }

    /// Cooks a [`RawOperation`] and all the types it references.
    pub fn operation(&self, raw: &RawOperation<'a>) -> &'a CookedOperation<'a> {
        use shape::{Operation, Request, Response};
        self.arena.alloc(Operation {
            id: raw.id,
            method: raw.method,
            path: raw.path,
            resource: raw.resource,
            description: raw.description,
            params: self
                .arena
                .alloc_slice_exact(raw.params.iter().map(|p| self.param(p))),
            request: raw.request.as_ref().map(|r| match r {
                Request::Json(ty) => Request::Json(self.resolve(ty)),
                Request::Multipart => Request::Multipart,
            }),
            response: raw.response.as_ref().map(|r| match r {
                Response::Json(ty) => Response::Json(self.resolve(ty)),
            }),
        })
    }

    fn schema(&self, raw: &RawSchemaType<'a>) -> CookedSchemaType<'a> {
        use shape::SchemaType::*;
        match *raw {
            Enum(info, e) => Enum(info, e),
            Struct(info, ref s) => Struct(info, self.struct_(s)),
            Tagged(info, ref t) => Tagged(info, self.tagged(t)),
            Untagged(info, ref u) => Untagged(info, self.untagged(u)),
            Container(info, ref c) => Container(info, self.container(c)),
            Primitive(info, p) => Primitive(info, p),
            Any(info) => Any(info),
        }
    }

    fn inline(&self, raw: &RawInlineType<'a>) -> CookedInlineType<'a> {
        use shape::InlineType::*;
        match *raw {
            Enum(path, e) => Enum(path, e),
            Struct(path, ref s) => Struct(path, self.struct_(s)),
            Tagged(path, ref t) => Tagged(path, self.tagged(t)),
            Untagged(path, ref u) => Untagged(path, self.untagged(u)),
            Container(path, ref c) => Container(path, self.container(c)),
            Primitive(path, p) => Primitive(path, p),
            Any(path) => Any(path),
        }
    }

    fn struct_(&self, raw: &RawStruct<'a>) -> CookedStruct<'a> {
        use shape::{Struct, StructField};
        Struct {
            description: raw.description,
            fields: self
                .arena
                .alloc_slice_exact(raw.fields.iter().map(|f| StructField {
                    name: f.name,
                    ty: self.resolve(f.ty),
                    required: f.required,
                    description: f.description,
                    flattened: f.flattened,
                })),
            parents: self
                .arena
                .alloc_slice_exact(raw.parents.iter().map(|p| self.resolve(p))),
        }
    }

    fn tagged(&self, raw: &RawTagged<'a>) -> CookedTagged<'a> {
        use shape::{Tagged, TaggedVariant};
        Tagged {
            description: raw.description,
            tag: raw.tag,
            variants: self
                .arena
                .alloc_slice_exact(raw.variants.iter().map(|v| TaggedVariant {
                    name: v.name,
                    aliases: v.aliases,
                    ty: self.resolve(v.ty),
                })),
        }
    }

    fn untagged(&self, raw: &RawUntagged<'a>) -> CookedUntagged<'a> {
        use shape::{Untagged, UntaggedVariant};
        Untagged {
            description: raw.description,
            variants: self
                .arena
                .alloc_slice_exact(raw.variants.iter().map(|v| match v {
                    &UntaggedVariant::Some(hint, ty) => {
                        UntaggedVariant::Some(hint, self.resolve(ty))
                    }
                    UntaggedVariant::Null => UntaggedVariant::Null,
                })),
        }
    }

    fn container(&self, raw: &RawContainer<'a>) -> CookedContainer<'a> {
        use shape::{Container::*, Inner};
        let inner = raw.inner();
        let cooked = Inner {
            description: inner.description,
            ty: self.resolve(inner.ty),
        };
        match raw {
            Array(_) => Array(cooked),
            Map(_) => Map(cooked),
            Optional(_) => Optional(cooked),
        }
    }

    fn param(&self, raw: &RawParameter<'a>) -> CookedParameter<'a> {
        use shape::{Parameter::*, ParameterInfo};
        let (Path(info) | Query(info)) = raw;
        let cooked = ParameterInfo {
            name: info.name,
            ty: self.resolve(info.ty),
            required: info.required,
            description: info.description,
            style: info.style,
        };
        match raw {
            Path(_) => Path(cooked),
            Query(_) => Query(cooked),
        }
    }

    /// Resolves a raw type reference to a cooked graph index.
    #[inline]
    fn resolve(&self, ty: &'a RawType<'a>) -> NodeIndex<usize> {
        (self.resolve)(ty)
    }
}
