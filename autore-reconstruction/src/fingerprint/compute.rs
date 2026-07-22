//! Fingerprint computation — deterministic BLAKE3 hashing of work-item inputs.
//!
//! A work item's fingerprint is a content hash over every input that can
//! affect its output: ingested snapshots, accepted hypotheses, upstream
//! declarations, dynamic observations, and configuration hashes.  If any
//! input changes the fingerprint changes; if nothing changes the
//! fingerprint is stable.

use std::collections::BTreeMap;

use autore_schema::domain::ContentHash;
use autore_schema::ids::HypothesisId;

/// All inputs that contribute to a work item's deterministic fingerprint.
#[derive(Debug, Clone)]
pub struct FingerprintInput {
    /// Content hashes of relevant ingested snapshots.
    pub static_artifact_hashes: Vec<ContentHash>,
    /// IDs of accepted hypotheses (via `AcceptHypothesisPolicyDriven`).
    pub accepted_hypotheses: Vec<HypothesisId>,
    /// `last_change_hash` from `generated_source_mappings` for upstream
    /// entity kinds.
    pub upstream_declarations: Vec<ContentHash>,
    /// Artifact content hashes of `debug.observation` artifacts.
    pub dynamic_observations: Vec<ContentHash>,
    /// Prompt template version (from `prompt_versions.toml` or a constant).
    pub prompt_template_version: String,
    /// Hash of model sampling params + endpoint config (no plaintext secrets).
    pub model_config_hash: ContentHash,
    /// Build-profile hash from the project.
    pub build_config_hash: ContentHash,
    /// Verification-policy string hash.
    pub verification_policy_hash: ContentHash,
}

/// Result of comparing a recomputed fingerprint against a stored one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FingerprintComparison {
    Changed,
    Unchanged,
    FirstTime,
}

/// Computes a deterministic BLAKE3 content hash over the given input.
pub fn compute_fingerprint(input: &FingerprintInput) -> ContentHash {
    let canonical = canonical_json(input);
    ContentHash::blake3(canonical.as_bytes())
}

/// Compares a recomputed fingerprint against a stored one.
pub fn compare_fingerprint(
    computed: &ContentHash,
    stored: Option<&ContentHash>,
) -> FingerprintComparison {
    match stored {
        Some(s) if s == computed => FingerprintComparison::Unchanged,
        Some(_) => FingerprintComparison::Changed,
        None => FingerprintComparison::FirstTime,
    }
}

fn canonical_json(input: &FingerprintInput) -> String {
    let mut map = BTreeMap::new();

    let sorted_hypotheses: Vec<String> = {
        let mut ids: Vec<String> = input
            .accepted_hypotheses
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        ids.sort();
        ids
    };

    map.insert(
        "accepted_hypotheses",
        serde_json::to_value(&sorted_hypotheses).expect("json"),
    );
    map.insert(
        "build_config_hash",
        serde_json::to_value(input.build_config_hash.digest_hex()).expect("json"),
    );
    map.insert(
        "dynamic_observations",
        serde_json::to_value(sorted_hex_vec(&input.dynamic_observations)).expect("json"),
    );
    map.insert(
        "model_config_hash",
        serde_json::to_value(input.model_config_hash.digest_hex()).expect("json"),
    );
    map.insert(
        "prompt_template_version",
        serde_json::to_value(&input.prompt_template_version).expect("json"),
    );
    map.insert(
        "static_artifact_hashes",
        serde_json::to_value(sorted_hex_vec(&input.static_artifact_hashes)).expect("json"),
    );
    map.insert(
        "upstream_declarations",
        serde_json::to_value(sorted_hex_vec(&input.upstream_declarations)).expect("json"),
    );
    map.insert(
        "verification_policy_hash",
        serde_json::to_value(input.verification_policy_hash.digest_hex()).expect("json"),
    );

    serde_json::to_string(&map).expect("json")
}

fn sorted_hex_vec(hashes: &[ContentHash]) -> Vec<String> {
    let mut hexes: Vec<String> = hashes.iter().map(ContentHash::digest_hex).collect();
    hexes.sort();
    hexes
}
