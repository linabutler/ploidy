use cargo_toml::Manifest;
pub use toml::Value as TomlValue;

use crate::ir::{InnerLeaf, InnerRef, IrSpec, IrType, PrimitiveIrType};

use super::{CargoMetadata, SchemaIdentMap};

pub type TomlMap = toml::map::Map<String, TomlValue>;

#[derive(Debug)]
pub struct CodegenContext<'a> {
    pub spec: &'a IrSpec<'a>,
    pub manifest: &'a Manifest<CargoMetadata>,
    pub map: SchemaIdentMap<'a>,
}

impl<'a> CodegenContext<'a> {
    pub fn new(spec: &'a IrSpec<'a>, manifest: &'a Manifest<CargoMetadata>) -> Self {
        Self {
            spec,
            manifest,
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
