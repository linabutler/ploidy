use itertools::Itertools;
use miette::{Context, IntoDiagnostic, Result};
use ploidy_codegen_rust::{CodegenCargoManifest, CodegenErrorModule, CodegenGraph, CodegenLibrary};
use ploidy_core::{
    arena::Arena,
    codegen::write_to_disk,
    ir::{RawGraph, Spec},
    parse::Document,
};

mod args;
mod cmd;

use self::cmd::{Generate, GenerateArgs, Main};

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> Result<()> {
    let Ok(main) = Main::parse().map_err(|err| err.exit());
    match main {
        Main::Generate(Generate::Rust(GenerateArgs {
            input,
            output,
            language,
        })) => {
            let source = std::fs::read_to_string(&input)
                .into_diagnostic()
                .with_context(|| format!("Failed to read `{}`", input.display()))?;

            let doc = Document::from_yaml(&source)
                .into_diagnostic()
                .context("Failed to parse OpenAPI document")?;

            if let Some(label) = doc.info.label() {
                match label.version {
                    Some(version) => println!("OpenAPI: {} (version {version})", label.title),
                    None => println!("OpenAPI: {}", label.title),
                }
            }

            let arena = Arena::new();
            let spec = Spec::from_doc(&arena, &doc).into_diagnostic()?;
            let mut raw = RawGraph::new(&arena, &spec);
            raw.inline_tagged_variants();
            raw.inline_untagged_variants();

            let config = language
                .manifest
                .package()
                .map(|p| p.config())
                .transpose()?
                .flatten();
            let graph = {
                let graph = raw.cook();
                match config.as_ref() {
                    Some(config) => CodegenGraph::with_config(graph, config),
                    None => CodegenGraph::new(graph),
                }
            };

            println!("Writing generated code to `{}`...", output.display());

            println!("Generating `Cargo.toml`...");
            write_to_disk(
                &output,
                CodegenCargoManifest::new(&graph, &language.manifest),
            )?;

            println!("Generating `lib.rs`...");
            write_to_disk(&output, CodegenLibrary)?;

            println!("Generating `error.rs`...");
            write_to_disk(&output, CodegenErrorModule)?;

            println!("Generating {} types...", graph.schemas().count());
            ploidy_codegen_rust::write_types_to_disk(&output, &graph)?;

            let counts = graph
                .operations()
                .into_grouping_map_by(|op| op.resource())
                .fold(0, |count, _, _| count + 1);
            println!(
                "Generating {} client methods across {} resources...",
                counts.values().copied().sum::<usize>(),
                counts.len(),
            );
            ploidy_codegen_rust::write_client_to_disk(&output, &graph)?;

            println!("Generation complete");

            if language.check {
                println!("Running `cargo check`...");
                let status = std::process::Command::new("cargo")
                    .arg("check")
                    .arg("--all-targets")
                    .current_dir(&output)
                    .status()
                    .into_diagnostic()?;

                if !status.success() {
                    miette::bail!("`cargo check` exited with status {status}");
                }
            }
        }
    }

    Ok(())
}
