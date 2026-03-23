use std::{
    io::ErrorKind as IoErrorKind,
    path::{Path, PathBuf},
};

use clap::{
    CommandFactory, FromArgMatches,
    error::{Error as ClapError, ErrorKind as ClapErrorKind, Result as ClapResult},
};
use ploidy_codegen_rust::{CargoManifest, CargoManifestDiff, CargoManifestError};
use semver::Version;

use super::args::{RawGenerate, RawGenerateRustArgs, RawMain, VersionBump};

const DEFAULT_VERSION: Version = Version::new(0, 1, 0);

#[derive(Debug)]
pub enum Main {
    Generate(Generate),
}

impl Main {
    pub fn parse() -> ClapResult<Main> {
        let mut cmd = RawMain::command();
        let mut matches = cmd
            .try_get_matches_from_mut(std::env::args_os())
            .map_err(|err| err.format(&mut cmd))?;
        let main =
            RawMain::from_arg_matches_mut(&mut matches).map_err(|err| err.format(&mut cmd))?;
        Ok(match main {
            RawMain::Generate(args) => {
                let args = Generate::try_new(args).map_err(|err| err.format(&mut cmd))?;
                Self::Generate(args)
            }
        })
    }
}

#[derive(Debug)]
pub enum Generate {
    Rust(GenerateArgs<GenerateRustArgs>),
}

impl Generate {
    pub fn try_new(args: RawGenerate) -> ClapResult<Self> {
        match args {
            RawGenerate::Rust(args) => {
                let input = args.input;
                let output = match args.output {
                    Some(output) => output,
                    None => input
                        .file_stem()
                        .ok_or_else(|| {
                            ClapError::raw(
                                ClapErrorKind::ValueValidation,
                                format!(
                                    "couldn't infer output directory from `{}`; \
                                        please specify one with `--output`",
                                    input.display()
                                ),
                            )
                        })?
                        .into(),
                };
                let language = GenerateRustArgs::try_new(&output, args.language)?;
                Ok(Self::Rust(GenerateArgs {
                    input,
                    output,
                    language,
                }))
            }
        }
    }
}

#[derive(Debug)]
pub struct GenerateArgs<T> {
    pub input: PathBuf,
    pub output: PathBuf,
    pub language: T,
}

#[derive(Debug)]
pub struct GenerateRustArgs {
    pub manifest: CargoManifest,
    pub check: bool,
}

impl GenerateRustArgs {
    pub fn try_new(output: &Path, args: RawGenerateRustArgs) -> ClapResult<Self> {
        let path = output.join("Cargo.toml");
        match CargoManifest::from_disk(&path) {
            Ok(manifest) => {
                let package = manifest.package().ok_or_else(|| {
                    ClapError::raw(
                        ClapErrorKind::ValueValidation,
                        format!(
                            "`{}` looks like a Cargo workspace; \
                                Ploidy can only generate packages",
                            output.display()
                        ),
                    )
                })?;
                let name = package.name();
                let version = package.version().map_err(|err| {
                    ClapError::raw(
                        ClapErrorKind::ValueValidation,
                        format!(
                            "manifest `{}` contains invalid package version: {err}",
                            path.display(),
                        ),
                    )
                })?;

                let diff = CargoManifestDiff {
                    name: Some(args.name.unwrap_or_else(|| name.to_owned())),
                    version: Some(
                        args.version
                            .map(|bump| bump_version(&version, bump))
                            .unwrap_or(version),
                    ),
                    ..Default::default()
                };
                let manifest = manifest.apply(diff);

                Ok(Self {
                    manifest,
                    check: args.check,
                })
            }
            Err(CargoManifestError::Io(err)) if err.kind() == IoErrorKind::NotFound => {
                let name = args
                    .name
                    .or_else(|| {
                        let output = std::path::absolute(output).ok()?;
                        let dir_name = output.file_name()?;
                        Some(dir_name.to_string_lossy().into_owned())
                    })
                    .ok_or_else(|| {
                        ClapError::raw(
                            ClapErrorKind::ValueValidation,
                            "couldn't infer generated package name; \
                                please specify one with `--name`"
                                .to_owned(),
                        )
                    })?;
                let version = args
                    .version
                    .map(|bump| bump_version(&DEFAULT_VERSION, bump))
                    .unwrap_or(DEFAULT_VERSION);
                let manifest = CargoManifest::new(&name, version);
                Ok(Self {
                    manifest,
                    check: args.check,
                })
            }
            Err(err) => Err(ClapError::raw(
                ClapErrorKind::ValueValidation,
                format!("couldn't parse manifest `{}`: {err}", path.display()),
            )),
        }
    }
}

/// Increments the major, minor, or patch component of the given base version.
fn bump_version(base: &Version, bump: VersionBump) -> Version {
    match bump {
        VersionBump::Major => Version::new(base.major + 1, 0, 0),
        VersionBump::Minor => Version::new(base.major, base.minor + 1, 0),
        VersionBump::Patch => Version::new(base.major, base.minor, base.patch + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    use indoc::indoc;

    use crate::args::RawGenerateArgs;

    #[test]
    fn test_generate_infers_output_from_input_stem() {
        let args = RawGenerate::Rust(RawGenerateArgs {
            input: PathBuf::from("specs/petstore.yaml"),
            output: None,
            language: RawGenerateRustArgs::default(),
        });
        let Generate::Rust(result) = Generate::try_new(args).unwrap();
        assert_eq!(result.output, PathBuf::from("petstore"));
    }

    #[test]
    fn test_generate_respects_explicit_output() {
        let args = RawGenerate::Rust(RawGenerateArgs {
            input: PathBuf::from("specs/petstore.yaml"),
            output: Some(PathBuf::from("my-output")),
            language: RawGenerateRustArgs::default(),
        });
        let Generate::Rust(result) = Generate::try_new(args).unwrap();
        assert_eq!(result.output, PathBuf::from("my-output"));
    }

    #[test]
    fn test_generate_fails_without_file_stem() {
        let args = RawGenerate::Rust(RawGenerateArgs {
            input: PathBuf::from("/"),
            output: None,
            language: RawGenerateRustArgs::default(),
        });
        let err = Generate::try_new(args).unwrap_err();
        assert_eq!(err.kind(), ClapErrorKind::ValueValidation);
    }

    #[test]
    fn test_generate_rust_creates_default_manifest_for_new_crate() {
        let dir = tempfile::tempdir().unwrap();
        let args = RawGenerateRustArgs::default();
        let result = GenerateRustArgs::try_new(dir.path(), args).unwrap();
        let package = result.manifest.package().unwrap();
        // Infers name from the temp directory name.
        assert!(!package.name().is_empty());
        assert_eq!(package.version().unwrap(), DEFAULT_VERSION);
    }

    #[test]
    fn test_generate_rust_bumps_default_version_for_new_crate() {
        let dir = tempfile::tempdir().unwrap();
        let args = RawGenerateRustArgs {
            name: Some("pkg".to_owned()),
            version: Some(VersionBump::Major),
            ..Default::default()
        };
        let result = GenerateRustArgs::try_new(dir.path(), args).unwrap();
        let package = result.manifest.package().unwrap();
        assert_eq!(package.version().unwrap(), Version::new(1, 0, 0));
    }

    #[test]
    fn test_generate_rust_respects_explicit_name_for_new_crate() {
        let dir = tempfile::tempdir().unwrap();
        let args = RawGenerateRustArgs {
            name: Some("my-crate".to_owned()),
            ..Default::default()
        };
        let result = GenerateRustArgs::try_new(dir.path(), args).unwrap();
        let package = result.manifest.package().unwrap();
        assert_eq!(package.name(), "my-crate");
    }

    #[test]
    fn test_generate_rust_reads_existing_manifest() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            indoc! {r#"
                [package]
                name = "existing-pkg"
                version = "2.0.0"
                edition = "2021"
            "#},
        )
        .unwrap();
        let args = RawGenerateRustArgs::default();
        let result = GenerateRustArgs::try_new(dir.path(), args).unwrap();
        let package = result.manifest.package().unwrap();
        assert_eq!(package.name(), "existing-pkg");
        assert_eq!(package.version().unwrap(), Version::new(2, 0, 0));
    }

    #[test]
    fn test_generate_rust_respects_name_override_for_existing_crate() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            indoc! {r#"
                [package]
                name = "old-name"
                version = "1.0.0"
                edition = "2021"
            "#},
        )
        .unwrap();
        let args = RawGenerateRustArgs {
            name: Some("new-name".to_owned()),
            ..Default::default()
        };
        let result = GenerateRustArgs::try_new(dir.path(), args).unwrap();
        let package = result.manifest.package().unwrap();
        assert_eq!(package.name(), "new-name");
    }

    #[test]
    fn test_generate_rust_bumps_version_in_existing_manifest() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            indoc! {r#"
                [package]
                name = "pkg"
                version = "1.2.3"
                edition = "2021"
            "#},
        )
        .unwrap();
        let args = RawGenerateRustArgs {
            version: Some(VersionBump::Minor),
            ..Default::default()
        };
        let result = GenerateRustArgs::try_new(dir.path(), args).unwrap();
        let package = result.manifest.package().unwrap();
        assert_eq!(package.version().unwrap(), Version::new(1, 3, 0));
    }

    #[test]
    fn test_generate_rust_rejects_workspace_manifest() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            indoc! {r#"
                [workspace]
                members = ["a"]
            "#},
        )
        .unwrap();
        let args = RawGenerateRustArgs::default();
        let err = GenerateRustArgs::try_new(dir.path(), args).unwrap_err();
        assert_eq!(err.kind(), ClapErrorKind::ValueValidation);
    }

    #[test]
    fn test_generate_rust_preserves_check_flag() {
        let dir = tempfile::tempdir().unwrap();
        let args = RawGenerateRustArgs {
            name: Some("pkg".to_owned()),
            check: true,
            ..Default::default()
        };
        let result = GenerateRustArgs::try_new(dir.path(), args).unwrap();
        assert!(result.check);
    }

    #[test]
    fn test_bump_version() {
        let base = Version::new(1, 2, 3);
        assert_eq!(
            bump_version(&base, VersionBump::Major),
            Version::new(2, 0, 0)
        );

        let base = Version::new(1, 2, 3);
        assert_eq!(
            bump_version(&base, VersionBump::Minor),
            Version::new(1, 3, 0)
        );

        let base = Version::new(1, 2, 3);
        assert_eq!(
            bump_version(&base, VersionBump::Patch),
            Version::new(1, 2, 4)
        );
    }
}
