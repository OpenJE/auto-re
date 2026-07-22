//! auto-re CLI entry point.
//!
//! This binary provides the Stage 0 and Stage 1 command-line interface for
//! managing auto-re projects, artifacts, entities, evidence, hypotheses,
//! contradictions, verifications, operations, events, reconstruction
//! campaigns, providers, work items, generated sources, and builds.
//!
//! All operations route through `LocalAutoReClient` — the CLI never
//! accesses storage directly.

mod cli;
mod handlers;

use clap::Parser;

fn main() {
    let args = cli::AutoReCli::parse();
    if let Err(e) = handlers::run(args) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
