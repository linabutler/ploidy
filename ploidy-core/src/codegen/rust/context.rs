use cargo_toml::Manifest;
pub use toml::Value as TomlValue;

use crate::ir::{IrGraph, IrType, IrTypeRef, PrimitiveIrType};

use super::{CargoMetadata, SchemaIdentMap};

pub type TomlMap = toml::map::Map<String, TomlValue>;

#[derive(Debug)]
pub struct CodegenContext<'a> {
    pub graph: &'a IrGraph<'a>,
    pub manifest: &'a Manifest<CargoMetadata>,
    pub map: SchemaIdentMap<'a>,
}

impl<'a> CodegenContext<'a> {
    pub fn new(graph: &'a IrGraph<'a>, manifest: &'a Manifest<CargoMetadata>) -> Self {
        Self {
            graph,
            manifest,
            map: SchemaIdentMap::new(graph),
        }
    }

    pub fn hashable(&self, ty: &IrType<'_>) -> bool {
        let Some(view) = self.graph.lookup(ty.as_ref()) else {
            return false;
        };
        view.reachable().all(|view| {
            !matches!(
                view.to_ref(),
                IrTypeRef::Primitive(PrimitiveIrType::F32 | PrimitiveIrType::F64)
            )
        })
    }
}
