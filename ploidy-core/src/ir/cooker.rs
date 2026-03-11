//! Raw-to-cooked IR type translation.

use petgraph::graph::NodeIndex;

use crate::arena::Arena;

use super::{
    graph::GraphNode,
    types::{
        Container, InlineIrType, Inner, IrOperation, IrParameter, IrParameterInfo, IrRequest,
        IrResponse, IrStruct, IrStructField, IrTagged, IrTaggedVariant, IrType, IrUntagged,
        IrUntaggedVariant, SchemaIrType,
    },
};

/// Turns raw IR types with [`IrType`] references into
/// cooked types with [`NodeIndex`] references.
pub struct Cooker<'a, F> {
    arena: &'a Arena,
    resolve: F,
}

impl<'a, F> Cooker<'a, F>
where
    F: Fn(&'a IrType<'a>) -> NodeIndex<usize>,
{
    /// Creates a new cooker with the given arena and reference resolver.
    pub fn new(arena: &'a Arena, resolve: F) -> Self {
        Self { arena, resolve }
    }

    /// Cooks a raw [`GraphNode`] by resolving all type references within it.
    pub fn node(&self, raw: GraphNode<'a>) -> GraphNode<'a, NodeIndex<usize>> {
        use GraphNode::*;
        match raw {
            Schema(ty) => Schema(self.arena.alloc(self.schema(ty))),
            Inline(ty) => Inline(self.arena.alloc(self.inline(ty))),
        }
    }

    /// Cooks an [`IrOperation`] and all the types it references.
    pub fn operation(&self, raw: &IrOperation<'a>) -> &'a IrOperation<'a, NodeIndex<usize>> {
        self.arena.alloc(IrOperation {
            id: raw.id,
            method: raw.method,
            path: raw.path,
            resource: raw.resource,
            description: raw.description,
            params: self
                .arena
                .alloc_slice_exact(raw.params.iter().map(|p| self.param(p))),
            request: raw.request.as_ref().map(|r| match r {
                IrRequest::Json(ty) => IrRequest::Json(self.resolve(ty)),
                IrRequest::Multipart => IrRequest::Multipart,
            }),
            response: raw.response.as_ref().map(|r| match r {
                IrResponse::Json(ty) => IrResponse::Json(self.resolve(ty)),
            }),
        })
    }

    fn schema(&self, raw: &SchemaIrType<'a>) -> SchemaIrType<'a, NodeIndex<usize>> {
        use SchemaIrType::*;
        match raw {
            Enum(info, e) => Enum(*info, *e),
            Struct(info, s) => Struct(*info, self.struct_(s)),
            Tagged(info, t) => Tagged(*info, self.tagged(t)),
            Untagged(info, u) => Untagged(*info, self.untagged(u)),
            Container(info, c) => Container(*info, self.container(c)),
            &Primitive(info, p) => Primitive(info, p),
            &Any(info) => Any(info),
        }
    }

    fn inline(&self, raw: &InlineIrType<'a>) -> InlineIrType<'a, NodeIndex<usize>> {
        use InlineIrType::*;
        match raw {
            Enum(path, e) => Enum(*path, *e),
            Struct(path, s) => Struct(*path, self.struct_(s)),
            Tagged(path, t) => Tagged(*path, self.tagged(t)),
            Untagged(path, u) => Untagged(*path, self.untagged(u)),
            Container(path, c) => Container(*path, self.container(c)),
            &Primitive(path, p) => Primitive(path, p),
            &Any(path) => Any(path),
        }
    }

    fn struct_(&self, raw: &IrStruct<'a>) -> IrStruct<'a, NodeIndex<usize>> {
        IrStruct {
            description: raw.description,
            fields: self
                .arena
                .alloc_slice_exact(raw.fields.iter().map(|f| IrStructField {
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

    fn tagged(&self, raw: &IrTagged<'a>) -> IrTagged<'a, NodeIndex<usize>> {
        IrTagged {
            description: raw.description,
            tag: raw.tag,
            variants: self
                .arena
                .alloc_slice_exact(raw.variants.iter().map(|v| IrTaggedVariant {
                    name: v.name,
                    aliases: v.aliases,
                    ty: self.resolve(v.ty),
                })),
        }
    }

    fn untagged(&self, raw: &IrUntagged<'a>) -> IrUntagged<'a, NodeIndex<usize>> {
        IrUntagged {
            description: raw.description,
            variants: self
                .arena
                .alloc_slice_exact(raw.variants.iter().map(|v| match v {
                    IrUntaggedVariant::Some(hint, ty) => {
                        IrUntaggedVariant::Some(*hint, self.resolve(ty))
                    }
                    IrUntaggedVariant::Null => IrUntaggedVariant::Null,
                })),
        }
    }

    fn container(&self, raw: &Container<'a>) -> Container<'a, NodeIndex<usize>> {
        let inner = raw.inner();
        let cooked = Inner {
            description: inner.description,
            ty: self.resolve(inner.ty),
        };
        match raw {
            Container::Array(_) => Container::Array(cooked),
            Container::Map(_) => Container::Map(cooked),
            Container::Optional(_) => Container::Optional(cooked),
        }
    }

    fn param(&self, raw: &IrParameter<'a>) -> IrParameter<'a, NodeIndex<usize>> {
        let (IrParameter::Path(info) | IrParameter::Query(info)) = raw;
        let cooked = IrParameterInfo {
            name: info.name,
            ty: self.resolve(info.ty),
            required: info.required,
            description: info.description,
            style: info.style,
        };
        match raw {
            IrParameter::Path(_) => IrParameter::Path(cooked),
            IrParameter::Query(_) => IrParameter::Query(cooked),
        }
    }

    /// Resolves a raw type reference to a cooked graph index.
    #[inline]
    fn resolve(&self, ty: &'a IrType<'a>) -> NodeIndex<usize> {
        (self.resolve)(ty)
    }
}
