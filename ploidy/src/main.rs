use itertools::Itertools;
use miette::{Context, IntoDiagnostic, Result};
use ploidy_codegen_rust::{CodegenCargoManifest, CodegenErrorModule, CodegenLibrary};
use ploidy_core::{codegen::write_to_disk, ir::Ir, parse::Document};

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

            let ir = Ir::from_doc(&doc).into_diagnostic()?;
            let mut raw = ir.graph();
            raw.lower_tagged_variants();

            let config = command
                .manifest
                .package
                .as_ref()
                .and_then(|p| p.metadata.as_ref()?.ploidy.as_ref());
            let graph = {
                let graph = raw.finalize();
                match config {
                    Some(config) => ploidy_codegen_rust::CodegenGraph::with_config(graph, config),
                    None => ploidy_codegen_rust::CodegenGraph::new(graph),
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
                    .current_dir(&output)
                    .status()
                    .into_diagnostic()?;

                if !status.success() {
                    miette::bail!("`cargo check` exited with status {status}");
                }
            }
        }
        Command::Codegen(CodegenCommand {
            input,
            output,
            language: CodegenCommandLanguage::TypeScript,
        }) => {
            let source = std::fs::read_to_string(&input)
                .into_diagnostic()
                .with_context(|| format!("Failed to read `{}`", input.display()))?;

            let doc = Document::from_yaml(&source)
                .into_diagnostic()
                .context("Failed to parse OpenAPI document")?;

            println!("OpenAPI: {} (version {})", doc.info.title, doc.info.version);

            let ir = Ir::from_doc(&doc).into_diagnostic()?;
            let graph = ploidy_codegen_typescript::CodegenGraph::new(ir.graph().finalize());

            println!("Writing generated code to `{}`...", output.display());
            println!("Generating {} types...", graph.schemas().count());
            ploidy_codegen_typescript::write_types_to_disk(&output, &graph)?;

            ploidy_codegen_typescript::write_client_to_disk(&output, &graph)?;

            println!("Generation complete");
        }
    }

    Ok(())
}
