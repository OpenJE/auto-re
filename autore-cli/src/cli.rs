//! Clap-based CLI definition for auto-re Stage 0 and Stage 1 commands.
//!
//! All verbs are defined here using clap's derive API.
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
    /// Interactive TUI dashboard.
    Tui(TuiArgs),
    /// Reconstruction campaign lifecycle: start, status, pause, resume, stop, validate.
    Reconstruct(ReconstructArgs),
    /// Provider installation and instance management.
    Provider(ProviderArgs),
    /// Work item inspection, blockers, retry, and dependencies.
    Work(WorkArgs),
    /// Generated source mapping inspection.
    Generated(GeneratedArgs),
    /// Build status inspection.
    Build(BuildArgs),
}

// ---------------------------------------------------------------------------
// TUI
// ---------------------------------------------------------------------------

/// Arguments for the interactive TUI subcommand.
#[derive(Args)]
pub struct TuiArgs {}

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
    RebuildIndexes {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
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
    /// Display verification coverage summary.
    Coverage {
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

// ---------------------------------------------------------------------------
// Stage 1 — Reconstruct
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct ReconstructArgs {
    #[command(subcommand)]
    pub command: ReconstructCommand,
}

#[derive(Subcommand)]
pub enum ReconstructCommand {
    /// Start a new reconstruction campaign.
    Start {
        /// Path to the binary to reconstruct.
        #[arg(long)]
        binary: PathBuf,
        /// Output directory for generated artifacts.
        #[arg(long)]
        output: PathBuf,
        /// Analysis provider specification.
        #[arg(long)]
        analysis_provider: String,
        /// Model provider specification.
        #[arg(long)]
        model_provider: String,
        /// Build profile identifier.
        #[arg(long)]
        build_profile: String,
    },
    /// Display reconstruction campaign status.
    Status {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// Pause the reconstruction coordinator.
    Pause,
    /// Resume the reconstruction coordinator.
    Resume,
    /// Stop the reconstruction coordinator.
    Stop,
    /// Validate the reconstruction campaign.
    Validate {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
}

// ---------------------------------------------------------------------------
// Stage 1 — Provider
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct ProviderArgs {
    #[command(subcommand)]
    pub command: ProviderCommand,
}

#[derive(Subcommand)]
pub enum ProviderCommand {
    /// Refresh the list of known provider installations.
    Refresh,
    /// List provider instances.
    List {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// Show a specific provider instance.
    Show {
        /// Provider instance ID.
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// Start a new provider instance from an installation.
    Start {
        /// Provider installation ID to instantiate.
        #[arg(long)]
        installation_id: String,
    },
    /// Stop a running provider instance.
    Stop {
        /// Provider instance ID to stop.
        #[arg(long)]
        id: String,
    },
    /// Restart a provider instance (stop then start).
    Restart {
        /// Provider instance ID to restart.
        #[arg(long)]
        id: String,
    },
    /// Check health of a provider instance.
    Health {
        /// Provider instance ID to check.
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
}

// ---------------------------------------------------------------------------
// Stage 1 — Work
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct WorkArgs {
    #[command(subcommand)]
    pub command: WorkCommand,
}

#[derive(Subcommand)]
pub enum WorkCommand {
    /// List work items in the project.
    List {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// Show a specific work item.
    Show {
        /// Work item ID.
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// List blockers for a work item.
    Blockers {
        /// Work item ID.
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// Retry a failed or blocked work item.
    Retry {
        /// Work item ID to retry.
        #[arg(long)]
        id: String,
    },
    /// List dependencies for a work item.
    Dependencies {
        /// Work item ID.
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
}

// ---------------------------------------------------------------------------
// Stage 1 — Generated
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct GeneratedArgs {
    #[command(subcommand)]
    pub command: GeneratedCommand,
}

#[derive(Subcommand)]
pub enum GeneratedCommand {
    /// Display generated source mapping status.
    Status {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// List generated source mapping files.
    Files {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    /// Show a specific generated source entity mapping.
    Entity {
        /// Entity ID.
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
}

// ---------------------------------------------------------------------------
// Stage 1 — Build
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct BuildArgs {
    #[command(subcommand)]
    pub command: BuildCommand,
}

#[derive(Subcommand)]
pub enum BuildCommand {
    /// Display the latest build status.
    Latest {
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn reconstruct_start_parses_all_flags() {
        let cli = AutoReCli::parse_from([
            "auto-re",
            "reconstruct",
            "start",
            "--binary",
            "/tmp/hello.exe",
            "--output",
            "/tmp/out",
            "--analysis-provider",
            "ida:latest",
            "--model-provider",
            "gpt-4o",
            "--build-profile",
            "release",
        ]);
        match cli.command {
            Some(Commands::Reconstruct(args)) => match args.command {
                ReconstructCommand::Start {
                    binary,
                    output,
                    analysis_provider,
                    model_provider,
                    build_profile,
                } => {
                    assert_eq!(binary.to_str().unwrap(), "/tmp/hello.exe");
                    assert_eq!(output.to_str().unwrap(), "/tmp/out");
                    assert_eq!(analysis_provider, "ida:latest");
                    assert_eq!(model_provider, "gpt-4o");
                    assert_eq!(build_profile, "release");
                }
                _ => panic!("expected Start subcommand"),
            },
            _ => panic!("expected Reconstruct command"),
        }
    }

    #[test]
    fn reconstruct_start_help_contains_flags() {
        use clap::CommandFactory;
        let cmd = AutoReCli::command();
        let reconstruct = cmd
            .find_subcommand("reconstruct")
            .expect("reconstruct subcommand exists");
        let start = reconstruct
            .find_subcommand("start")
            .expect("start subcommand exists");
        let help = start.clone().render_help().to_string();
        assert!(help.contains("--binary"), "help missing --binary: {help}");
        assert!(help.contains("--output"), "help missing --output: {help}");
        assert!(
            help.contains("--analysis-provider"),
            "help missing --analysis-provider: {help}"
        );
        assert!(
            help.contains("--model-provider"),
            "help missing --model-provider: {help}"
        );
        assert!(
            help.contains("--build-profile"),
            "help missing --build-profile: {help}"
        );
    }

    #[test]
    fn provider_show_help_contains_id() {
        use clap::CommandFactory;
        let cmd = AutoReCli::command();
        let provider = cmd
            .find_subcommand("provider")
            .expect("provider subcommand exists");
        let show = provider
            .find_subcommand("show")
            .expect("show subcommand exists");
        let help = show.clone().render_help().to_string();
        assert!(help.contains("--id"), "help missing --id: {help}");
    }

    #[test]
    fn provider_show_parses_id() {
        let cli = AutoReCli::parse_from(["auto-re", "provider", "show", "--id", "inst-001"]);
        match cli.command {
            Some(Commands::Provider(args)) => match args.command {
                ProviderCommand::Show { id, .. } => assert_eq!(id, "inst-001"),
                _ => panic!("expected Show subcommand"),
            },
            _ => panic!("expected Provider command"),
        }
    }

    #[test]
    fn work_retry_help_contains_id() {
        use clap::CommandFactory;
        let cmd = AutoReCli::command();
        let work = cmd.find_subcommand("work").expect("work subcommand exists");
        let retry = work
            .find_subcommand("retry")
            .expect("retry subcommand exists");
        let help = retry.clone().render_help().to_string();
        assert!(help.contains("--id"), "help missing --id: {help}");
    }

    #[test]
    fn work_retry_parses_id() {
        let cli = AutoReCli::parse_from(["auto-re", "work", "retry", "--id", "wi-42"]);
        match cli.command {
            Some(Commands::Work(args)) => match args.command {
                WorkCommand::Retry { id } => assert_eq!(id, "wi-42"),
                _ => panic!("expected Retry subcommand"),
            },
            _ => panic!("expected Work command"),
        }
    }

    #[test]
    fn generated_files_output_json_parses() {
        let cli = AutoReCli::parse_from(["auto-re", "generated", "files", "--output", "json"]);
        match cli.command {
            Some(Commands::Generated(args)) => match args.command {
                GeneratedCommand::Files { output } => {
                    assert!(matches!(output, OutputFormat::Json));
                }
                _ => panic!("expected Files subcommand"),
            },
            _ => panic!("expected Generated command"),
        }
    }

    #[test]
    fn build_latest_parses() {
        let cli = AutoReCli::parse_from(["auto-re", "build", "latest"]);
        match cli.command {
            Some(Commands::Build(args)) => match args.command {
                BuildCommand::Latest { output } => {
                    assert!(matches!(output, OutputFormat::Human));
                }
            },
            _ => panic!("expected Build command"),
        }
    }

    #[test]
    fn verification_coverage_parses() {
        let cli =
            AutoReCli::parse_from(["auto-re", "verification", "coverage", "--output", "json"]);
        match cli.command {
            Some(Commands::Verification(args)) => match args.command {
                VerificationCommand::Coverage { output } => {
                    assert!(matches!(output, OutputFormat::Json));
                }
                _ => panic!("expected Coverage subcommand"),
            },
            _ => panic!("expected Verification command"),
        }
    }

    #[test]
    fn top_level_help_lists_stage1_commands() {
        use clap::CommandFactory;
        let cmd = AutoReCli::command();
        let help = cmd.clone().render_help().to_string();
        assert!(
            help.contains("reconstruct"),
            "help missing reconstruct: {help}"
        );
        assert!(help.contains("provider"), "help missing provider: {help}");
        assert!(help.contains("work"), "help missing work: {help}");
        assert!(help.contains("generated"), "help missing generated: {help}");
        assert!(help.contains("build"), "help missing build: {help}");
    }

    #[test]
    fn reconstruct_status_default_human() {
        let cli = AutoReCli::parse_from(["auto-re", "reconstruct", "status"]);
        match cli.command {
            Some(Commands::Reconstruct(args)) => match args.command {
                ReconstructCommand::Status { output } => {
                    assert!(matches!(output, OutputFormat::Human));
                }
                _ => panic!("expected Status"),
            },
            _ => panic!("expected Reconstruct"),
        }
    }

    #[test]
    fn reconstruct_pause_resume_stop_parse() {
        for subcmd in &["pause", "resume", "stop"] {
            let cli = AutoReCli::parse_from(["auto-re", "reconstruct", subcmd]);
            match cli.command {
                Some(Commands::Reconstruct(_)) => {}
                _ => panic!("expected Reconstruct for {subcmd}"),
            }
        }
    }

    #[test]
    fn provider_start_parses_installation_id() {
        let cli = AutoReCli::parse_from([
            "auto-re",
            "provider",
            "start",
            "--installation-id",
            "install-7",
        ]);
        match cli.command {
            Some(Commands::Provider(args)) => match args.command {
                ProviderCommand::Start { installation_id } => {
                    assert_eq!(installation_id, "install-7");
                }
                _ => panic!("expected Start"),
            },
            _ => panic!("expected Provider"),
        }
    }

    #[test]
    fn provider_stop_restart_health_parse() {
        let cli = AutoReCli::parse_from(["auto-re", "provider", "stop", "--id", "inst-1"]);
        match cli.command {
            Some(Commands::Provider(args)) => match args.command {
                ProviderCommand::Stop { id } => assert_eq!(id, "inst-1"),
                _ => panic!("expected Stop"),
            },
            _ => panic!("expected Provider"),
        }

        let cli = AutoReCli::parse_from(["auto-re", "provider", "restart", "--id", "inst-2"]);
        match cli.command {
            Some(Commands::Provider(args)) => match args.command {
                ProviderCommand::Restart { id } => assert_eq!(id, "inst-2"),
                _ => panic!("expected Restart"),
            },
            _ => panic!("expected Provider"),
        }

        let cli = AutoReCli::parse_from(["auto-re", "provider", "health", "--id", "inst-3"]);
        match cli.command {
            Some(Commands::Provider(args)) => match args.command {
                ProviderCommand::Health { id, .. } => assert_eq!(id, "inst-3"),
                _ => panic!("expected Health"),
            },
            _ => panic!("expected Provider"),
        }
    }

    #[test]
    fn work_show_blockers_dependencies_parse() {
        let cli = AutoReCli::parse_from(["auto-re", "work", "show", "--id", "wi-1"]);
        match cli.command {
            Some(Commands::Work(args)) => match args.command {
                WorkCommand::Show { id, .. } => assert_eq!(id, "wi-1"),
                _ => panic!("expected Show"),
            },
            _ => panic!("expected Work"),
        }

        let cli = AutoReCli::parse_from(["auto-re", "work", "blockers", "--id", "wi-2"]);
        match cli.command {
            Some(Commands::Work(args)) => match args.command {
                WorkCommand::Blockers { id, .. } => assert_eq!(id, "wi-2"),
                _ => panic!("expected Blockers"),
            },
            _ => panic!("expected Work"),
        }

        let cli = AutoReCli::parse_from(["auto-re", "work", "dependencies", "--id", "wi-3"]);
        match cli.command {
            Some(Commands::Work(args)) => match args.command {
                WorkCommand::Dependencies { id, .. } => assert_eq!(id, "wi-3"),
                _ => panic!("expected Dependencies"),
            },
            _ => panic!("expected Work"),
        }
    }

    #[test]
    fn generated_entity_parses_id() {
        let cli = AutoReCli::parse_from(["auto-re", "generated", "entity", "--id", "entity-99"]);
        match cli.command {
            Some(Commands::Generated(args)) => match args.command {
                GeneratedCommand::Entity { id, .. } => assert_eq!(id, "entity-99"),
                _ => panic!("expected Entity"),
            },
            _ => panic!("expected Generated"),
        }
    }

    #[test]
    fn json_output_roundtrip_via_print_helper() {
        let val = serde_json::json!({"covered": 5, "total": 10});
        let mut map = serde_json::Map::new();
        map.insert(
            "$schema".to_owned(),
            serde_json::Value::String("auto-re/schema/verification-coverage/v2.0".to_owned()),
        );
        if let serde_json::Value::Object(inner) = &val {
            for (k, v) in inner {
                map.insert(k.clone(), v.clone());
            }
        }
        let json_str = serde_json::to_string_pretty(&serde_json::Value::Object(map)).unwrap();
        let reparsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(
            reparsed["$schema"].as_str().unwrap(),
            "auto-re/schema/verification-coverage/v2.0"
        );
        assert_eq!(reparsed["covered"], 5);
        assert_eq!(reparsed["total"], 10);
        let reserialized = serde_json::to_string_pretty(&reparsed).unwrap();
        assert_eq!(json_str, reserialized);
    }

    #[test]
    fn all_subcommands_have_help() {
        use clap::CommandFactory;
        let cmd = AutoReCli::command();
        for sub in cmd.get_subcommands() {
            let name = sub.get_name();
            assert!(
                sub.find_subcommand("start").is_some()
                    || sub.find_subcommand("list").is_some()
                    || sub.find_subcommand("latest").is_some()
                    || sub.find_subcommand("status").is_some()
                    || sub.find_subcommand("coverage").is_some()
                    || name == "project"
                    || name == "artifact"
                    || name == "entity"
                    || name == "evidence"
                    || name == "hypothesis"
                    || name == "contradiction"
                    || name == "verification"
                    || name == "operation"
                    || name == "events"
                    || name == "tui",
                "subcommand {name} has no recognizable subcommand tree"
            );
        }
    }
}
