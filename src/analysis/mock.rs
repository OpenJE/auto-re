//! Deterministic mock analysis backend for testing.
//!
//! `MockAnalysisBackend` produces a fixed set of 10 synthetic functions
//! and returns deterministic analysis output. No I/O, no randomness.

use std::collections::HashMap;

use crate::analysis::backend::{AnalysisBackend, AnalysisCapability};
use crate::domain::{Address, AddressSpace, ContentHash, Function, Provenance, SymbolName};
use crate::ids::{BinaryRevisionId, FunctionId, ModuleId};

/// Number of synthetic functions in the mock fixture.
const FIXTURE_SIZE: usize = 10;

/// Capabilities the mock backend advertises.
const ADVERTISED: &[AnalysisCapability] = &[
    AnalysisCapability::InventoryFunctions,
    AnalysisCapability::Disassemble,
    AnalysisCapability::Decompile,
    AnalysisCapability::ControlFlowGraph,
];

/// A deterministic mock analysis backend.
///
/// Every instantiation produces the same 10 synthetic functions with
/// fixed IDs, addresses, names, and hashes. Analysis output is a pure
/// function of `(function_id, capability)`.
pub struct MockAnalysisBackend {
    functions: Vec<Function>,
    by_id: HashMap<FunctionId, usize>,
}

impl MockAnalysisBackend {
    /// Creates a new mock backend with a fixed 10-function fixture.
    pub fn new() -> Self {
        let functions = Self::build_fixture();
        let by_id = functions
            .iter()
            .enumerate()
            .map(|(i, f)| (f.id, i))
            .collect();
        MockAnalysisBackend { functions, by_id }
    }

    fn build_fixture() -> Vec<Function> {
        let binary_rev_id = BinaryRevisionId::from_uuid(uuid::Uuid::from_bytes([
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x01,
        ]));
        let module_id = ModuleId::from_uuid(uuid::Uuid::from_bytes([
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x02,
        ]));

        let names: [&str; FIXTURE_SIZE] = [
            "entry_point",
            "parse_header",
            "validate_input",
            "compute_hash",
            "lookup_symbol",
            "resolve_reloc",
            "emit_output",
            "handle_error",
            "cleanup_resources",
            "main_loop",
        ];

        (0..FIXTURE_SIZE)
            .map(|i| {
                let mut id_bytes = [0u8; 16];
                id_bytes[14] = (i >> 8) as u8;
                id_bytes[15] = i as u8;
                // Set UUID version/variant bits for validity
                id_bytes[6] = (id_bytes[6] & 0x0f) | 0x40; // version 4
                id_bytes[8] = (id_bytes[8] & 0x3f) | 0x80; // variant 1

                let func_id = FunctionId::from_uuid(uuid::Uuid::from_bytes(id_bytes));
                let addr = 0x401000u128 + (i as u128 * 0x100);
                let cf_bytes = format!("cfg_{i}_{name}", name = names[i]);

                Function::new(
                    func_id,
                    binary_rev_id,
                    module_id,
                    Address::new(AddressSpace::Virtual, addr),
                    SymbolName::new(names[i]),
                    SymbolName::new(format!("sub_{addr:x}")),
                    ContentHash::from_bytes(format!("bytes_{i}").as_bytes()),
                    Some(ContentHash::from_bytes(cf_bytes.as_bytes())),
                    Provenance::StaticAnalysis,
                    false,
                    0,
                )
            })
            .collect()
    }
}

impl Default for MockAnalysisBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl AnalysisBackend for MockAnalysisBackend {
    fn capabilities(&self) -> Vec<AnalysisCapability> {
        ADVERTISED.to_vec()
    }

    async fn inventory(&self, _binary_rev_id: BinaryRevisionId) -> crate::Result<Vec<Function>> {
        Ok(self.functions.clone())
    }

    async fn analyze(
        &self,
        function_id: FunctionId,
        capability: AnalysisCapability,
    ) -> crate::Result<String> {
        if !ADVERTISED.contains(&capability) {
            return Err(crate::Error::AnalysisBackend(format!(
                "unsupported capability: {capability:?}"
            )));
        }

        let idx = self.by_id.get(&function_id).ok_or_else(|| {
            crate::Error::AnalysisBackend(format!("unknown function: {function_id}"))
        })?;

        let func = &self.functions[*idx];
        let name = &func.current_name;

        let output = match capability {
            AnalysisCapability::InventoryFunctions => {
                format!("inventory entry for {name}")
            }
            AnalysisCapability::Disassemble => {
                format!("push rbp\nmov rbp, rsp\n; body of {name}\npop rbp\nret")
            }
            AnalysisCapability::Decompile => {
                format!("void {name}() {{ /* decompiled */ }}")
            }
            AnalysisCapability::ControlFlowGraph => {
                format!("digraph {name} {{ entry -> body -> exit }}")
            }
            // These are not advertised; unreachable due to the guard above.
            AnalysisCapability::RecoverTypes | AnalysisCapability::CallGraph => {
                unreachable!("guarded by ADVERTISED check")
            }
        };

        Ok(output)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_backend_inventory_returns_ten_functions() {
        let backend = MockAnalysisBackend::new();
        let binary_rev = BinaryRevisionId::new();
        let functions = backend.inventory(binary_rev).await.unwrap();
        assert_eq!(functions.len(), FIXTURE_SIZE);

        // All IDs are distinct
        let mut ids: Vec<FunctionId> = functions.iter().map(|f| f.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), FIXTURE_SIZE);

        // All addresses are distinct
        let mut addrs: Vec<u128> = functions.iter().map(|f| f.entry_address.value).collect();
        addrs.sort();
        addrs.dedup();
        assert_eq!(addrs.len(), FIXTURE_SIZE);

        // All names are distinct
        let mut names: Vec<String> = functions
            .iter()
            .map(|f| f.current_name.to_string())
            .collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), FIXTURE_SIZE);
    }

    #[test]
    fn mock_backend_capabilities() {
        let backend = MockAnalysisBackend::new();
        let caps = backend.capabilities();
        assert!(caps.contains(&AnalysisCapability::InventoryFunctions));
        assert!(caps.contains(&AnalysisCapability::Disassemble));
        assert!(caps.contains(&AnalysisCapability::Decompile));
        assert!(caps.contains(&AnalysisCapability::ControlFlowGraph));
        assert!(!caps.contains(&AnalysisCapability::RecoverTypes));
        assert!(!caps.contains(&AnalysisCapability::CallGraph));
    }

    #[tokio::test]
    async fn mock_backend_analyze_is_deterministic() {
        let backend = MockAnalysisBackend::new();
        let binary_rev = BinaryRevisionId::new();
        let functions = backend.inventory(binary_rev).await.unwrap();
        let func_id = functions[0].id;

        let result_a = backend
            .analyze(func_id, AnalysisCapability::Decompile)
            .await
            .unwrap();
        let result_b = backend
            .analyze(func_id, AnalysisCapability::Decompile)
            .await
            .unwrap();
        assert_eq!(result_a, result_b);

        // Different capability produces different output
        let disasm = backend
            .analyze(func_id, AnalysisCapability::Disassemble)
            .await
            .unwrap();
        assert_ne!(result_a, disasm);

        // Two separate backend instances produce the same output
        let backend2 = MockAnalysisBackend::new();
        let result_c = backend2
            .analyze(func_id, AnalysisCapability::Decompile)
            .await
            .unwrap();
        assert_eq!(result_a, result_c);
    }

    #[tokio::test]
    async fn unsupported_capability_returns_error() {
        let backend = MockAnalysisBackend::new();
        let binary_rev = BinaryRevisionId::new();
        let functions = backend.inventory(binary_rev).await.unwrap();
        let func_id = functions[0].id;

        let result = backend
            .analyze(func_id, AnalysisCapability::RecoverTypes)
            .await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unsupported capability"),
            "unexpected error: {err_msg}"
        );

        // Unknown function ID also returns an error
        let unknown_id = FunctionId::new();
        let result = backend
            .analyze(unknown_id, AnalysisCapability::Disassemble)
            .await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unknown function"),
            "unexpected error: {err_msg}"
        );
    }
}
