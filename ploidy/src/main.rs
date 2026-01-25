use itertools::Itertools;
use miette::{Context, IntoDiagnostic, Result};
use ploidy_codegen_rust::{CodegenCargoManifest, CodegenErrorModule, CodegenGraph, CodegenLibrary};
use ploidy_core::{
    codegen::write_to_disk,
    ir::{IrGraph, IrSpec},
    parse::Document,
};

mod config;

use self::config::{CodegenCommand, CodegenCommandLanguage, Command, Main};

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> Result<()> {
    let Ok(main) = Main::parse().map_err(|err| err.exit());
    match main.command {
        Command::Codegen(CodegenCommand {
            input,
            output,
            language: CodegenCommandLanguage::Rust(command),
        }) => {
            let source = std::fs::read_to_string(&input)
                .into_diagnostic()
                .with_context(|| format!("Failed to read `{}`", input.display()))?;

            let doc = Document::from_yaml(&source)
                .into_diagnostic()
                .context("Failed to parse OpenAPI document")?;

            println!("OpenAPI: {} (version {})", doc.info.title, doc.info.version);

            let spec = IrSpec::from_doc(&doc).into_diagnostic()?;

            let config = command
                .manifest
                .package
                .as_ref()
                .and_then(|p| p.metadata.as_ref()?.ploidy.as_ref());
            let graph = {
                let graph = IrGraph::new(&spec);
                match config {
                    Some(config) => CodegenGraph::with_config(graph, config),
                    None => CodegenGraph::new(graph),
                }
            };

            println!("Writing generated code to `{}`...", output.display());

            println!("Generating `Cargo.toml`...");
            write_to_disk(
                &output,
                CodegenCargoManifest::new(&graph, &command.manifest),
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

            if command.check {
                println!("Running `cargo check`...");
                let status = std::process::Command::new("cargo")
                    .arg("check")
                    .arg("--all-targets")
                    .arg("--features")
                    .arg("full")
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
