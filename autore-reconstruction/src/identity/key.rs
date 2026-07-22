//! [`CanonicalEntityKey`] — structural identity of an entity inside a
//! binary revision, plus deterministic [`StableEntityKey`] derivation.
//!
//! The four canonical fields form the stable identity; the extension map
//! is provider-native provenance (IDA `ea`, row uuid, …) that is **NOT**
//! part of the stable key. Two observations that differ only in their
//! extension map still refer to the same canonical entity.

use std::collections::{BTreeMap, HashMap};

use autore_schema::domain::{NamespacedId, StableEntityKey};
use autore_schema::ids::ArtifactId;

/// Namespace used when wrapping the canonical JSON in a [`StableEntityKey`].
pub static CANONICAL_KEY_NAMESPACE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("autore.recon.canonical").unwrap());

/// Structural identity of an entity inside a binary revision.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalEntityKey {
    pub binary_revision_id: ArtifactId,
    pub address_space: u32,
    pub entry_address: u64,
    pub entity_kind: NamespacedId,
    /// Provider-native provenance. Deliberately excluded from
    /// [`Self::stable_key`] and [`Self::identity_hash`].
    pub provider_native_extension: HashMap<String, serde_json::Value>,
}

impl CanonicalEntityKey {
    /// Constructs a canonical key with an empty extension map.
    pub fn new(
        binary_revision_id: ArtifactId,
        address_space: u32,
        entry_address: u64,
        entity_kind: NamespacedId,
    ) -> Self {
        Self {
            binary_revision_id,
            address_space,
            entry_address,
            entity_kind,
            provider_native_extension: HashMap::new(),
        }
    }

    /// Returns the deterministic [`StableEntityKey`] used for cross-session
    /// rematch. The key is the canonical JSON of the four structural
    /// fields — the extension map is excluded.
    pub fn stable_key(&self) -> StableEntityKey {
        StableEntityKey::ExternalIdentity {
            namespace: CANONICAL_KEY_NAMESPACE.clone(),
            value: self.canonical_json(),
        }
    }

    /// Returns a BLAKE3 hex digest over the canonical JSON.
    pub fn identity_hash(&self) -> String {
        let bytes = self.canonical_json().into_bytes();
        blake3::hash(&bytes).to_hex().to_string()
    }

    fn canonical_json(&self) -> String {
        let mut map = BTreeMap::new();
        map.insert(
            "binary_revision_id".to_string(),
            serde_json::Value::String(self.binary_revision_id.to_string()),
        );
        map.insert(
            "address_space".to_string(),
            serde_json::Value::Number(self.address_space.into()),
        );
        map.insert(
            "entry_address".to_string(),
            serde_json::Value::Number(self.entry_address.into()),
        );
        map.insert(
            "entity_kind".to_string(),
            serde_json::Value::String(self.entity_kind.as_str().to_string()),
        );
        serde_json::to_string(&map).expect("canonical map serialization is infallible")
    }
}
