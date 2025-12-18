use std::collections::BTreeSet;

use itertools::Itertools;
use toml::Value as TomlValue;

use crate::codegen::IntoCode;

use super::context::CodegenContext;

type TomlMap = toml::map::Map<String, TomlValue>;

#[derive(Clone, Debug)]
pub struct CargoManifest<'a> {
    context: &'a CodegenContext<'a>,
}

impl<'a> CargoManifest<'a> {
    #[inline]
    pub fn new(context: &'a CodegenContext<'a>) -> Self {
        Self { context }
    }

    pub fn into_map(self) -> TomlMap {
        let name = self.context.name;
        let version = self.context.version.to_string();
        let license = self.context.license;
        let mut package = toml::toml! {
            name = name
            version = version
            edition = "2024"
            license = license
        };
        if let Some(description) = self.context.description {
            package.insert("description".into(), description.into());
        }

        let features = {
            let names: BTreeSet<_> = self
                .context
                .spec
                .operations()
                .map(|view| view.op().resource)
                .filter(|&name| name != "full")
                .collect();
            let mut features = names
                .iter()
                .map(|&name| (name.to_owned(), TomlValue::Array(vec![])))
                .collect::<TomlMap>();
            features.insert(
                "full".to_owned(),
                names
                    .iter()
                    .map(|&name| name.to_owned())
                    .collect_vec()
                    .into(),
            );
            features.insert("default".to_owned(), TomlValue::Array(vec![]));
            features
        };

        toml::toml! {
            package = package
            features = features

            [dependencies]
            bytes = { version = "1", features = ["serde"] }
            chrono = { version = "0.4", features = ["serde"] }
            http = "1"
            reqwest = { version = "0.12", default-features = false, features = ["http2", "json", "multipart", "rustls-tls"] }
            serde = { version = "1", features = ["derive"] }
            serde_json = "1"
            serde_path_to_error = "0.1"
            thiserror = "2"
            url = { version = "2.5", features = ["serde"] }
            uuid = { version = "1", features = ["serde", "v4"] }
        }
    }
}

impl IntoCode for CargoManifest<'_> {
    type Code = (&'static str, TomlMap);

    fn into_code(self) -> Self::Code {
        ("Cargo.toml", self.into_map())
    }
}
