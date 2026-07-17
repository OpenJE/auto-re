//! Analysis backend trait — abstracts over IDA, Ghidra, or mock backends.
//!
//! An `AnalysisBackend` advertises its capabilities and provides inventory
//! and per-function analysis results. All methods are async to support
//! backends that communicate over IPC or network.

use crate::domain::Function;
use crate::ids::{BinaryRevisionId, FunctionId};

/// A discrete analysis capability that a backend may advertise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AnalysisCapability {
    /// Enumerate functions within a binary revision.
    InventoryFunctions,
    /// Produce disassembly (assembly text) for a function.
    Disassemble,
    /// Produce decompiled pseudocode for a function.
    Decompile,
    /// Recover type information (signatures, structs) for a function.
    RecoverTypes,
    /// Compute the control-flow graph of a function.
    ControlFlowGraph,
    /// Compute the inter-function call graph.
    CallGraph,
}

/// Trait for analysis backends (IDA, Ghidra, mock, etc.).
///
/// Implementations must be `Send + Sync` so they can be shared across
/// async tasks. All methods return `crate::Result<T>` to propagate
/// backend-specific errors uniformly.
#[async_trait::async_trait]
pub trait AnalysisBackend: Send + Sync {
    /// Returns the set of capabilities this backend supports.
    fn capabilities(&self) -> Vec<AnalysisCapability>;

    /// Lists all functions discovered in the given binary revision.
    async fn inventory(&self, binary_rev_id: BinaryRevisionId) -> crate::Result<Vec<Function>>;

    /// Runs a single analysis capability on a specific function.
    ///
    /// Returns a backend-specific string representation of the result
    /// (e.g., pseudocode text, CFG dot graph, type signature).
    async fn analyze(
        &self,
        function_id: FunctionId,
        capability: AnalysisCapability,
    ) -> crate::Result<String>;
}
