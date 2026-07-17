pub use autore_core::{Error, Result};
pub use autore_events;
pub use autore_schema::{domain, ids};
pub use autore_store::storage;
pub use autore_tui::{runtime, tui};

pub mod lifecycle;

#[cfg(test)]
mod operation;

pub use lifecycle::{close_project, create_project, open_project};

pub mod application_service;

pub use application_service::{
    ApplicationCommand, ApplicationQuery, ApplicationService, AutoReClient, CommandResult,
    LocalAutoReClient, QueryResult,
};

