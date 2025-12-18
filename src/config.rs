use std::{io::ErrorKind as IoErrorKind, path::PathBuf};

use clap::{
    CommandFactory, FromArgMatches,
    error::{ErrorKind as ClapErrorKind, Result as ClapResult},
};
use semver::Version;
use serde::Deserialize;

#[derive(Debug, clap::Parser)]
#[command(version, about, long_about = None)]
pub struct Main {
    /// Enable verbose logging.
    #[arg(short, long)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Command,
}

impl Main {
    pub fn parse() -> ClapResult<Main> {
        let mut cmd = Self::command();
        let mut matches = cmd
            .try_get_matches_from_mut(std::env::args_os())
            .map_err(|err| err.format(&mut cmd))?;
        let args = Self::from_arg_matches_mut(&mut matches).map_err(|err| err.format(&mut cmd))?;
        let command = match args.command {
            Command::Codegen(Codegen {
                input,
                output,
                language,
            }) => {
                let file: Option<ConfigFileSections> = {
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
                match language {
                    CodegenLanguage::Rust(rust) => {
                        let package = match file {
                            Some(ConfigFileSections {
                                rust:
                                    Some(RustConfigFileSection {
                                        package: Some(other),
                                    }),
                            }) => rust.package.merge(other),
                            _ => rust.package,
                        };
                        Command::Codegen(Codegen {
                            input,
                            output,
                            language: CodegenLanguage::Rust(CodegenRust { package, ..rust }),
                        })
                    }
                }
            }
        };
        Ok(Main {
            verbose: args.verbose,
            command,
        })
    }
}

#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Generate a client from an OpenAPI document.
    Codegen(Codegen),
}

#[derive(Debug, clap::Args)]
pub struct Codegen {
    /// The path to the OpenAPI document (`.yaml` or `.json`).
    pub input: PathBuf,

    /// The output directory for the generated files.
    pub output: PathBuf,

    #[command(subcommand)]
    pub language: CodegenLanguage,
}

#[derive(Debug, clap::Subcommand)]
pub enum CodegenLanguage {
    /// Generate a Rust package.
    Rust(CodegenRust),
}

#[derive(Debug, Default, clap::Args)]
pub struct CodegenRust {
    /// Run `cargo check` on the generated code.
    #[arg(short, long)]
    pub check: bool,

    #[command(flatten)]
    pub package: RustPackageFragment,
}

#[derive(Debug, Default, Deserialize, clap::Args)]
#[command(next_help_heading = "Generated package options")]
pub struct RustPackageFragment {
    /// The generated package name. Defaults to the output directory name
    /// if not set.
    #[arg(name = "package-name", long)]
    #[serde(default)]
    pub name: Option<String>,

    /// The generated package version. Defaults to `0.0.0` if not set.
    #[arg(name = "package-version", long)]
    #[serde(default)]
    pub version: Option<Version>,

    /// The generated package description.
    #[arg(name = "package-description", long)]
    #[serde(default)]
    pub description: Option<String>,

    /// The generated package license. Defaults to `UNLICENSED` if not set.
    #[arg(name = "package-license", long)]
    #[serde(default)]
    pub license: Option<String>,
}

impl RustPackageFragment {
    #[inline]
    pub fn merge(self, other: Self) -> Self {
        Self {
            name: self.name.or(other.name),
            version: self.version.or(other.version),
            description: self.description.or(other.description),
            license: self.license.or(other.license),
        }
    }

    #[inline]
    pub fn version_or_default(&self) -> Version {
        self.version
            .clone()
            .unwrap_or_else(|| Version::new(0, 0, 0))
    }

    #[inline]
    pub fn license_or_default(&self) -> &str {
        self.license.as_deref().unwrap_or("UNLICENSED")
    }
}

#[derive(Debug, Deserialize)]
pub struct ConfigFileSections {
    #[serde(default)]
    pub rust: Option<RustConfigFileSection>,
}

#[derive(Debug, Deserialize)]
pub struct RustConfigFileSection {
    #[serde(default)]
    pub package: Option<RustPackageFragment>,
}
