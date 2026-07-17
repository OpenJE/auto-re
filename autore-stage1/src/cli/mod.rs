//! CLI entry point — clap-based argument parsing and subcommand dispatch.
//!
//! The CLI is always compiled (no feature gate). When no subcommand is given
//! and the `tui` feature is enabled, the TUI launches instead. Without the
//! `tui` feature, a help message is printed.

mod campaign;
mod headless;
mod headless_queries;
mod task;

use std::ffi::OsString;
use std::sync::Arc;

use clap::Parser;

use crate::storage::Database;

/// auto-re — automated reverse engineering platform.
#[derive(Parser)]
#[command(name = "auto-re", version, about)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Manage campaigns.
    Campaign(campaign::CampaignArgs),
    /// Manage tasks.
    Task(task::TaskArgs),
}

/// Main entry point called from `main.rs`.
pub async fn run() -> crate::Result<()> {
    run_from(std::env::args_os(), None).await
}

/// Parses CLI arguments from the given iterator and dispatches to the
/// appropriate subcommand handler.
///
/// If `db` is `Some`, it is used directly (tests pass a pre-opened
/// in-memory database). If `None`, the database is opened via
/// `open_database()`, which reads `AUTO_RE_DB_PATH` or defaults to
/// `.auto-re/state.sqlite3`.
async fn run_from<I>(args: I, db: Option<Arc<Database>>) -> crate::Result<()>
where
    I: IntoIterator,
    I::Item: Into<OsString> + Clone,
{
    let cli = Cli::try_parse_from(args).map_err(|e| crate::Error::Validation(e.to_string()))?;

    match cli.command {
        Some(Commands::Campaign(c)) => match c.command {
            campaign::CampaignCommand::Status { id } => {
                let db = match db {
                    Some(db) => db,
                    None => open_database()?,
                };
                campaign::status(db, id).await
            }
            campaign::CampaignCommand::Run => {
                let db = match db {
                    Some(db) => db,
                    None => open_database()?,
                };
                headless::run_headless(db).await
            }
        },
        Some(Commands::Task(t)) => match t.command {
            task::TaskCommand::List => {
                let db = match db {
                    Some(db) => db,
                    None => open_database()?,
                };
                task::list(db).await
            }
            task::TaskCommand::Status { id } => {
                let db = match db {
                    Some(db) => db,
                    None => open_database()?,
                };
                task::status(db, id).await
            }
        },
        None => {
            #[cfg(feature = "tui")]
            {
                return crate::runtime::run().await;
            }
            #[cfg(not(feature = "tui"))]
            {
                println!("auto-re: no subcommand given. Try `auto-re --help`.");
                Ok(())
            }
        }
    }
}

/// Opens the SQLite database. Reads path from the `AUTO_RE_DB_PATH`
/// environment variable if set, otherwise defaults to
/// `.auto-re/state.sqlite3`. Creates parent directories as needed.
fn open_database() -> crate::Result<Arc<Database>> {
    let path =
        std::env::var("AUTO_RE_DB_PATH").unwrap_or_else(|_| ".auto-re/state.sqlite3".to_string());
    let db = Database::open(&path)?;
    Ok(Arc::new(db))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cli_campaign_status_runs() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let result = run_from(["auto-re", "campaign", "status"], Some(db)).await;
        assert!(result.is_ok(), "campaign status should succeed: {result:?}");
    }

    #[tokio::test]
    async fn cli_task_list_runs() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let result = run_from(["auto-re", "task", "list"], Some(db)).await;
        assert!(result.is_ok(), "task list should succeed: {result:?}");
    }

    #[tokio::test]
    async fn cli_task_status_missing_id_errors() {
        // Fails at clap argument parsing before opening the database.
        let result = run_from(["auto-re", "task", "status"], None).await;
        assert!(
            result.is_err(),
            "task status without ID should fail (clap rejects missing required arg)"
        );
    }

    #[tokio::test]
    async fn cli_task_status_invalid_uuid_errors() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let result = run_from(["auto-re", "task", "status", "not-a-uuid"], Some(db)).await;
        assert!(
            result.is_err(),
            "task status with invalid UUID should return validation error"
        );
    }

    #[tokio::test]
    async fn cli_task_status_nonexistent_id_errors() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let result = run_from(
            [
                "auto-re",
                "task",
                "status",
                "00000000-0000-0000-0000-000000000000",
            ],
            Some(db),
        )
        .await;
        assert!(
            result.is_err(),
            "task status for nonexistent ID should return error"
        );
    }

    #[tokio::test]
    async fn cli_campaign_status_with_nonexistent_id() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let result = run_from(
            [
                "auto-re",
                "campaign",
                "status",
                "00000000-0000-0000-0000-000000000000",
            ],
            Some(db),
        )
        .await;
        assert!(
            result.is_ok(),
            "campaign status with nonexistent ID should succeed (prints 'not found')"
        );
    }

    #[tokio::test]
    async fn cli_no_subcommand_no_tui() {
        // Without a subcommand, the CLI either launches TUI (feature enabled)
        // or prints a help message (feature disabled). Both paths return Ok.
        // In test mode, TUI would block, so we only test the no-tui path.
        #[cfg(not(feature = "tui"))]
        {
            let result = run_from(["auto-re"], None).await;
            assert!(result.is_ok());
        }
    }
}
