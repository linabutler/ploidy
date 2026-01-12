use std::{io::ErrorKind as IoErrorKind, path::PathBuf};

use cargo_toml::{Manifest, Package};
use clap::{
    CommandFactory, FromArgMatches,
    error::{Error as ClapError, ErrorKind as ClapErrorKind, Result as ClapResult},
};
use ploidy_codegen_rust::CargoMetadata;
use semver::Version;

const DEFAULT_VERSION: Version = Version::new(0, 1, 0);

#[derive(Debug)]
pub struct Main {
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
            CommandArgs::Codegen(args) => {
                Command::Codegen(args.into_command().map_err(|err| err.format(&mut cmd))?)
            }
        };
        Ok(Main { command })
    }
}

#[derive(Debug)]
pub enum Command {
    Codegen(CodegenCommand),
}

#[derive(Debug)]
pub struct CodegenCommand {
    pub input: PathBuf,
    pub output: PathBuf,
    pub language: CodegenCommandLanguage,
}

#[derive(Debug)]
pub enum CodegenCommandLanguage {
    Rust(RustCodegenCommand),
}

#[derive(Debug)]
pub struct RustCodegenCommand {
    pub manifest: Manifest<CargoMetadata>,
    pub check: bool,
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
    Codegen(CodegenCommandArgs),
}

#[derive(Debug, clap::Args)]
struct CodegenCommandArgs {
    /// The path to the OpenAPI document (`.yaml` or `.json`).
    input: PathBuf,

    /// The output directory for the generated files.
    output: PathBuf,

    #[command(subcommand)]
    language: CodegenCommandLanguageArgs,
}

impl CodegenCommandArgs {
    fn into_command(self) -> ClapResult<CodegenCommand> {
        let language = match self.language {
            CodegenCommandLanguageArgs::Rust(args) => {
                let path = self.output.join("Cargo.toml");
                match Manifest::<CargoMetadata>::from_path_with_metadata(&path) {
                    Ok(mut manifest) => {
                        let package = manifest.package.as_mut().ok_or_else(|| {
                            ClapError::raw(
                                ClapErrorKind::ValueValidation,
                                format!(
                                    "`{}` looks like a Cargo workspace; \
                                    Ploidy can only generate packages",
                                    self.output.display()
                                ),
                            )
                        })?;
                        // Update the generated package name and version,
                        // if specified on the command line.
                        package.name = args.name.unwrap_or_else(|| package.name().to_owned());
                        if let Some(bump) = args.version {
                            let base = package.version().parse().map_err(|err| {
                                ClapError::raw(
                                    ClapErrorKind::ValueValidation,
                                    format!(
                                        "manifest `{}` contains invalid package version `{}`: {err}",
                                        path.display(),
                                        package.version()
                                    ),
                                )
                            })?;
                            package.version.set(bump_version(&base, bump).to_string());
                        }
                        CodegenCommandLanguage::Rust(RustCodegenCommand {
                            manifest,
                            check: args.check,
                        })
                    }
                    Err(cargo_toml::Error::Io(err)) if err.kind() == IoErrorKind::NotFound => {
                        let name = args
                            .name
                            .or_else(|| {
                                let output = std::path::absolute(&self.output).ok()?;
                                let dir_name = output.file_name()?;
                                Some(dir_name.to_string_lossy().into_owned())
                            })
                            .ok_or_else(|| {
                                ClapError::raw(
                                    ClapErrorKind::ValueValidation,
                                    "couldn't infer generated package name; \
                                        please specify one with `--name`"
                                        .to_string(),
                                )
                            })?;
                        let manifest = Manifest {
                            package: Some(Package::new(name, DEFAULT_VERSION.to_string())),
                            ..Default::default()
                        };
                        CodegenCommandLanguage::Rust(RustCodegenCommand {
                            manifest,
                            check: args.check,
                        })
                    }
                    Err(err) => {
                        return Err(ClapError::raw(
                            ClapErrorKind::ValueValidation,
                            format!("couldn't parse manifest `{}`: {err}", path.display()),
                        ));
                    }
                }
            }
        };
        Ok(CodegenCommand {
            input: self.input,
            output: self.output,
            language,
        })
    }
}

#[derive(Debug, clap::Subcommand)]
enum CodegenCommandLanguageArgs {
    /// Generate a Rust package.
    Rust(RustCodegenCommandArgs),
}

#[derive(Debug, Default, clap::Args)]
#[command(next_help_heading = "Generated package options")]
struct RustCodegenCommandArgs {
    /// Override the generated package name. Defaults to the existing package name,
    /// or the output directory name if the package doesn't exist yet.
    #[arg(name = "name", long)]
    name: Option<String>,

    /// Increment the existing package version. Keeps the existing version if not set,
    /// or 0.1.0 if the package doesn't exist yet.
    #[arg(name = "version", long)]
    version: Option<VersionBump>,

    /// Run `cargo check` on the generated code.
    #[arg(short, long)]
    check: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
enum VersionBump {
    #[clap(name = "bump-major")]
    Major,
    #[clap(name = "bump-minor")]
    Minor,
    #[clap(name = "bump-patch")]
    Patch,
}

/// Increments the major, minor, or patch component of the given base version.
fn bump_version(base: &Version, bump: VersionBump) -> Version {
    match bump {
        VersionBump::Major => Version::new(base.major + 1, 0, 0),
        VersionBump::Minor => Version::new(base.major, base.minor + 1, 0),
        VersionBump::Patch => Version::new(base.major, base.minor, base.patch + 1),
    }
}
