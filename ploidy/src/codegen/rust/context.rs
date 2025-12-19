use semver::Version;

use crate::ir::{InnerLeaf, InnerRef, IrSpec, IrType, PrimitiveIrType};

use super::SchemaIdentMap;

#[derive(Debug)]
pub struct CodegenContext<'a> {
    pub name: &'a str,
    pub version: Version,
    pub license: &'a str,
    pub description: Option<&'a str>,
    pub spec: &'a IrSpec<'a>,
    pub map: SchemaIdentMap<'a>,
}

impl<'a> CodegenContext<'a> {
    pub fn new(
        name: &'a str,
        version: Version,
        license: &'a str,
        description: Option<&'a str>,
        spec: &'a IrSpec<'a>,
    ) -> Self {
        Self {
            name,
            version,
            description,
            license,
            spec,
            map: SchemaIdentMap::new(spec),
        }
    }

    pub fn hashable(&self, ty: &IrType<'_>) -> bool {
        itertools::chain!(
            ty.visit::<InnerLeaf>(),
            ty.visit::<InnerRef>()
                .flat_map(|r| self.spec.lookup(r.name()))
                .flat_map(|view| view.refs())
                .flat_map(|view| view.ty().visit::<InnerLeaf>()),
        )
        .all(|leaf| {
            !matches!(
                leaf,
                InnerLeaf::Primitive(PrimitiveIrType::F32 | PrimitiveIrType::F64)
            )
        })
    }
}
