use std::path::PathBuf;

#[derive(Debug, clap::Parser)]
#[command(version, about, long_about = None)]
pub enum RawMain {
    /// Generate code from an OpenAPI spec.
    #[command(subcommand)]
    Generate(RawGenerate),
}

#[derive(Debug, clap::Subcommand)]
pub enum RawGenerate {
    /// Generate a Rust crate.
    Rust(RawGenerateArgs<RawGenerateRustArgs>),
}

#[derive(Debug, clap::Args)]
pub struct RawGenerateArgs<T: clap::Args> {
    /// The path to the OpenAPI spec (`.yaml` or `.json`).
    pub input: PathBuf,

    /// The output directory. Defaults to a subdirectory
    /// named after the spec file.
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    #[command(flatten)]
    pub language: T,
}

#[derive(Debug, Default, clap::Args)]
#[command(next_help_heading = "Crate options")]
pub struct RawGenerateRustArgs {
    /// Set the crate name. If omitted, keeps the existing crate name,
    /// or uses the output directory name for new crates.
    #[arg(long)]
    pub name: Option<String>,

    /// Increment the crate version. If omitted, keeps the
    /// existing version, or uses 0.1.0 for new crates.
    #[arg(long)]
    pub version: Option<VersionBump>,

    /// Verify the generated crate compiles.
    #[arg(short, long)]
    pub check: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
pub enum VersionBump {
    #[clap(name = "bump-major")]
    Major,
    #[clap(name = "bump-minor")]
    Minor,
    #[clap(name = "bump-patch")]
    Patch,
}
