pub use autore_core::{Error, Result};
pub use autore_events;
pub use autore_schema::{domain, ids};
pub use autore_store::storage as storage;
pub use autore_tui::{runtime, tui};

pub mod lifecycle;

pub use lifecycle::{close_project, create_project, open_project};
