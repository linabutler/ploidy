//! IR type reference translation.

use std::marker::PhantomData;

use crate::arena::Arena;

use crate::ir::types::shape::{
    Container, InlineType, Inner, Operation, Parameter, ParameterInfo, Request, Response,
    SchemaType, Struct, StructField, Tagged, TaggedVariant, Untagged, UntaggedVariant,
};

/// Translates shape types parameterized by `T` references into
/// shape types parameterized by `U` references.
///
/// Mapping isn't recursive: the mapper translates references within
/// each type, but doesn't follow them to map the types they point to.
pub struct TypeMapper<'a, F, T, U> {
    arena: &'a Arena,
    map: F,
    _phantom: PhantomData<fn(T) -> U>,
}

impl<'a, F, T: Copy, U: Copy> TypeMapper<'a, F, T, U>
where
    F: Fn(T) -> U,
{
    /// Creates a new type mapper with the given arena and mapping function.
    pub fn new(arena: &'a Arena, map: F) -> Self {
        Self {
            arena,
            map,
            _phantom: PhantomData,
        }
    }

    /// Maps all the types that an [`Operation`] references.
    pub fn operation(&self, raw: &Operation<'a, T>) -> &'a Operation<'a, U> {
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
                Request::Json(ty) => Request::Json(self.map(*ty)),
                Request::Multipart => Request::Multipart,
            }),
            response: raw.response.as_ref().map(|r| match r {
                Response::Json(ty) => Response::Json(self.map(*ty)),
            }),
        })
    }

    /// Maps all the types that a [`SchemaType`] references.
    pub fn schema(&self, raw: &SchemaType<'a, T>) -> SchemaType<'a, U> {
        use SchemaType::*;
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

    /// Maps all the types that an [`InlineType`] references.
    pub fn inline(&self, raw: &InlineType<'a, T>) -> InlineType<'a, U> {
        use InlineType::*;
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

    fn struct_(&self, raw: &Struct<'a, T>) -> Struct<'a, U> {
        Struct {
            description: raw.description,
            fields: self
                .arena
                .alloc_slice_exact(raw.fields.iter().map(|f| StructField {
                    name: f.name,
                    ty: self.map(f.ty),
                    required: f.required,
                    description: f.description,
                    flattened: f.flattened,
                })),
            parents: self
                .arena
                .alloc_slice_exact(raw.parents.iter().map(|p| self.map(*p))),
        }
    }

    fn tagged(&self, raw: &Tagged<'a, T>) -> Tagged<'a, U> {
        Tagged {
            description: raw.description,
            tag: raw.tag,
            variants: self
                .arena
                .alloc_slice_exact(raw.variants.iter().map(|v| TaggedVariant {
                    name: v.name,
                    aliases: v.aliases,
                    ty: self.map(v.ty),
                })),
            fields: self.fields(raw.fields),
        }
    }

    fn untagged(&self, raw: &Untagged<'a, T>) -> Untagged<'a, U> {
        Untagged {
            description: raw.description,
            variants: self
                .arena
                .alloc_slice_exact(raw.variants.iter().map(|v| match v {
                    &UntaggedVariant::Some(hint, ty) => UntaggedVariant::Some(hint, self.map(ty)),
                    UntaggedVariant::Null => UntaggedVariant::Null,
                })),
            fields: self.fields(raw.fields),
        }
    }

    fn fields(&self, raw: &[StructField<'a, T>]) -> &'a [StructField<'a, U>] {
        self.arena
            .alloc_slice_exact(raw.iter().map(|f| StructField {
                name: f.name,
                ty: self.map(f.ty),
                required: f.required,
                description: f.description,
                flattened: f.flattened,
            }))
    }

    fn container(&self, raw: &Container<'a, T>) -> Container<'a, U> {
        use Container::*;
        match raw {
            Array(inner) => Array(self.inner(inner)),
            Map(inner) => Map(self.inner(inner)),
            Optional(inner) => Optional(self.inner(inner)),
        }
    }

    fn inner(&self, inner: &Inner<'a, T>) -> Inner<'a, U> {
        Inner {
            description: inner.description,
            ty: self.map(inner.ty),
        }
    }

    fn param(&self, raw: &Parameter<'a, T>) -> Parameter<'a, U> {
        use Parameter::*;
        match raw {
            Path(info) => Path(self.param_info(info)),
            Query(info) => Query(self.param_info(info)),
        }
    }

    fn param_info(&self, info: &ParameterInfo<'a, T>) -> ParameterInfo<'a, U> {
        ParameterInfo {
            name: info.name,
            ty: self.map(info.ty),
            required: info.required,
            description: info.description,
            style: info.style,
        }
    }

    /// Maps a type reference from `T` to `U`.
    fn map(&self, ty: T) -> U {
        (self.map)(ty)
    }
}
