use super::types::{InlineIrType, IrType, IrUntaggedVariant, PrimitiveIrType, SchemaIrType};

/// An inner type within a schema.
pub trait Visitable<'a>: Sized {
    fn accept(inner: InnerIrType<'a>) -> Option<Self>;
}

/// An inner reference to another schema.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InnerRef<'a>(&'a str);

impl<'a> InnerRef<'a> {
    #[inline]
    pub fn name(self) -> &'a str {
        self.0
    }
}

impl<'a> Visitable<'a> for InnerRef<'a> {
    #[inline]
    fn accept(inner: InnerIrType<'a>) -> Option<Self> {
        match inner {
            InnerIrType::Ref(name) => Some(Self(name)),
            _ => None,
        }
    }
}

/// An inner leaf type that doesn't reference or contain nested schemas.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InnerLeaf {
    Any,
    Primitive(PrimitiveIrType),
}

impl<'a> Visitable<'a> for InnerLeaf {
    #[inline]
    fn accept(inner: InnerIrType<'a>) -> Option<Self> {
        match inner {
            InnerIrType::Any => Some(InnerLeaf::Any),
            InnerIrType::Primitive(ty) => Some(InnerLeaf::Primitive(ty)),
            _ => None,
        }
    }
}

impl<'a> Visitable<'a> for &InlineIrType<'a> {
    #[inline]
    fn accept(inner: InnerIrType<'a>) -> Option<Self> {
        match inner {
            InnerIrType::Inline(ty) => Some(ty),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct Visitor<'a> {
    stack: Vec<&'a IrType<'a>>,
}

impl<'a> Visitor<'a> {
    #[inline]
    pub fn new(root: &'a IrType<'a>) -> Self {
        Self { stack: vec![root] }
    }

    #[inline]
    pub fn for_schema_ty(root: &'a SchemaIrType<'a>) -> Self {
        let stack = match root {
            SchemaIrType::Struct(_, ty) => ty.fields.iter().map(|field| &field.ty).rev().collect(),
            SchemaIrType::Untagged(_, ty) => ty
                .variants
                .iter()
                .filter_map(|variant| match variant {
                    IrUntaggedVariant::Some(_, ty) => Some(ty),
                    _ => None,
                })
                .rev()
                .collect(),
            SchemaIrType::Tagged(_, ty) => ty.variants.iter().map(|name| &name.ty).rev().collect(),
            SchemaIrType::Enum(..) => vec![],
        };
        Self { stack }
    }
}

impl<'a> Iterator for Visitor<'a> {
    type Item = InnerIrType<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(top) = self.stack.pop() {
            match top {
                IrType::Array(ty) => {
                    self.stack.push(ty.as_ref());
                }
                IrType::Map(ty) => {
                    self.stack.push(ty.as_ref());
                }
                IrType::Nullable(ty) => {
                    self.stack.push(ty.as_ref());
                }
                IrType::Schema(SchemaIrType::Struct(_, ty)) => {
                    self.stack
                        .extend(ty.fields.iter().map(|field| &field.ty).rev());
                }
                IrType::Schema(SchemaIrType::Untagged(_, ty)) => {
                    self.stack.extend(
                        ty.variants
                            .iter()
                            .filter_map(|variant| match variant {
                                IrUntaggedVariant::Some(_, ty) => Some(ty),
                                _ => None,
                            })
                            .rev(),
                    );
                }
                IrType::Schema(SchemaIrType::Tagged(_, ty)) => {
                    self.stack
                        .extend(ty.variants.iter().map(|name| &name.ty).rev());
                }
                IrType::Schema(SchemaIrType::Enum(..)) => continue,
                IrType::Any => return Some(InnerIrType::Any),
                &IrType::Primitive(ty) => return Some(InnerIrType::Primitive(ty)),
                IrType::Inline(ty) => {
                    match ty {
                        InlineIrType::Enum(..) => {}
                        InlineIrType::Untagged(_, ty) => self.stack.extend(
                            ty.variants
                                .iter()
                                .filter_map(|variant| match variant {
                                    IrUntaggedVariant::Some(_, ty) => Some(ty),
                                    _ => None,
                                })
                                .rev(),
                        ),
                        InlineIrType::Struct(_, ty) => self
                            .stack
                            .extend(ty.fields.iter().map(|field| &field.ty).rev()),
                    }
                    return Some(InnerIrType::Inline(ty));
                }
                &IrType::Ref(r) => return Some(InnerIrType::Ref(r.name())),
            }
        }
        None
    }
}

#[derive(Clone, Copy, Debug)]
pub enum InnerIrType<'a> {
    Any,
    Primitive(PrimitiveIrType),
    Inline(&'a InlineIrType<'a>),
    Ref(&'a str),
}
