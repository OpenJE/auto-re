//! Task CLI subcommands — read-only list and status display for M1.

use std::sync::Arc;

use clap::Args;

use crate::storage::Database;

/// Task subcommand group.
#[derive(Args)]
pub struct TaskArgs {
    #[command(subcommand)]
    pub command: TaskCommand,
}

/// Available task subcommands.
#[derive(clap::Subcommand)]
pub enum TaskCommand {
    /// List all tasks across campaigns.
    List,
    /// Display detailed status for a specific task.
    Status {
        /// Task ID (UUID).
        id: String,
    },
}

/// Executes the `task list` subcommand.
pub async fn list(db: Arc<Database>) -> crate::Result<()> {
    let conn = db.connection()?;

    let mut stmt = conn
        .prepare(
            "SELECT id, campaign_id, kind, state, priority \
             FROM tasks ORDER BY priority DESC",
        )
        .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;

    let tasks: Vec<_> = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;

    if tasks.is_empty() {
        println!("No tasks found.");
    } else {
        println!(
            "{:<38} {:<38} {:<22} {:<12} {}",
            "ID", "Campaign", "Kind", "State", "Priority"
        );
        println!("{}", "-".repeat(120));
        for (id, campaign_id, kind, state, priority) in &tasks {
            println!(
                "{:<38} {:<38} {:<22} {:<12} {}",
                id, campaign_id, kind, state, priority
            );
        }
    }

    Ok(())
}

/// Executes the `task status <id>` subcommand.
pub async fn status(db: Arc<Database>, id_str: String) -> crate::Result<()> {
    let uuid = uuid::Uuid::parse_str(&id_str).map_err(|e| {
        crate::Error::from(autore_core::Error::Validation(format!(
            "invalid task ID '{id_str}': {e}"
        )))
    })?;
    let _task_id = crate::TaskId::from_uuid(uuid);

    let conn = db.connection()?;

    let result = conn.query_row(
        "SELECT id, campaign_id, kind, subject, state, priority, \
         attempt_count, maximum_attempts, error_message \
         FROM tasks WHERE id = ?1",
        rusqlite::params![id_str],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, u32>(6)?,
                row.get::<_, u32>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        },
    );

    match result {
        Ok((id, campaign_id, kind, subject, state, priority, attempts, max_attempts, error)) => {
            println!("Task Status");
            println!("{}", "-".repeat(40));
            println!("  ID:               {id}");
            println!("  Campaign:         {campaign_id}");
            println!("  Kind:             {kind}");
            println!("  Subject:          {subject}");
            println!("  State:            {state}");
            println!("  Priority:         {priority}");
            println!("  Attempts:         {attempts}/{max_attempts}");
            if let Some(err) = error {
                println!("  Error:            {err}");
            }
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(crate::Error::from(autore_core::Error::Validation(format!(
                "task not found: {id_str}"
            ))));
        }
        Err(e) => {
            return Err(crate::Error::from(autore_core::Error::Database(
                e.to_string(),
            )));
        }
    }

    Ok(())
}
