use std::collections::{BTreeMap, BTreeSet};

use cargo_toml::{Edition, Manifest};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use toml::Value as TomlValue;

use crate::codegen::IntoCode;

use super::context::CodegenContext;

type TomlMap = toml::map::Map<String, TomlValue>;

const PLOIDY_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug)]
pub struct CodegenCargoManifest<'a> {
    context: &'a CodegenContext<'a>,
}

impl<'a> CodegenCargoManifest<'a> {
    #[inline]
    pub fn new(context: &'a CodegenContext<'a>) -> Self {
        Self { context }
    }

    pub fn to_manifest(self) -> Manifest<CargoMetadata> {
        let mut manifest = self.context.manifest.clone();

        // Ploidy generates Rust 2024-compatible code.
        manifest
            .package
            .as_mut()
            .unwrap()
            .edition
            .set(Edition::E2024);

        let features = {
            let names: BTreeSet<_> = self
                .context
                .spec
                .operations()
                .map(|view| view.op().resource)
                .filter(|&name| name != "full")
                .collect();
            let mut features: BTreeMap<_, _> = names
                .iter()
                .map(|&name| (name.to_owned(), vec![]))
                .collect();
            features.insert(
                "full".to_owned(),
                names.iter().map(|&name| name.to_owned()).collect_vec(),
            );
            features.insert("default".to_owned(), vec![]);
            features
        };

        let dependencies = toml::toml! {
            bytes = { version = "1", features = ["serde"] }
            chrono = { version = "0.4", features = ["serde"] }
            http = "1"
            ploidy-util = PLOIDY_VERSION
            reqwest = { version = "0.12", default-features = false, features = ["http2", "json", "multipart", "rustls-tls"] }
            serde = { version = "1", features = ["derive"] }
            serde_json = "1"
            serde_path_to_error = "0.1"
            thiserror = "2"
            url = { version = "2.5", features = ["serde"] }
            uuid = { version = "1", features = ["serde", "v4"] }
        }.try_into().unwrap();

        Manifest {
            features,
            dependencies,
            ..manifest
        }
    }
}

impl IntoCode for CodegenCargoManifest<'_> {
    type Code = (&'static str, Manifest<CargoMetadata>);

    fn into_code(self) -> Self::Code {
        ("Cargo.toml", self.to_manifest())
    }
}

/// Cargo metadata of any type.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct CargoMetadata(TomlValue);

impl Default for CargoMetadata {
    fn default() -> Self {
        Self(TomlMap::default().into())
    }
}
