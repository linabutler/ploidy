use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error as StdError,
    fmt::{Debug, Display},
    ops::Range,
    path::Path,
};

use itertools::Itertools;
use miette::SourceSpan;
use ploidy_core::{codegen::Code, ir::View};
use semver::Version;
use serde::{Deserialize, de::IntoDeserializer};
use toml_edit::{Array, DocumentMut, InlineTable, Table, TableLike, value};

use super::{config::CodegenConfig, graph::CodegenGraph, naming::CargoFeature};

const PLOIDY_VERSION: Version = {
    const fn parse(value: &'static str) -> u64 {
        match u64::from_str_radix(value, 10) {
            Ok(v) => v,
            Err(_) => unreachable!(),
        }
    }
    Version::new(
        parse(env!("CARGO_PKG_VERSION_MAJOR")),
        parse(env!("CARGO_PKG_VERSION_MINOR")),
        parse(env!("CARGO_PKG_VERSION_PATCH")),
    )
};

#[derive(Clone, Debug)]
pub struct CodegenCargoManifest<'a> {
    graph: &'a CodegenGraph<'a>,
    manifest: &'a CargoManifest,
}

impl<'a> CodegenCargoManifest<'a> {
    #[inline]
    pub fn new(graph: &'a CodegenGraph<'a>, manifest: &'a CargoManifest) -> Self {
        Self { graph, manifest }
    }

    pub fn to_manifest(self) -> CargoManifest {
        // Translate resource names from operations and schemas into
        // Cargo feature names with dependencies.
        let features = {
            let mut deps_by_feature = BTreeMap::new();

            // For each schema type with an explicitly declared resource name,
            // use the resource name as the feature name, and enable features
            // for all its transitive dependencies.
            for schema in self.graph.schemas() {
                let feature = match schema.resource().map(CargoFeature::from_name) {
                    Some(CargoFeature::Named(name)) => CargoFeature::Named(name),
                    _ => continue,
                };
                let entry: &mut BTreeSet<_> = deps_by_feature.entry(feature).or_default();
                for dep in schema.dependencies().filter_map(|ty| {
                    match CargoFeature::from_name(ty.into_schema().ok()?.resource()?) {
                        CargoFeature::Named(name) => Some(CargoFeature::Named(name)),
                        CargoFeature::Default => None,
                    }
                }) {
                    entry.insert(dep);
                }
            }

            // For each operation with an explicitly declared resource name,
            // use the resource name as the feature name, and enable features for
            // all the types that are reachable from the operation.
            for op in self.graph.operations() {
                let feature = match op.resource().map(CargoFeature::from_name) {
                    Some(CargoFeature::Named(name)) => CargoFeature::Named(name),
                    _ => continue,
                };
                let entry = deps_by_feature.entry(feature).or_default();
                for dep in op.dependencies().filter_map(|ty| {
                    match CargoFeature::from_name(ty.into_schema().ok()?.resource()?) {
                        CargoFeature::Named(name) => Some(CargoFeature::Named(name)),
                        CargoFeature::Default => None,
                    }
                }) {
                    entry.insert(dep);
                }
            }

            // Build the `features` section of the manifest.
            let mut features: BTreeMap<_, _> = deps_by_feature
                .iter()
                .map(|(feature, deps)| {
                    (
                        feature.display().to_string(),
                        FeatureDependencies(
                            deps.iter()
                                .map(|dep| dep.display().to_string())
                                .collect_vec(),
                        ),
                    )
                })
                .collect();
            if features.is_empty() {
                BTreeMap::new()
            } else {
                // `default` enables all other features.
                features.insert(
                    "default".to_owned(),
                    FeatureDependencies(
                        deps_by_feature
                            .keys()
                            .map(|feature| feature.display().to_string())
                            .collect_vec(),
                    ),
                );
                features
            }
        };

        self.manifest.clone().apply(CargoManifestDiff {
            // Ploidy generates Rust 2024-compatible code.
            edition: Some(RustEdition::E2024),
            dependencies: Some(BTreeMap::from_iter([
                // `ploidy-util` is our only runtime dependency.
                ("ploidy-util".to_owned(), Dependency::Simple(PLOIDY_VERSION)),
            ])),
            features: Some(features),
            ..Default::default()
        })
    }
}

impl Code for CodegenCargoManifest<'_> {
    fn path(&self) -> &str {
        "Cargo.toml"
    }

    fn into_string(self) -> miette::Result<String> {
        Ok(self.to_manifest().to_string())
    }
}

/// A `Cargo.toml` manifest.
#[derive(Clone, Debug)]
pub struct CargoManifest(DocumentMut);

impl CargoManifest {
    /// Creates a Cargo manifest with the given package `name` and `version`.
    pub fn new(name: &str, version: Version) -> Self {
        let package = Table::from_iter([
            ("name", value(name)),
            ("version", value(version.to_string())),
            ("edition", value(RustEdition::E2024)),
        ]);
        let manifest = Table::from_iter([("package", package)]);
        Self(manifest.into())
    }

    /// Reads and parses an existing Cargo manifest from disk.
    pub fn from_disk(path: &Path) -> Result<Self, CargoManifestError> {
        let contents = std::fs::read_to_string(path)?;
        Self::parse(&contents)
    }

    /// Parses a Cargo manifest from a TOML string.
    pub fn parse(s: &str) -> Result<Self, CargoManifestError> {
        Ok(Self(s.parse().map_err(
            |source: toml_edit::TomlError| {
                let span = source.span().map(SourceSpan::from);
                SpannedError {
                    source: Box::new(source),
                    code: s.to_owned(),
                    span,
                }
            },
        )?))
    }

    /// Returns a view of the `package` section, or `None` if this is
    /// a workspace or malformed manifest.
    #[inline]
    pub fn package(&self) -> Option<Package<'_>> {
        let package = self.0.get("package")?.as_table_like()?;
        let name = package.get("name")?;
        let version = package.get("version")?;
        Some(Package {
            name: SpannedValue::new(name.as_str()?, &self.0, name.span()),
            version: SpannedValue::new(version.as_str()?, &self.0, version.span()),
            metadata: package
                .get("metadata")
                .and_then(|meta| Some((meta.as_table_like()?, meta.span())))
                .map(|(meta, range)| SpannedValue::new(meta, &self.0, range)),
        })
    }

    /// Returns the `features` table.
    pub fn features(&self) -> BTreeMap<&str, Vec<&str>> {
        self.0
            .get("features")
            .and_then(|features| features.as_table_like())
            .into_iter()
            .flat_map(|features| features.iter())
            .map(|(name, item)| {
                let deps = item
                    .as_array()
                    .into_iter()
                    .flat_map(|deps| deps.iter())
                    .filter_map(|dep| dep.as_str())
                    .collect_vec();
                (name, deps)
            })
            .collect()
    }

    /// Applies a diff of changes to the manifest.
    pub fn apply(mut self, diff: CargoManifestDiff) -> Self {
        let package = &mut self.0["package"];
        if let Some(name) = diff.name {
            package["name"] = value(name);
        }
        if let Some(version) = diff.version {
            package["version"] = value(version.to_string());
        }
        if let Some(edition) = diff.edition {
            package["edition"] = value(edition);
        }
        if let Some(deps) = diff.dependencies.filter(|f| !f.is_empty()) {
            let table = self.0["dependencies"].or_insert(Table::new().into());
            for (name, dep) in deps {
                dep.merge_into(&mut table[&name]);
            }
        }
        if let Some(features) = diff.features.filter(|f| !f.is_empty()) {
            let table = self.0["features"].or_insert(Table::new().into());
            for (name, feature) in features {
                feature.merge_into(&mut table[&name]);
            }
        }
        self
    }
}

impl Display for CargoManifest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A view of the `package` section of a Cargo manifest.
#[derive(Clone, Copy)]
pub struct Package<'a> {
    name: SpannedValue<'a, &'a str>,
    version: SpannedValue<'a, &'a str>,
    metadata: Option<SpannedValue<'a, &'a dyn TableLike>>,
}

impl<'a> Package<'a> {
    /// Returns the package name.
    pub fn name(&self) -> &'a str {
        self.name.value
    }

    /// Parses and returns the package version.
    pub fn version(&self) -> Result<Version, SpannedError<PackageError>> {
        Version::parse(self.version.value).map_err(|err| SpannedError {
            source: Box::new(PackageError::from(err)),
            code: self.version.source.to_string(),
            span: self.version.span,
        })
    }

    /// Deserializes `package.metadata.ploidy` into a [`CodegenConfig`].
    /// Returns `Ok(None)` if the section is absent, or `Err` if
    /// it's present but malformed.
    pub fn config(&self) -> Result<Option<CodegenConfig>, SpannedError<PackageError>> {
        let meta = match self.metadata {
            Some(meta) => meta,
            None => return Ok(None),
        };
        let table: Table = match meta.value.get("ploidy").and_then(|v| v.as_table_like()) {
            Some(table) => table.iter().collect(),
            None => return Ok(None),
        };
        let value: toml_edit::Value = table.into_inline_table().into();
        let config =
            CodegenConfig::deserialize(value.into_deserializer()).map_err(|err| SpannedError {
                source: Box::new(PackageError::from(err)),
                code: meta.source.to_string(),
                span: meta.span,
            })?;
        Ok(Some(config))
    }
}

impl Debug for Package<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Package")
            .field("name", &self.name)
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

/// A TOML value with source location information for diagnostics.
#[derive(Clone, Copy, Debug)]
struct SpannedValue<'a, T> {
    source: &'a DocumentMut,
    value: T,
    span: Option<SourceSpan>,
}

impl<'a, T> SpannedValue<'a, T> {
    fn new(value: T, source: &'a DocumentMut, range: Option<Range<usize>>) -> Self {
        Self {
            source,
            value,
            span: range.map(SourceSpan::from),
        }
    }
}

/// An error with source location information for diagnostics.
#[derive(Debug, miette::Diagnostic)]
pub struct SpannedError<E: StdError + Send + Sync + 'static> {
    source: Box<E>,
    #[source_code]
    code: String,
    #[label]
    span: Option<SourceSpan>,
}

impl<E: StdError + Send + Sync + 'static> Display for SpannedError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.source, f)
    }
}

impl<E: StdError + Send + Sync + 'static> StdError for SpannedError<E> {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        // Equivalent to the generated implementation for
        // `#[error(transparent)]`.
        self.source.source()
    }
}

/// The Rust edition that a package is compiled with.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RustEdition {
    E2021,
    #[default]
    E2024,
}

impl From<RustEdition> for toml_edit::Value {
    fn from(edition: RustEdition) -> Self {
        toml_edit::Value::from(match edition {
            RustEdition::E2021 => "2021",
            RustEdition::E2024 => "2024",
        })
    }
}

/// A diff of changes to apply to a [`CargoManifest`].
#[derive(Clone, Debug, Default)]
pub struct CargoManifestDiff {
    pub name: Option<String>,
    pub version: Option<Version>,
    pub edition: Option<RustEdition>,
    pub dependencies: Option<BTreeMap<String, Dependency>>,
    pub features: Option<BTreeMap<String, FeatureDependencies>>,
}

/// An entry in the `dependencies` section of a Cargo manifest.
#[derive(Clone, Debug)]
pub enum Dependency {
    Simple(Version),
    Detailed(DependencyDetail),
}

impl Dependency {
    /// Merges this dependency into an existing manifest entry. If the entry is
    /// already a table, only the specified fields are updated; if it's
    /// absent or a simple version string, it's replaced.
    fn merge_into(self, entry: &mut toml_edit::Item) {
        match self {
            Dependency::Simple(version) => {
                if let Some(table) = entry.as_table_like_mut() {
                    table.insert("version", value(version.to_string()));
                } else {
                    *entry = value(version.to_string());
                }
            }
            Dependency::Detailed(detail) => {
                let table = match entry.as_table_like_mut() {
                    Some(table) => table,
                    None => {
                        *entry = InlineTable::new().into();
                        entry.as_table_like_mut().unwrap()
                    }
                };
                table.insert("version", value(detail.version.to_string()));
                if let Some(path) = detail.path {
                    table.insert("path", value(path));
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct DependencyDetail {
    pub version: Version,
    pub path: Option<String>,
}

/// A set of feature dependencies to merge into a `[features]` entry.
#[derive(Clone, Debug)]
pub struct FeatureDependencies(Vec<String>);

impl FeatureDependencies {
    /// Merges these feature dependencies into an existing manifest entry.
    /// If the entry is already an array, all its existing dependencies
    /// are preserved, and only new ones are added; if it's absent,
    /// the array is created.
    fn merge_into(self, entry: &mut toml_edit::Item) {
        match entry.as_array_mut() {
            Some(array) => {
                let existing: BTreeSet<_> = array.iter().filter_map(|dep| dep.as_str()).collect();
                let new = self
                    .0
                    .into_iter()
                    .filter(|dep| !existing.contains(dep.as_str()))
                    .collect_vec();
                array.extend(new);
            }
            None => {
                *entry = Array::from_iter(self.0).into();
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CargoManifestError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Parse(#[from] SpannedError<toml_edit::TomlError>),
}

#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error(transparent)]
    Deserialize(#[from] toml_edit::de::Error),

    #[error(transparent)]
    Semver(#[from] semver::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        arena::Arena,
        ir::{RawGraph, Spec},
        parse::Document,
    };

    use crate::{config::DateTimeFormat, tests::assert_matches};

    fn default_manifest() -> CargoManifest {
        CargoManifest::new("test-client", Version::new(0, 1, 0))
    }

    // MARK: Manifest and TOML types

    #[test]
    fn test_new_manifest_has_package_name_version_and_edition() {
        assert_eq!(
            CargoManifest::new("my-crate", Version::new(1, 0, 0)).to_string(),
            indoc::indoc! {r#"
                [package]
                name = "my-crate"
                version = "1.0.0"
                edition = "2024"
            "#},
        );
    }

    #[test]
    fn test_package_returns_none_for_workspace() {
        let manifest = CargoManifest::parse(indoc::indoc! {r#"
            [workspace]
            members = ["a"]
        "#})
        .unwrap();
        assert!(manifest.package().is_none());
    }

    #[test]
    fn test_apply_sets_name() {
        let manifest = CargoManifest::new("old", Version::new(1, 0, 0)).apply(CargoManifestDiff {
            name: Some("new".to_owned()),
            ..Default::default()
        });
        assert_eq!(manifest.package().unwrap().name.value, "new");
    }

    #[test]
    fn test_apply_sets_version() {
        let manifest = CargoManifest::new("pkg", Version::new(1, 0, 0)).apply(CargoManifestDiff {
            version: Some(Version::new(2, 0, 0)),
            ..Default::default()
        });
        assert_eq!(manifest.package().unwrap().version.value, "2.0.0");
    }

    #[test]
    fn test_apply_sets_edition() {
        let manifest = CargoManifest::new("pkg", Version::new(1, 0, 0)).apply(CargoManifestDiff {
            edition: Some(RustEdition::E2021),
            ..Default::default()
        });
        assert_eq!(
            manifest.to_string(),
            indoc::indoc! {r#"
                [package]
                name = "pkg"
                version = "1.0.0"
                edition = "2021"
            "#},
        );
    }

    #[test]
    fn test_apply_sets_simple_dependency() {
        let mut deps = BTreeMap::new();
        deps.insert(
            "serde".to_owned(),
            Dependency::Simple(Version::new(1, 0, 0)),
        );
        let manifest = CargoManifest::new("pkg", Version::new(1, 0, 0)).apply(CargoManifestDiff {
            dependencies: Some(deps),
            ..Default::default()
        });
        assert_eq!(
            manifest.to_string(),
            indoc::indoc! {r#"
                [package]
                name = "pkg"
                version = "1.0.0"
                edition = "2024"

                [dependencies]
                serde = "1.0.0"
            "#},
        );
    }

    #[test]
    fn test_apply_sets_detailed_dependency() {
        let mut deps = BTreeMap::new();
        deps.insert(
            "ploidy-util".to_owned(),
            Dependency::Detailed(DependencyDetail {
                version: Version::new(0, 10, 0),
                path: Some("../ploidy-util".to_owned()),
            }),
        );
        let manifest = CargoManifest::new("pkg", Version::new(1, 0, 0)).apply(CargoManifestDiff {
            dependencies: Some(deps),
            ..Default::default()
        });
        assert_eq!(
            manifest.to_string(),
            indoc::indoc! {r#"
                [package]
                name = "pkg"
                version = "1.0.0"
                edition = "2024"

                [dependencies]
                ploidy-util = { version = "0.10.0", path = "../ploidy-util" }
            "#},
        );
    }

    #[test]
    fn test_apply_preserves_existing_dependencies() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
        "})
        .unwrap();

        let manifest = default_manifest().apply(CargoManifestDiff {
            dependencies: Some({
                let mut deps = BTreeMap::new();
                deps.insert(
                    "serde".to_owned(),
                    Dependency::Simple(Version::new(1, 0, 0)),
                );
                deps
            }),
            ..Default::default()
        });

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());
        let manifest = CodegenCargoManifest::new(&graph, &manifest).to_manifest();

        assert_eq!(
            manifest.to_string(),
            indoc::formatdoc! {r#"
                [package]
                name = "test-client"
                version = "0.1.0"
                edition = "2024"

                [dependencies]
                serde = "1.0.0"
                ploidy-util = "{PLOIDY_VERSION}"
            "#},
        );
    }

    #[test]
    fn test_apply_sets_features() {
        let mut features = BTreeMap::new();
        features.insert(
            "default".to_owned(),
            FeatureDependencies(vec!["customer".to_owned()]),
        );
        features.insert("customer".to_owned(), FeatureDependencies(vec![]));
        let manifest = CargoManifest::new("pkg", Version::new(1, 0, 0)).apply(CargoManifestDiff {
            features: Some(features),
            ..Default::default()
        });
        let f = manifest.features();
        assert_eq!(f["default"], vec!["customer"]);
        assert_eq!(f["customer"], Vec::<String>::new());
    }

    #[test]
    fn test_apply_preserves_untouched_fields() {
        let manifest = CargoManifest::parse(indoc::indoc! {r#"
            [package]
            name = "pkg"
            version = "1.0.0"
            edition = "2021"

            [profile.release]
            lto = true
        "#})
        .unwrap()
        .apply(CargoManifestDiff {
            edition: Some(RustEdition::E2024),
            ..Default::default()
        });
        assert_eq!(
            manifest.to_string(),
            indoc::indoc! {r#"
                [package]
                name = "pkg"
                version = "1.0.0"
                edition = "2024"

                [profile.release]
                lto = true
            "#},
        );
    }

    #[test]
    fn test_config_returns_none_when_absent() {
        let manifest = CargoManifest::new("pkg", Version::new(1, 0, 0));
        let pkg = manifest.package().unwrap();
        assert_matches!(pkg.config(), Ok(None));
    }

    #[test]
    fn test_config_deserializes_codegen_config() {
        let manifest = CargoManifest::parse(indoc::indoc! {r#"
            [package]
            name = "pkg"
            version = "1.0.0"
            edition = "2024"

            [package.metadata.ploidy]
            date-time-format = "unix-seconds"
        "#})
        .unwrap();
        let pkg = manifest.package().unwrap();
        let config = pkg.config().unwrap().unwrap();
        assert_eq!(config.date_time_format, DateTimeFormat::UnixSeconds);
    }

    // MARK: Feature collection

    #[test]
    fn test_schema_with_x_resource_id_creates_feature() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        let features = manifest.features();
        let keys = features.keys().copied().collect_vec();
        assert_matches!(&*keys, ["customer", "default"]);
    }

    #[test]
    fn test_operation_with_x_resource_name_creates_feature() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /pets:
                get:
                  operationId: listPets
                  x-resource-name: pets
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        let features = manifest.features();
        let keys = features.keys().copied().collect_vec();
        assert_matches!(&*keys, ["default", "pets"]);
    }

    #[test]
    fn test_unnamed_schema_creates_no_features() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Simple:
                  type: object
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        let features = manifest.features();
        let keys = features.keys().copied().collect_vec();
        assert_matches!(&*keys, []);
    }

    // MARK: Schema feature dependencies

    #[test]
    fn test_schema_dependency_creates_feature_dependency() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    billing:
                      $ref: '#/components/schemas/BillingInfo'
                BillingInfo:
                  type: object
                  x-resourceId: billing
                  properties:
                    card:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // `Customer` depends on `BillingInfo`, so the `customer` feature
        // should depend on `billing`.
        let features = manifest.features();
        assert_eq!(features["customer"], ["billing"]);
    }

    #[test]
    fn test_transitive_schema_dependency_creates_feature_dependency() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Order:
                  type: object
                  x-resourceId: orders
                  properties:
                    customer:
                      $ref: '#/components/schemas/Customer'
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    billing:
                      $ref: '#/components/schemas/BillingInfo'
                BillingInfo:
                  type: object
                  x-resourceId: billing
                  properties:
                    card:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // `Order` → `Customer` → `BillingInfo`, so `order` should
        // depend on both `customer` and `billing`.
        let features = manifest.features();
        assert_eq!(features["orders"], ["billing", "customer"]);
    }

    #[test]
    fn test_unnamed_dependency_does_not_create_feature_dependency() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    address:
                      $ref: '#/components/schemas/Address'
                Address:
                  type: object
                  properties:
                    street:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // `Customer` depends on `Address`, which doesn't have a resource.
        // The `customer` feature should _not_ depend on `default`;
        // that's handled via `cfg` attributes instead.
        let features = manifest.features();
        assert_matches!(&*features["customer"], &[]);
    }

    #[test]
    fn test_feature_does_not_depend_on_itself() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Node:
                  type: object
                  x-resourceId: nodes
                  properties:
                    children:
                      type: array
                      items:
                        $ref: '#/components/schemas/Node'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // Self-referential schemas should not create self-dependencies.
        let features = manifest.features();
        assert_matches!(&*features["nodes"], []);
    }

    // MARK: Operation feature dependencies

    #[test]
    fn test_operation_type_dependency_creates_feature_dependency() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /orders:
                get:
                  operationId: listOrders
                  x-resource-name: orders
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            type: array
                            items:
                              $ref: '#/components/schemas/Order'
            components:
              schemas:
                Order:
                  type: object
                  properties:
                    customer:
                      $ref: '#/components/schemas/Customer'
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // `listOrders` returns `Order`, which references `Customer`, so
        // `orders` should depend on `customer`.
        let features = manifest.features();
        assert_eq!(features["orders"], ["customer"]);
    }

    #[test]
    fn test_operation_with_unnamed_type_dependency_does_not_create_full_dependency() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /customers:
                get:
                  operationId: listCustomers
                  x-resource-name: customer
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            type: array
                            items:
                              $ref: '#/components/schemas/Customer'
            components:
              schemas:
                Customer:
                  type: object
                  properties:
                    address:
                      $ref: '#/components/schemas/Address'
                Address:
                  type: object
                  properties:
                    street:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // `listOrders` returns `Customer`, which references `Address`, but
        // `customer` should _not_ depend on `default`.
        let features = manifest.features();
        assert_matches!(&*features["customer"], []);
    }

    // MARK: Diamond dependencies

    #[test]
    fn test_diamond_dependency_deduplicates_feature() {
        // A -> B, A -> C, B -> D, C -> D. All have resources.
        // A's feature should depend on B, C, and D; D should appear once.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                A:
                  type: object
                  x-resourceId: a
                  properties:
                    b:
                      $ref: '#/components/schemas/B'
                    c:
                      $ref: '#/components/schemas/C'
                B:
                  type: object
                  x-resourceId: b
                  properties:
                    d:
                      $ref: '#/components/schemas/D'
                C:
                  type: object
                  x-resourceId: c
                  properties:
                    d:
                      $ref: '#/components/schemas/D'
                D:
                  type: object
                  x-resourceId: d
                  properties:
                    value:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        let features = manifest.features();

        // `a` depends directly on `b`, `c`;
        // transitively on `d` though `b` and `c`.
        assert_eq!(features["a"], ["b", "c", "d"]);

        // `b` and `c` each depend on `d`.
        assert_eq!(features["b"], ["d"]);
        assert_eq!(features["c"], ["d"]);

        // `d` has no dependencies.
        assert_matches!(&*features["d"], []);
    }

    // MARK: Cycles with mixed resources

    #[test]
    fn test_cycle_with_mixed_resources_does_not_create_feature_dependency() {
        // Type A (resource `a`) -> Type B (no resource) -> Type C (resource `c`) -> Type A.
        // Since B doesn't have a resource, we don't create a dependency on it;
        // that's handled via `#[cfg(...)]` attributes.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                A:
                  type: object
                  x-resourceId: a
                  properties:
                    b:
                      $ref: '#/components/schemas/B'
                B:
                  type: object
                  properties:
                    c:
                      $ref: '#/components/schemas/C'
                C:
                  type: object
                  x-resourceId: c
                  properties:
                    a:
                      $ref: '#/components/schemas/A'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        let features = manifest.features();

        // A depends on B (unnamed) and C. Since B is unnamed,
        // A only depends on C.
        assert_eq!(features["a"], ["c"]);

        // C depends on A (which depends on B, unnamed). C only depends on A.
        assert_eq!(features["c"], ["a"]);

        // `default` should include both named features.
        assert_eq!(features["default"], ["a", "c"]);
    }

    #[test]
    fn test_cycle_with_all_named_resources_creates_mutual_dependencies() {
        // Type A (resource `a`) -> Type B (resource `b`) -> Type C (resource `c`) -> Type A.
        // Each feature should depend on the others in the cycle.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                A:
                  type: object
                  x-resourceId: a
                  properties:
                    b:
                      $ref: '#/components/schemas/B'
                B:
                  type: object
                  x-resourceId: b
                  properties:
                    c:
                      $ref: '#/components/schemas/C'
                C:
                  type: object
                  x-resourceId: c
                  properties:
                    a:
                      $ref: '#/components/schemas/A'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        let features = manifest.features();

        // A transitively depends on B and C.
        assert_eq!(features["a"], ["b", "c"]);

        // B transitively depends on A and C.
        assert_eq!(features["b"], ["a", "c"]);

        // C transitively depends on A and B.
        assert_eq!(features["c"], ["a", "b"]);

        // `default` should include all three.
        assert_eq!(features["default"], ["a", "b", "c"]);
    }

    // MARK: Default feature

    #[test]
    fn test_default_feature_includes_all_other_features() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /pets:
                get:
                  operationId: listPets
                  x-resource-name: pets
                  responses:
                    '200':
                      description: OK
            components:
              schemas:
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    id:
                      type: string
                Order:
                  type: object
                  x-resourceId: orders
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // The `default` feature should include all other features,
        // but not itself.
        let features = manifest.features();
        assert_eq!(features["default"], ["customer", "orders", "pets"]);
    }

    #[test]
    fn test_default_feature_includes_all_named_features() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // The `default` feature should include all named features.
        let features = manifest.features();
        assert_eq!(features["default"], ["customer"]);
    }
}
