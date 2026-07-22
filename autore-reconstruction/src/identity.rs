//! Identity layer — re-exports for the `identity::` module path.
//!
//! Split across four sub-modules for maintainability (each ≤250 pure LOC):
//!
//! - [`key`] — [`CanonicalEntityKey`] and its `StableEntityKey` derivation.
//! - [`payload`] — JSON payload parsing helpers.
//! - [`routing`] — observation-kind → entity-kind / work-item-kind mapping.
//! - [`importer`] — [`ObservationImporter`] + [`ImportSummary`].

pub mod importer;
pub mod key;
pub mod payload;
pub mod routing;

pub use importer::{ImportSummary, ObservationImporter};
pub use key::{CANONICAL_KEY_NAMESPACE, CanonicalEntityKey};
pub use payload::{ObservationEntity, parse_observation_payload};
pub use routing::{
    entity_kind_for_observation_kind, entity_kind_from_observation, work_item_kind_for_entity,
};

// Re-exports for callers that don't want to reach into autore-provider-protocol.
pub use autore_provider_protocol::v1::{
    Diagnostic, ExecutionEvent, ObservationProduced, diagnostic, execution_event,
};

/// Stale-diagnostic reason stored on `BlockWorkItemRequest::reason`.
pub const STALE_BLOCK_REASON: &str = "ProviderObservedStaleEntity";

/// Errors produced by the identity / importer layer.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("observation payload failed to parse: {0}")]
    InvalidPayload(String),
    #[error("missing field in entity payload: {0}")]
    MissingField(String),
    #[error("application command failed: {0}")]
    CommandFailure(String),
}

impl From<IdentityError> for autore_core::Error {
    fn from(value: IdentityError) -> Self {
        autore_core::Error::Validation(value.to_string())
    }
}

// ---------------------------------------------------------------------------
// Tests (kept here so they run as `identity::tests::*` per the plan).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::key::CanonicalEntityKey;
    use super::routing::entity_kind_for_observation_kind;
    use super::{
        Diagnostic, ImportSummary, ObservationImporter, ObservationProduced, STALE_BLOCK_REASON,
        diagnostic, parse_observation_payload,
    };
    use crate::tests_support::RecordingAutoReClient;
    use autore_app::ApplicationCommand;
    use autore_schema::domain::records::ENTITY_KIND_FUNCTION;
    use autore_schema::ids::{ArtifactId, ProjectId, ProviderRunId, ReconstructionCampaignId};
    use std::collections::HashMap;

    fn sample_binary() -> ArtifactId {
        ArtifactId::from_uuid(uuid::Uuid::nil())
    }

    fn sample_observation_flat(
        kind: &str,
        address_space: u32,
        entry_address: u64,
        ea: Option<&str>,
    ) -> ObservationProduced {
        let mut payload = serde_json::Map::new();
        payload.insert(
            "address_space".into(),
            serde_json::Value::Number(address_space.into()),
        );
        payload.insert(
            "entry_address".into(),
            serde_json::Value::Number(entry_address.into()),
        );
        payload.insert(
            "display_name".into(),
            serde_json::Value::String(format!("fn_{entry_address}")),
        );
        if let Some(ea) = ea {
            payload.insert("ea".into(), serde_json::Value::String(ea.into()));
        }
        ObservationProduced {
            provider_instance_id: String::new(),
            request_id: String::new(),
            operation_id: String::new(),
            capability_id: String::new(),
            capability_version: String::new(),
            sequence: 0,
            observation_kind: kind.to_string(),
            payload: serde_json::to_vec(&payload).unwrap(),
            artifacts: Vec::new(),
        }
    }

    fn canonical_with_ext(binary: ArtifactId, ext: Vec<(&str, &str)>) -> CanonicalEntityKey {
        let mut m = HashMap::new();
        for (k, v) in ext {
            m.insert(k.to_string(), serde_json::Value::String(v.to_string()));
        }
        CanonicalEntityKey {
            binary_revision_id: binary,
            address_space: 1,
            entry_address: 0x1000,
            entity_kind: ENTITY_KIND_FUNCTION.clone(),
            provider_native_extension: m,
        }
    }

    #[test]
    fn canonical_key_is_stable_across_refresh() {
        let binary = sample_binary();
        let a = canonical_with_ext(binary, vec![("ea", "0x401000")]);
        let b = canonical_with_ext(
            binary,
            vec![("ea", "0x5001000"), ("ida_function_row_uuid", "uuid-2")],
        );
        assert_eq!(
            a.stable_key(),
            b.stable_key(),
            "stable key must ignore extension"
        );
        assert_eq!(a.identity_hash(), b.identity_hash());
    }

    #[test]
    fn canonical_key_excludes_ida_row_id() {
        // Negative proof: if ea were part of the canonical key, two
        // refreshes at different eas would produce different keys.
        // The actual implementation excludes the extension, so the
        // stable keys match; a hypothetical key that included ea
        // would diverge.
        let binary = sample_binary();
        let key_a = canonical_with_ext(binary, vec![("ea", "0x401000")]);
        let key_b = canonical_with_ext(binary, vec![("ea", "0x5001000")]);
        // Correct implementation: stable keys match despite differing eas.
        assert_eq!(key_a.stable_key(), key_b.stable_key());

        // Hypothetical incorrect implementation: serialising the
        // entire struct (extension included) produces DIFFERENT JSON.
        let wrong_a = serde_json::to_string(&key_a).unwrap();
        let wrong_b = serde_json::to_string(&key_b).unwrap();
        assert_ne!(
            wrong_a, wrong_b,
            "full-struct serialisation must differ on ea"
        );
    }

    #[test]
    fn import_observations_creates_entities_with_stable_keys() {
        let client = RecordingAutoReClient::new();
        let importer = ObservationImporter::new(&client);
        let binary = sample_binary();
        let campaign = ReconstructionCampaignId::new();
        let project = ProjectId::new();
        let run = ProviderRunId::new();

        let obs = vec![
            sample_observation_flat("ida.ingest.functions", 1, 0x1000, Some("0x401000")),
            sample_observation_flat("ida.ingest.functions", 1, 0x2000, Some("0x402000")),
            sample_observation_flat("ida.ingest.types", 1, 0x3000, None),
        ];

        let summary: ImportSummary = importer
            .import(&obs, binary, campaign, project, run)
            .unwrap();

        assert_eq!(summary.entities_created, 3);
        assert_eq!(summary.entities_rematched, 0);
        assert_eq!(
            client.count(|c| matches!(c, ApplicationCommand::RegisterEntity(_))),
            3
        );
        assert_eq!(
            client.count(|c| matches!(c, ApplicationCommand::ImportProviderRunResult(_))),
            0
        );

        for cmd in client.commands() {
            if let ApplicationCommand::RegisterEntity(req) = cmd {
                assert!(
                    req.stable_key.is_some(),
                    "every registered entity must carry a stable_key"
                );
            }
        }
    }

    #[test]
    fn import_observations_rematches_by_key_on_refresh() {
        let client = RecordingAutoReClient::new();
        let importer = ObservationImporter::new(&client);
        let binary = sample_binary();
        let campaign = ReconstructionCampaignId::new();
        let project = ProjectId::new();
        let run1 = ProviderRunId::new();
        let run2 = ProviderRunId::new();

        let first = vec![sample_observation_flat(
            "ida.ingest.functions",
            1,
            0x1000,
            Some("0x401000"),
        )];
        let summary1 = importer
            .import(&first, binary, campaign, project, run1)
            .unwrap();
        assert_eq!(summary1.entities_created, 1);
        assert_eq!(summary1.entities_rematched, 0);

        // Second observation with the SAME canonical tuple but a
        // relocated ea → must rematch, NOT create.
        let second = vec![sample_observation_flat(
            "ida.ingest.functions",
            1,
            0x1000,
            Some("0x5001000"),
        )];
        let summary2 = importer
            .import(&second, binary, campaign, project, run2)
            .unwrap();
        assert_eq!(summary2.entities_created, 0);
        assert_eq!(summary2.entities_rematched, 1);
        assert_eq!(
            client.count(|c| matches!(c, ApplicationCommand::ImportProviderRunResult(_))),
            1
        );
    }

    #[test]
    fn import_observations_does_not_delete_on_stale() {
        let client = RecordingAutoReClient::new();
        let importer = ObservationImporter::new(&client);
        let binary = sample_binary();
        let campaign = ReconstructionCampaignId::new();
        let project = ProjectId::new();
        let run = ProviderRunId::new();

        let first = vec![sample_observation_flat(
            "ida.ingest.functions",
            1,
            0x1000,
            Some("0x401000"),
        )];
        importer
            .import(&first, binary, campaign, project, run)
            .unwrap();
        assert_eq!(
            client.count(|c| matches!(c, ApplicationCommand::RegisterEntity(_))),
            1
        );

        let stale = Diagnostic {
            provider_instance_id: String::new(),
            request_id: "work-item-1".into(),
            operation_id: String::new(),
            capability_id: String::new(),
            capability_version: String::new(),
            sequence: 0,
            severity: diagnostic::Severity::Warning as i32,
            code: "stale".into(),
            message: "entity no longer present in IDA".into(),
        };
        let summary = importer
            .import_stale_diagnostics(&[stale], project, campaign)
            .unwrap();
        assert_eq!(summary.stale_blocked, 1);
        assert_eq!(summary.investigations_created, 1);

        // Entity is still registered — no delete was issued (none
        // exists in the command vocabulary).
        let key = canonical_with_ext(binary, vec![]).stable_key();
        assert!(
            client.find_by_stable_key(project, &key).is_some(),
            "stale diagnostic must not delete the entity"
        );
    }

    #[test]
    fn import_observations_creates_investigation_for_stale() {
        let client = RecordingAutoReClient::new();
        let importer = ObservationImporter::new(&client);
        let project = ProjectId::new();
        let campaign = ReconstructionCampaignId::new();

        let stale = Diagnostic {
            provider_instance_id: String::new(),
            request_id: "work-item-A".into(),
            operation_id: String::new(),
            capability_id: String::new(),
            capability_version: String::new(),
            sequence: 0,
            severity: diagnostic::Severity::Warning as i32,
            code: "stale".into(),
            message: "vanished".into(),
        };
        let non_stale = Diagnostic {
            code: "transient-io".into(),
            ..stale.clone()
        };

        let summary = importer
            .import_stale_diagnostics(&[stale, non_stale], project, campaign)
            .unwrap();

        assert_eq!(summary.stale_blocked, 1);
        assert_eq!(summary.investigations_created, 1);
        assert_eq!(
            client.count(|c| matches!(c, ApplicationCommand::CreateWorkItems(_))),
            1
        );

        if let Some(ApplicationCommand::BlockWorkItem(req)) = client
            .commands()
            .into_iter()
            .find(|c| matches!(c, ApplicationCommand::BlockWorkItem(_)))
        {
            assert_eq!(req.reason, STALE_BLOCK_REASON);
            assert_eq!(req.work_item_id, "work-item-A");
        } else {
            panic!("expected a BlockWorkItem command");
        }
    }

    #[test]
    fn observation_payload_with_entities_array_is_supported() {
        let payload = serde_json::json!({
            "entities": [
                {"address_space": 1, "entry_address": 0x1000, "ea": "0x401000"},
                {"address_space": 1, "entry_address": 0x2000, "ea": "0x402000"},
            ]
        });
        let obs = ObservationProduced {
            provider_instance_id: String::new(),
            request_id: String::new(),
            operation_id: String::new(),
            capability_id: String::new(),
            capability_version: String::new(),
            sequence: 0,
            observation_kind: "ida.ingest.functions".into(),
            payload: serde_json::to_vec(&payload).unwrap(),
            artifacts: Vec::new(),
        };
        let client = RecordingAutoReClient::new();
        let importer = ObservationImporter::new(&client);
        let summary = importer
            .import(
                &[obs],
                sample_binary(),
                ReconstructionCampaignId::new(),
                ProjectId::new(),
                ProviderRunId::new(),
            )
            .unwrap();
        assert_eq!(summary.entities_created, 2);
    }

    #[test]
    fn parse_observation_payload_handles_all_shapes() {
        let one = serde_json::json!({"address_space": 1, "entry_address": 0x10}).to_string();
        let out = parse_observation_payload(one.as_bytes()).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry_address, 0x10);

        let arr = serde_json::json!([
            {"address_space": 1, "entry_address": 0x10},
            {"address_space": 1, "entry_address": 0x20},
        ])
        .to_string();
        let out = parse_observation_payload(arr.as_bytes()).unwrap();
        assert_eq!(out.len(), 2);

        let keyed = serde_json::json!({"entities": [
            {"address_space": 1, "entry_address": 0x30},
        ]})
        .to_string();
        let out = parse_observation_payload(keyed.as_bytes()).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry_address, 0x30);
    }

    #[test]
    fn entity_kind_routing_covers_ida_stages() {
        assert_eq!(
            entity_kind_for_observation_kind("ida.ingest.functions").as_str(),
            "core.function"
        );
        assert_eq!(
            entity_kind_for_observation_kind("ida.ingest.types").as_str(),
            "core.type"
        );
        assert_eq!(
            entity_kind_for_observation_kind("ida.ingest.globals").as_str(),
            "core.global"
        );
        assert_eq!(
            entity_kind_for_observation_kind("ida.ingest.strings").as_str(),
            "core.string"
        );
        assert_eq!(
            entity_kind_for_observation_kind("ida.ingest.imports").as_str(),
            "core.external-function"
        );
    }
}
