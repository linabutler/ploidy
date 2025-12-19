use std::{
    io::ErrorKind as IoErrorKind,
    path::{Path, PathBuf},
};

use clap::{
    CommandFactory, FromArgMatches,
    error::{ErrorKind as ClapErrorKind, Result as ClapResult},
};
use miette::miette;
use semver::Version;
use serde::Deserialize;

const DEFAULT_VERSION: Version = Version::new(0, 1, 0);
const DEFAULT_LICENSE: &str = "UNLICENSED";

#[derive(Debug)]
pub struct Main {
    pub verbose: bool,
    pub command: Command,
}

impl Main {
    pub fn parse() -> ClapResult<Main> {
        let mut cmd = MainArgs::command();
        let mut matches = cmd
            .try_get_matches_from_mut(std::env::args_os())
            .map_err(|err| err.format(&mut cmd))?;
        let args =
            MainArgs::from_arg_matches_mut(&mut matches).map_err(|err| err.format(&mut cmd))?;

        let command = match args.command {
            CommandArgs::Codegen(CodegenArgs {
                input,
                output,
                language,
            }) => {
                let file: Option<ConfigFile> = {
                    let path = output.join(".ploidy.toml");
                    match std::fs::read_to_string(&path) {
                        Ok(contents) => Some(toml::from_str(&contents).map_err(|err| {
                            cmd.error(
                                ClapErrorKind::ValueValidation,
                                format!("Failed to parse `{}`: {err}", path.display()),
                            )
                        })?),
                        Err(err) if err.kind() == IoErrorKind::NotFound => None,
                        Err(err) => {
                            return Err(cmd.error(
                                ClapErrorKind::Io,
                                format!("Failed to read `{}`: {err}", path.display()),
                            ));
                        }
                    }
                };

                let language = match language {
                    CodegenLanguageArgs::Rust(rust) => {
                        let config = match file {
                            Some(ConfigFile { rust: Some(file) }) => file.merge(rust.package),
                            _ => rust.package.into(),
                        };
                        CodegenLanguage::Rust(CodegenRust {
                            check: rust.check,
                            package: config,
                        })
                    }
                };

                Command::Codegen(Codegen {
                    input,
                    output,
                    language,
                })
            }
        };

        Ok(Main {
            verbose: args.verbose,
            command,
        })
    }
}

#[derive(Debug)]
pub enum Command {
    Codegen(Codegen),
}

#[derive(Debug)]
pub struct Codegen {
    pub input: PathBuf,
    pub output: PathBuf,
    pub language: CodegenLanguage,
}

#[derive(Debug)]
pub enum CodegenLanguage {
    Rust(CodegenRust),
}

#[derive(Debug)]
pub struct CodegenRust {
    pub check: bool,
    pub package: CodegenRustPackage,
}

#[derive(Debug, Default)]
pub struct CodegenRustPackage {
    pub name: Option<String>,
    pub version: Option<VersionBump>,
    pub license: Option<String>,
    pub description: Option<String>,
}

impl From<CodegenRustPackageArgs> for CodegenRustPackage {
    fn from(value: CodegenRustPackageArgs) -> Self {
        Self {
            name: value.name,
            version: value.version,
            license: value.license,
            description: value.description,
        }
    }
}

#[derive(Debug)]
pub struct RustPackageConfig {
    pub name: String,
    pub version: Version,
    pub license: String,
    pub description: Option<String>,
}

impl RustPackageConfig {
    pub fn resolve(output: &Path, config: CodegenRustPackage) -> miette::Result<Self> {
        match CargoManifest::from_existing_output(output) {
            Some(manifest) => {
                let name = config.name.unwrap_or(manifest.package.name);
                let version = match config.version {
                    Some(version) => version.bump(
                        manifest
                            .package
                            .version
                            .as_ref()
                            .unwrap_or(&DEFAULT_VERSION),
                    ),
                    None => manifest.package.version.unwrap_or(DEFAULT_VERSION),
                };
                let license = config
                    .license
                    .or(manifest.package.license)
                    .unwrap_or_else(|| DEFAULT_LICENSE.to_owned());
                let description = config.description.or(manifest.package.description);
                Ok(Self {
                    name,
                    version,
                    license,
                    description,
                })
            }
            None => {
                let name = config
                    .name
                    .or_else(|| {
                        let output = std::path::absolute(output).ok()?;
                        let dir_name = output.file_name()?;
                        Some(dir_name.to_string_lossy().into_owned())
                    })
                    .ok_or_else(|| {
                        miette!("couldn't infer generated package name; please specify one with `--name`")
                    })?;
                Ok(Self {
                    name,
                    version: DEFAULT_VERSION,
                    license: config.license.unwrap_or_else(|| DEFAULT_LICENSE.to_owned()),
                    description: config.description,
                })
            }
        }
    }
}

#[derive(Debug, clap::Parser)]
#[command(version, about, long_about = None)]
struct MainArgs {
    /// Enable verbose logging.
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: CommandArgs,
}

#[derive(Debug, clap::Subcommand)]
enum CommandArgs {
    /// Generate a client from an OpenAPI document.
    Codegen(CodegenArgs),
}

#[derive(Debug, clap::Args)]
struct CodegenArgs {
    /// The path to the OpenAPI document (`.yaml` or `.json`).
    input: PathBuf,

    /// The output directory for the generated files.
    output: PathBuf,

    #[command(subcommand)]
    language: CodegenLanguageArgs,
}

#[derive(Debug, clap::Subcommand)]
enum CodegenLanguageArgs {
    /// Generate a Rust package.
    Rust(CodegenRustArgs),
}

#[derive(Debug, clap::Args)]
struct CodegenRustArgs {
    /// Run `cargo check` on the generated code.
    #[arg(short, long)]
    check: bool,

    #[command(flatten)]
    package: CodegenRustPackageArgs,
}

#[derive(Debug, Default, clap::Args)]
#[command(next_help_heading = "Generated package options")]
pub struct CodegenRustPackageArgs {
    /// Increment the existing package version. Keeps the existing version if not set,
    /// or 0.1.0 if the package doesn't exist yet.
    #[arg(name = "version", long)]
    version: Option<VersionBump>,

    /// The generated package name. Defaults to the existing package name,
    /// or the output directory name.
    #[arg(name = "name", long)]
    name: Option<String>,

    /// The generated package license. Defaults to the existing package license,
    /// or `UNLICENSED` if not set.
    #[arg(name = "license", long)]
    license: Option<String>,

    /// The generated package description. Defaults to the existing package description.
    #[arg(name = "description", long)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    rust: Option<RustConfigFile>,
}

#[derive(Debug, Deserialize)]
struct RustConfigFile {
    #[serde(default)]
    version: Option<VersionBump>,
}

impl RustConfigFile {
    fn merge(self, args: CodegenRustPackageArgs) -> CodegenRustPackage {
        CodegenRustPackage {
            name: args.name,
            version: args.version.or(self.version),
            license: args.license,
            description: args.description,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, clap::ValueEnum)]
pub enum VersionBump {
    #[clap(name = "bump-major")]
    #[serde(rename = "bump-major")]
    Major,
    #[clap(name = "bump-minor")]
    #[serde(rename = "bump-minor")]
    Minor,
    #[clap(name = "bump-patch")]
    #[serde(rename = "bump-patch")]
    Patch,
}

impl VersionBump {
    /// Apply this version bump to the given base version.
    fn bump(self, base: &Version) -> Version {
        match self {
            Self::Major => Version::new(base.major + 1, 0, 0),
            Self::Minor => Version::new(base.major, base.minor + 1, 0),
            Self::Patch => Version::new(base.major, base.minor, base.patch + 1),
        }
    }
}

/// An existing `Cargo.toml`.
#[derive(Debug, Deserialize)]
struct CargoManifest {
    package: CargoManifestPackage,
}

impl CargoManifest {
    fn from_existing_output(output: &Path) -> Option<Self> {
        let path = output.join("Cargo.toml");
        let contents = std::fs::read_to_string(&path).ok()?;
        let manifest: CargoManifest = toml::from_str(&contents).ok()?;
        Some(manifest)
    }
}

/// The `package` section of a `Cargo.toml`.
#[derive(Debug, Deserialize)]
pub struct CargoManifestPackage {
    name: String,
    version: Option<Version>,
    license: Option<String>,
    description: Option<String>,
}
