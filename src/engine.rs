// Experimental engine module — remote TUI code, needs migration to idax.
// This module is behind the `tui` feature and is not part of the M1 plan.

mod graph;

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("io error: {0}")]
    StdIoError(#[from] std::io::Error),

    #[cfg(feature = "ida")]
    #[error("ida error: {0}")]
    IdaError(#[from] idax::Error),
}

pub type EngineResult<T> = std::result::Result<T, EngineError>;

pub struct Engine {
    #[cfg(feature = "ida")]
    _initialized: bool,
    graph: graph::RETaskGraph,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "ida")]
            _initialized: false,
            graph: graph::RETaskGraph::new(),
        }
    }

    #[cfg(feature = "ida")]
    pub fn open(&mut self, path: impl AsRef<Path>) -> EngineResult<()> {
        idax::database::init().map_err(EngineError::IdaError)?;
        idax::database::open(path.as_ref().to_str().unwrap_or(""), true)
            .map_err(EngineError::IdaError)?;
        self._initialized = true;
        Ok(())
    }
}
