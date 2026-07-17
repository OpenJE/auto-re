//! Campaign CLI subcommands — read-only status display for M1.

use std::sync::Arc;

use clap::Args;

use crate::storage::Database;

/// Campaign subcommand group.
#[derive(Args)]
pub struct CampaignArgs {
    #[command(subcommand)]
    pub command: CampaignCommand,
}

/// Available campaign subcommands.
#[derive(clap::Subcommand)]
pub enum CampaignCommand {
    /// Display campaign status. Omit ID to list all campaigns.
    Status {
        /// Campaign ID (UUID). If omitted, shows all campaigns.
        id: Option<String>,
    },
    /// Run a headless campaign using mock backends (for integration testing).
    Run,
}

/// Executes the `campaign status` subcommand.
pub async fn status(db: Arc<Database>, id: Option<String>) -> crate::Result<()> {
    let conn = db.connection()?;

    if let Some(id_str) = id {
        let result = conn.query_row(
            "SELECT id, name, state FROM campaigns WHERE id = ?1",
            rusqlite::params![id_str],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        );

        match result {
            Ok((cid, name, state)) => {
                println!("{:<38} {:<20} {}", "ID", "Name", "State");
                println!("{}", "-".repeat(70));
                println!("{:<38} {:<20} {}", cid, name, state);
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                println!("No campaign found with ID: {id_str}");
            }
            Err(e) => {
                return Err(crate::Error::from(autore_core::Error::Database(
                    e.to_string(),
                )));
            }
        }
    } else {
        let mut stmt = conn
            .prepare("SELECT id, name, state FROM campaigns ORDER BY name")
            .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;

        let campaigns: Vec<_> = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;

        if campaigns.is_empty() {
            println!("No campaigns found.");
        } else {
            println!("{:<38} {:<20} {}", "ID", "Name", "State");
            println!("{}", "-".repeat(70));
            for (cid, name, state) in &campaigns {
                println!("{:<38} {:<20} {}", cid, name, state);
            }
        }
    }

    Ok(())
}
