//! Clap-based CLI definition for auto-re Stage 0 commands.
//!
//! All Stage 0 verbs are defined here using clap's derive API.
//! Read commands accept `--output json` (default: human-readable).
//! Write commands route through `LocalAutoReClient` — no direct storage access.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// auto-re — reverse-engineering project manager.
///
/// Run a subcommand to manage projects, artifacts, entities, evidence,
/// hypotheses, contradictions, verifications, operations, and events.
/// Omit the subcommand to display this help message.
#[derive(Parser)]
#[command(name = "auto-re", version, about, long_about = None)]
pub struct AutoReCli {
    /// Path to the parent directory containing the project.auto-re/ subdirectory.
    #[arg(long, global = true, default_value = ".")]
    pub project_dir: PathBuf,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Project management: create, inspect, validate, migrate, rebuild.
    Project(ProjectArgs),
    /// Artifact management: register, list, show.
    Artifact(ArtifactArgs),
    /// Entity management: register, list, show.
    Entity(EntityArgs),
    /// Evidence management: add, list.
    Evidence(EvidenceArgs),
    /// Hypothesis management: add, list, accept, reject.
    Hypothesis(HypothesisArgs),
    /// Contradiction inspection: list, show.
    Contradiction(ContradictionArgs),
    /// Verification inspection: list, show.
    Verification(VerificationArgs),
    /// Operation management: list, show, cancel.
    Operation(OperationArgs),
    /// Event stream: list.
    Events(EventsArgs),
}

// ---------------------------------------------------------------------------
// Output format
// ---------------------------------------------------------------------------

/// Output format for read commands.
#[derive(Clone, Default, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable tabular output (default).
    #[default]
    Human,
    /// Machine-readable JSON with $schema reference.
    Json,
}

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct ProjectArgs {
    #[command(subcommand)]
    pub command: ProjectCommand,
}

#[derive(Subcommand)]
pub enum ProjectCommand {
    /// Create a new project in the current (or specified) directory.
    Create {
        /// Project name (must not be empty).
        #[arg(long)]
        name: String,
    },
    /// Display project summary.
    Info {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// Run project-wide validation and print the report.
    Validate {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// Queue a project migration operation.
    Migrate,
    /// Queue a rebuild-indexes operation.
    RebuildIndexes,
    /// Check artifact integrity (scaffold — not yet implemented).
    CheckArtifacts,
}

// ---------------------------------------------------------------------------
// Artifact
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct ArtifactArgs {
    #[command(subcommand)]
    pub command: ArtifactCommand,
}

#[derive(Subcommand)]
pub enum ArtifactCommand {
    /// Register a new artifact.
    Add {
        /// Path to the source file.
        #[arg(long)]
        file: PathBuf,
        /// Artifact kind (namespaced ID, e.g. "core.binary").
        #[arg(long)]
        kind: String,
    },
    /// List artifacts in the project.
    List {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// Show a specific artifact.
    Show {
        /// Artifact ID (UUID).
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
}

// ---------------------------------------------------------------------------
// Entity
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct EntityArgs {
    #[command(subcommand)]
    pub command: EntityCommand,
}

#[derive(Subcommand)]
pub enum EntityCommand {
    /// Register a new semantic entity.
    Add {
        /// Entity kind (namespaced ID, e.g. "entity.function").
        #[arg(long)]
        kind: String,
        /// Optional human-readable display name.
        #[arg(long)]
        display_name: Option<String>,
        /// Optional stable key (JSON).
        #[arg(long)]
        stable_key: Option<String>,
    },
    /// List entities in the project.
    List {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// Show a specific entity.
    Show {
        /// Entity ID (UUID).
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
}

// ---------------------------------------------------------------------------
// Evidence
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct EvidenceArgs {
    #[command(subcommand)]
    pub command: EvidenceCommand,
}

#[derive(Subcommand)]
pub enum EvidenceCommand {
    /// Add an evidence record from a JSON file.
    Add {
        /// Path to a JSON file containing the EvidenceRecord.
        #[arg(long)]
        record: PathBuf,
    },
    /// List evidence records in the project.
    List {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
}

// ---------------------------------------------------------------------------
// Hypothesis
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct HypothesisArgs {
    #[command(subcommand)]
    pub command: HypothesisCommand,
}

#[derive(Subcommand)]
pub enum HypothesisCommand {
    /// Add a new hypothesis.
    Add {
        /// Subject entity ID (UUID).
        #[arg(long)]
        subject: String,
        /// Predicate (namespaced ID, e.g. "hypothesis.test").
        #[arg(long)]
        predicate: String,
        /// Candidate value as a JSON string (EvidenceValue).
        #[arg(long)]
        candidate: String,
        /// Confidence score (0.0–1.0).
        #[arg(long)]
        confidence: f64,
    },
    /// List hypotheses in the project.
    List {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// Accept a hypothesis.
    Accept {
        /// Hypothesis ID (UUID).
        #[arg(long)]
        id: String,
    },
    /// Reject a hypothesis.
    Reject {
        /// Hypothesis ID (UUID).
        #[arg(long)]
        id: String,
    },
}

// ---------------------------------------------------------------------------
// Contradiction
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct ContradictionArgs {
    #[command(subcommand)]
    pub command: ContradictionCommand,
}

#[derive(Subcommand)]
pub enum ContradictionCommand {
    /// List contradictions in the project.
    List {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// Show a specific contradiction.
    Show {
        /// Contradiction ID (UUID).
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
}

// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct VerificationArgs {
    #[command(subcommand)]
    pub command: VerificationCommand,
}

#[derive(Subcommand)]
pub enum VerificationCommand {
    /// List verification records in the project.
    List {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// Show a specific verification record.
    Show {
        /// Verification record ID (UUID).
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
}

// ---------------------------------------------------------------------------
// Operation
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct OperationArgs {
    #[command(subcommand)]
    pub command: OperationCommand,
}

#[derive(Subcommand)]
pub enum OperationCommand {
    /// List operations in the project.
    List {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// Show a specific operation.
    Show {
        /// Operation ID (UUID).
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// Cancel an operation.
    Cancel {
        /// Operation ID (UUID).
        #[arg(long)]
        id: String,
        /// Who requested the cancellation.
        #[arg(long, default_value = "cli-user")]
        requested_by: String,
        /// Optional reason for cancellation.
        #[arg(long)]
        reason: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct EventsArgs {
    #[command(subcommand)]
    pub command: EventsCommand,
}

#[derive(Subcommand)]
pub enum EventsCommand {
    /// List project events.
    List {
        /// Return events with sequence strictly after this value.
        #[arg(long, default_value = "0")]
        after: u64,
        /// Maximum number of events to return.
        #[arg(long, default_value = "100")]
        limit: usize,
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
}
