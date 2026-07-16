//! Function entity — a function discovered in a binary.
//!
//! A `Function` represents a single function within a module of a binary
//! revision. It is identified by an address, named by one or more symbol
//! names, and tracked through analysis revisions.

use crate::domain::{Address, ContentHash, Provenance, SymbolName};
use crate::ids::{BinaryRevisionId, FunctionId, ModuleId};

/// A function discovered within a binary revision.
///
/// Fields are immutable after construction (the struct is not `Copy`).
/// Callers construct via `Function::new(...)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Function {
    /// Unique identifier for this function.
    pub id: FunctionId,
    /// The binary revision this function belongs to.
    pub binary_revision_id: BinaryRevisionId,
    /// The module within the binary that contains this function.
    pub module_id: ModuleId,
    /// The entry-point address of the function.
    pub entry_address: Address,
    /// The current (user-visible or demangled) name.
    pub current_name: SymbolName,
    /// The name assigned by the disassembly backend (IDA/Ghidra/etc.).
    pub backend_name: SymbolName,
    /// Content-addressed hash of the function's raw bytes.
    pub content_hash: ContentHash,
    /// Optional hash of the function's control-flow structure.
    pub control_flow_hash: Option<ContentHash>,
    /// How this function record was created.
    pub provenance: Provenance,
    /// Whether this function is locked against re-analysis.
    pub locked: bool,
    /// Monotonically increasing counter bumped each time analysis produces
    /// new claims about this function.
    pub analysis_revision: u64,
}

impl Function {
    /// Creates a new `Function` with the given properties.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: FunctionId,
        binary_revision_id: BinaryRevisionId,
        module_id: ModuleId,
        entry_address: Address,
        current_name: SymbolName,
        backend_name: SymbolName,
        content_hash: ContentHash,
        control_flow_hash: Option<ContentHash>,
        provenance: Provenance,
        locked: bool,
        analysis_revision: u64,
    ) -> Self {
        Function {
            id,
            binary_revision_id,
            module_id,
            entry_address,
            current_name,
            backend_name,
            content_hash,
            control_flow_hash,
            provenance,
            locked,
            analysis_revision,
        }
    }

    /// Locks this function to prevent re-analysis.
    pub fn lock(&mut self) {
        self.locked = true;
    }

    /// Unlocks this function, allowing re-analysis.
    pub fn unlock(&mut self) {
        self.locked = false;
    }

    /// Bumps the analysis revision counter, indicating new analysis output.
    pub fn bump_analysis_revision(&mut self) {
        self.analysis_revision = self.analysis_revision.wrapping_add(1);
    }

    /// Updates the current name (e.g., from a rename claim).
    pub fn rename(&mut self, new_name: SymbolName) {
        self.current_name = new_name;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Address, AddressSpace, ContentHash};
    use crate::ids::*;

    fn sample_function() -> Function {
        Function::new(
            FunctionId::new(),
            BinaryRevisionId::new(),
            ModuleId::new(),
            Address::new(AddressSpace::Virtual, 0x401000),
            SymbolName::new("main"),
            SymbolName::new("entry_point"),
            ContentHash::from_bytes(b"\x55\x48\x89\xe5"),
            None,
            Provenance::ImportedSymbol,
            false,
            0,
        )
    }

    #[test]
    fn function_new_constructs() {
        let f = sample_function();
        assert_eq!(f.current_name.to_string(), "main");
        assert_eq!(f.backend_name.to_string(), "entry_point");
        assert!(!f.locked);
        assert_eq!(f.analysis_revision, 0);
    }

    #[test]
    fn function_lock_and_unlock() {
        let mut f = sample_function();
        assert!(!f.locked);
        f.lock();
        assert!(f.locked);
        f.unlock();
        assert!(!f.locked);
    }

    #[test]
    fn function_bump_revision() {
        let mut f = sample_function();
        assert_eq!(f.analysis_revision, 0);
        f.bump_analysis_revision();
        assert_eq!(f.analysis_revision, 1);
        f.bump_analysis_revision();
        assert_eq!(f.analysis_revision, 2);
    }

    #[test]
    fn function_rename() {
        let mut f = sample_function();
        f.rename(SymbolName::new("new_main"));
        assert_eq!(f.current_name.to_string(), "new_main");
        // backend_name is unchanged
        assert_eq!(f.backend_name.to_string(), "entry_point");
    }

    #[test]
    fn function_serialize_roundtrip() {
        let f = sample_function();
        let json = serde_json::to_string(&f).unwrap();
        let deserialized: Function = serde_json::from_str(&json).unwrap();
        assert_eq!(f.id, deserialized.id);
        assert_eq!(
            f.entry_address,
            deserialized.entry_address
        );
        assert_eq!(f.current_name.to_string(), "main");
        assert!(!deserialized.locked);
    }

    #[test]
    fn function_with_control_flow_hash() {
        let cf_hash = Some(ContentHash::from_bytes(b"cfg_data"));
        let f = Function::new(
            FunctionId::new(),
            BinaryRevisionId::new(),
            ModuleId::new(),
            Address::new(AddressSpace::Virtual, 0x402000),
            SymbolName::new("helper"),
            SymbolName::new("sub_402000"),
            ContentHash::from_bytes(b"\x48\x89\xf8"),
            cf_hash.clone(),
            Provenance::StaticAnalysis,
            true,
            5,
        );
        assert!(f.locked);
        assert!(f.control_flow_hash.is_some());
        assert_eq!(f.analysis_revision, 5);
    }
}
