//! Worker packet builder — bounded, deterministic analysis packets.
//!
//! `FunctionAnalysisPacket` is the unit of work dispatched to analysis workers.
//! It contains only typed domain primitives (no raw backend types) and is
//! fully deterministic: equal inputs produce byte-equal serializations.

use crate::analysis::backend::{AnalysisBackend, AnalysisCapability};
use crate::analysis::mock::MockAnalysisBackend;
use crate::domain::{Address, ContentHash, SymbolName};
use crate::ids::{BinaryRevisionId, FunctionId, ModuleId};

/// A bounded, deterministic packet describing a single function for analysis.
///
/// Contains no raw backend types — only typed IDs and domain primitives.
/// All fields implement `Hash`, making packets suitable for deduplication
/// in sets and maps.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FunctionAnalysisPacket {
    /// The function to analyze.
    pub function_id: FunctionId,
    /// The binary revision context.
    pub binary_revision_id: BinaryRevisionId,
    /// The module containing the function.
    pub module_id: ModuleId,
    /// Entry-point address of the function.
    pub address: Address,
    /// Current symbol name, if known.
    pub symbol_name: Option<SymbolName>,
    /// Control-flow hash from prior analysis, if available.
    pub control_flow_hash: Option<ContentHash>,
    /// Functions that call this function.
    pub callers: Vec<FunctionId>,
    /// Functions called by this function.
    pub callees: Vec<FunctionId>,
    /// Capabilities the worker should apply.
    pub requested_capabilities: Vec<AnalysisCapability>,
}

/// Builds `FunctionAnalysisPacket`s from backend inventory.
///
/// Implementations must be `Send + Sync` so they can be shared across
/// async worker tasks.
#[async_trait::async_trait]
pub trait PacketBuilder: Send + Sync {
    /// Constructs a packet for the given function, requesting the specified
    /// capabilities. Returns an error if the function is not found in the
    /// backend's inventory.
    async fn build_packet(
        &self,
        function_id: FunctionId,
        capabilities: Vec<AnalysisCapability>,
    ) -> crate::Result<FunctionAnalysisPacket>;
}

/// A `PacketBuilder` backed by `MockAnalysisBackend`.
///
/// Builds packets from the mock's deterministic fixture inventory.
/// Callers and callees are always empty (the mock does not model a call graph).
pub struct MockPacketBuilder {
    backend: MockAnalysisBackend,
}

impl MockPacketBuilder {
    /// Creates a new builder wrapping the given mock backend.
    pub fn new(backend: MockAnalysisBackend) -> Self {
        MockPacketBuilder { backend }
    }
}

#[async_trait::async_trait]
impl PacketBuilder for MockPacketBuilder {
    async fn build_packet(
        &self,
        function_id: FunctionId,
        capabilities: Vec<AnalysisCapability>,
    ) -> crate::Result<FunctionAnalysisPacket> {
        // The mock backend ignores the BinaryRevisionId argument.
        let inventory = self.backend.inventory(BinaryRevisionId::new()).await?;

        let func = inventory
            .into_iter()
            .find(|f| f.id == function_id)
            .ok_or_else(|| {
                crate::Error::AnalysisBackend(format!(
                    "function not found in inventory: {function_id}"
                ))
            })?;

        Ok(FunctionAnalysisPacket {
            function_id: func.id,
            binary_revision_id: func.binary_revision_id,
            module_id: func.module_id,
            address: func.entry_address,
            symbol_name: Some(func.current_name),
            control_flow_hash: func.control_flow_hash,
            callers: Vec::new(),
            callees: Vec::new(),
            requested_capabilities: capabilities,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn builder_and_first_function_id() -> (MockPacketBuilder, FunctionId) {
        let backend = MockAnalysisBackend::new();
        let builder = MockPacketBuilder::new(backend);
        // Deterministic fixture: the first function ID from the mock.
        let mut id_bytes = [0u8; 16];
        id_bytes[6] = 0x40; // version 4
        id_bytes[8] = 0x80; // variant 1
        let func_id = FunctionId::from_uuid(uuid::Uuid::from_bytes(id_bytes));
        (builder, func_id)
    }

    #[tokio::test]
    async fn packet_is_serializable() {
        let (builder, func_id) = builder_and_first_function_id();
        let caps = vec![AnalysisCapability::Decompile];
        let packet = builder.build_packet(func_id, caps).await.unwrap();

        let json = serde_json::to_string(&packet).unwrap();
        let deserialized: FunctionAnalysisPacket = serde_json::from_str(&json).unwrap();
        assert_eq!(packet, deserialized);
    }

    #[tokio::test]
    async fn packet_is_deterministic() {
        let (builder, func_id) = builder_and_first_function_id();
        let caps = vec![
            AnalysisCapability::Disassemble,
            AnalysisCapability::Decompile,
        ];

        let packet_a = builder.build_packet(func_id, caps.clone()).await.unwrap();
        let packet_b = builder.build_packet(func_id, caps).await.unwrap();
        assert_eq!(packet_a, packet_b);

        // Serialization is byte-equal across builds.
        let json_a = serde_json::to_string(&packet_a).unwrap();
        let json_b = serde_json::to_string(&packet_b).unwrap();
        assert_eq!(json_a, json_b);
    }

    #[tokio::test]
    async fn packet_builder_uses_mock_backend() {
        let (builder, func_id) = builder_and_first_function_id();
        let caps = vec![AnalysisCapability::ControlFlowGraph];
        let packet = builder.build_packet(func_id, caps.clone()).await.unwrap();

        assert_eq!(packet.function_id, func_id);
        assert_eq!(packet.requested_capabilities, caps);
        assert!(packet.symbol_name.is_some());
        assert_eq!(
            packet.symbol_name.as_ref().unwrap().to_string(),
            "entry_point"
        );
        assert!(packet.control_flow_hash.is_some());
        assert!(packet.callers.is_empty());
        assert!(packet.callees.is_empty());

        // Unknown function ID returns an error.
        let unknown = FunctionId::new();
        let result = builder.build_packet(unknown, vec![]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn packet_hashes_equal_for_equal_input() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let (builder, func_id) = builder_and_first_function_id();
        let caps = vec![AnalysisCapability::Decompile];

        let packet_a = builder.build_packet(func_id, caps.clone()).await.unwrap();
        let packet_b = builder.build_packet(func_id, caps).await.unwrap();

        let hash = |p: &FunctionAnalysisPacket| {
            let mut h = DefaultHasher::new();
            p.hash(&mut h);
            h.finish()
        };

        assert_eq!(hash(&packet_a), hash(&packet_b));

        // Different capabilities produce different hashes.
        let packet_c = builder
            .build_packet(func_id, vec![AnalysisCapability::Disassemble])
            .await
            .unwrap();
        assert_ne!(hash(&packet_a), hash(&packet_c));
    }
}
