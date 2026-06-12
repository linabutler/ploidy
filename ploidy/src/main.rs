use itertools::Itertools;
use miette::{Context, IntoDiagnostic, Result};
use ploidy_codegen_rust::{
    CodegenCargoManifest, CodegenErrorModule, CodegenGraph, CodegenIdentUsage, CodegenLibrary,
    ResourceGroup,
};
use ploidy_core::{
    arena::Arena,
    codegen::write_to_disk,
    ir::{RawGraph, Spec},
    parse::Document,
};

mod args;
mod cmd;
mod stats;

use self::{
    cmd::{Generate, GenerateArgs, Main},
    stats::{GenerateStats, OutputStats, Timings, timed},
};

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> Result<()> {
    let Ok(main) = Main::parse().map_err(|err| err.exit());
    match main {
        Main::Generate(Generate::Rust(GenerateArgs {
            input,
            output,
            stats,
            language,
        })) => {
            let mut timings = Timings::default();

            let source = std::fs::read_to_string(&input)
                .into_diagnostic()
                .with_context(|| format!("Failed to read `{}`", input.display()))?;

            let doc = {
                let timing = timed(|| {
                    Document::from_yaml(&source)
                        .into_diagnostic()
                        .context("Failed to parse OpenAPI document")
                });
                timings.parse = timing.as_secs_f64();
                timing.into_inner()
            }?;

            let label = doc.info.label();
            if let Some(label) = label {
                match label.version {
                    Some(version) => eprintln!("OpenAPI: {} (version {version})", label.title),
                    None => eprintln!("OpenAPI: {}", label.title),
                }
            }

            let arena = Arena::new();
            let spec = {
                let timing = timed(|| Spec::from_doc(&arena, &doc).into_diagnostic());
                timings.ir = timing.as_secs_f64();
                timing.into_inner()
            }?;

            let raw = {
                let timing = timed(|| {
                    let mut raw = RawGraph::new(&arena, &spec);
                    raw.collapse_trivial_inlines();
                    raw.inline_tagged_variants();
                    raw.inline_untagged_variants();
                    raw
                });
                timings.ir += timing.as_secs_f64();
                timing.into_inner()
            };

            let config = language
                .manifest
                .package()
                .map(|p| p.config())
                .transpose()?
                .flatten();
            let graph = {
                let timing = timed(|| {
                    let graph = raw.cook();
                    match config.as_ref() {
                        Some(config) => CodegenGraph::with_config(graph, config),
                        None => CodegenGraph::new(graph),
                    }
                });
                timings.cook = timing.as_secs_f64();
                timing.into_inner()
            };

            eprintln!("Writing generated code to `{}`...", output.display());

            let schemas = graph.schemas().count();
            let counts = graph
                .operations()
                .into_grouping_map_by(|op| graph.resource_for(op))
                .fold(0, |count, _, _| count + 1);

            let written = {
                let timing = timed(|| -> Result<_> {
                    let mut written = Vec::new();

                    eprintln!("Generating `Cargo.toml`...");
                    written.push(write_to_disk(
                        &output,
                        CodegenCargoManifest::new(&graph, &language.manifest),
                    )?);

                    eprintln!("Generating `lib.rs`...");
                    written.push(write_to_disk(&output, CodegenLibrary)?);

                    eprintln!("Generating `error.rs`...");
                    written.push(write_to_disk(&output, CodegenErrorModule)?);

                    eprintln!("Generating {schemas} types...");
                    written.extend(ploidy_codegen_rust::write_types_to_disk(&output, &graph)?);

                    eprintln!(
                        "Generating {} client methods across {} resources...",
                        counts.values().copied().sum::<usize>(),
                        counts.len(),
                    );
                    written.extend(ploidy_codegen_rust::write_client_to_disk(&output, &graph)?);

                    Ok(written)
                });
                timings.codegen = timing.as_secs_f64();
                timing.into_inner()
            }?;

            eprintln!("Generation complete");

            if stats {
                let stats = GenerateStats {
                    spec: label,
                    schemas,
                    operations: counts
                        .iter()
                        .map(|(&resource, &count)| {
                            let key = match resource {
                                ResourceGroup::Named(name) => {
                                    CodegenIdentUsage::Module(name).display().to_string()
                                }
                                ResourceGroup::Default => "default".to_owned(),
                            };
                            (key, count)
                        })
                        .collect(),
                    timings,
                    output: OutputStats {
                        files: written.len(),
                        size: written.iter().map(|file| file.size).sum(),
                    },
                };
                println!("{}", serde_json::to_string(&stats).into_diagnostic()?);
            }

            if language.check {
                eprintln!("Running `cargo check`...");
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
