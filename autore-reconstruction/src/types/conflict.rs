//! LLM-driven conflict arbitration for type-layout conflicts.
//!
//! When the deterministic reconciler emits a `ConflictResolution` work item,
//! this module builds the `llm.analysis.conflict` bundle, parses the LLM's
//! structured recommendation, and issues policy-driven commands
//! (`AcceptHypothesisPolicyDriven`, `InvalidateGeneratedSource`) per spec §10.3.

use autore_app::ApplicationCommand;
use autore_app::application_service::requests::{
    AcceptHypothesisPolicyDrivenRequest, InvalidateGeneratedSourceRequest,
};
use autore_core::Result;
use autore_schema::domain::records::{GeneratedSourceMapping, Hypothesis, PolicyDecision};
use autore_schema::ids::{EntityId, EvidenceRecordId, HypothesisId, ProjectId, WorkItemId};
use serde_json::Value;

use crate::analysis::bundle::InvestigationBundle;
use crate::analysis::schemas::validate_response_payload;
use crate::types::constraint::LayoutConstraint;

const CAPABILITY_ID: &str = "llm.analysis.conflict";

/// Errors specific to conflict arbitration.
#[derive(Debug, thiserror::Error)]
pub enum ConflictError {
    #[error("invalid LLM response: {0}")]
    InvalidResponse(String),
    #[error("LLM analysis failed: {0}")]
    LlmFailed(String),
    #[error("hypothesis {0} not found in conflict set")]
    HypothesisNotFound(HypothesisId),
    #[error("mapping {0} has no usable mapping_id")]
    InvalidMapping(String),
}

/// Trait for LLM clients capable of running `llm.analysis.conflict`.
pub trait ConflictLlm {
    /// Runs the conflict-analysis capability and returns the parsed JSON response.
    fn analyze_conflict(&self, bundle: &InvestigationBundle) -> Result<Value, ConflictError>;
}

/// Parsed outcome of a conflict-analysis response.
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictResolution {
    pub decision: PolicyDecision,
    pub target_hypothesis_id: HypothesisId,
    pub superseding_hypothesis_id: Option<HypothesisId>,
    pub rationale: String,
    pub evidence_references: Vec<String>,
    pub confidence: f64,
}

/// Arbitrates a layout conflict by invoking `llm.analysis.conflict`.
#[derive(Debug, Clone, Default)]
pub struct ConflictArbitrator;

impl ConflictArbitrator {
    /// Creates a new arbitrator.
    pub fn new() -> Self {
        Self
    }

    /// Analyzes a conflict and returns the canonical commands to apply.
    ///
    /// The returned commands are *not* executed; the caller (typically the
    /// coordinator worker) issues them through an [`AutoReClient`] so that
    /// every mutation is recorded in the event log.
    #[allow(clippy::too_many_arguments)]
    pub fn arbitrate(
        &self,
        project: ProjectId,
        subject: EntityId,
        hypotheses: &[Hypothesis],
        constraints: &[LayoutConstraint],
        evidence: &[EvidenceRecordId],
        mappings: &[GeneratedSourceMapping],
        llm: &dyn ConflictLlm,
    ) -> Result<Vec<ApplicationCommand>, ConflictError> {
        let bundle = build_conflict_bundle(subject, hypotheses, constraints, evidence)?;
        let response = llm.analyze_conflict(&bundle)?;
        let resolution = parse_conflict_resolution(&response, hypotheses)?;
        Ok(emit_commands(project, subject, &resolution, mappings))
    }
}

/// Builds the bounded investigation bundle for `llm.analysis.conflict`.
fn build_conflict_bundle(
    subject: EntityId,
    hypotheses: &[Hypothesis],
    _constraints: &[LayoutConstraint],
    _evidence: &[EvidenceRecordId],
) -> Result<InvestigationBundle, ConflictError> {
    let requested_output_schema = crate::analysis::schemas::response_schema_for(CAPABILITY_ID)
        .ok_or_else(|| ConflictError::InvalidResponse(format!("no schema for {CAPABILITY_ID}")))?;

    Ok(InvestigationBundle {
        subject_identity: WorkItemId::new(),
        subject_entity_id: Some(subject),
        static_structural_snapshot: None,
        decompilation_artifact: None,
        disassembly_artifact: None,
        cfg_summary: None,
        callers_and_callees: vec![],
        relevant_types: vec![subject],
        relevant_globals: vec![],
        strings_and_constants: vec![],
        dynamic_observations: vec![],
        accepted_hypotheses: hypotheses.iter().map(|h| h.id).collect(),
        unresolved_conflicts: vec![],
        prior_generated_candidate: None,
        compiler_diagnostics: vec![],
        verification_failures: vec![],
        requested_output_schema,
    })
}

/// Validates and parses a conflict-analysis response.
fn parse_conflict_resolution(
    response: &Value,
    hypotheses: &[Hypothesis],
) -> Result<ConflictResolution, ConflictError> {
    validate_response_payload(CAPABILITY_ID, response).map_err(ConflictError::InvalidResponse)?;

    let kind = response
        .get("resolution_kind")
        .and_then(Value::as_str)
        .ok_or_else(|| ConflictError::InvalidResponse("missing resolution_kind".into()))?;
    let rationale = response
        .get("rationale")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let confidence = response
        .get("confidence")
        .and_then(Value::as_f64)
        .unwrap_or(0.5);
    let evidence_references = response
        .get("evidence_references")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    match kind.to_ascii_lowercase().as_str() {
        "accept" => {
            let target_id = parse_hypothesis_id(response, "accepted_hypothesis_id", hypotheses)?;
            Ok(ConflictResolution {
                decision: PolicyDecision::Accept,
                target_hypothesis_id: target_id,
                superseding_hypothesis_id: None,
                rationale,
                evidence_references,
                confidence,
            })
        }
        "reject" => {
            let target_id = first_rejected_hypothesis_id(response, hypotheses)?;
            Ok(ConflictResolution {
                decision: PolicyDecision::Reject,
                target_hypothesis_id: target_id,
                superseding_hypothesis_id: None,
                rationale,
                evidence_references,
                confidence,
            })
        }
        "supersede" => {
            let accepted_id = parse_hypothesis_id(response, "accepted_hypothesis_id", hypotheses)?;
            let target_id = first_rejected_hypothesis_id(response, hypotheses)?;
            Ok(ConflictResolution {
                decision: PolicyDecision::Supersede,
                target_hypothesis_id: target_id,
                superseding_hypothesis_id: Some(accepted_id),
                rationale,
                evidence_references,
                confidence,
            })
        }
        other => Err(ConflictError::InvalidResponse(format!(
            "unknown resolution_kind: {other}"
        ))),
    }
}

fn parse_hypothesis_id(
    response: &Value,
    key: &str,
    hypotheses: &[Hypothesis],
) -> Result<HypothesisId, ConflictError> {
    let raw = response
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ConflictError::InvalidResponse(format!("missing {key}")))?;
    let id = raw
        .parse::<uuid::Uuid>()
        .map(HypothesisId::from_uuid)
        .map_err(|e| ConflictError::InvalidResponse(format!("invalid hypothesis id: {e}")))?;
    if !hypotheses.iter().any(|h| h.id == id) {
        return Err(ConflictError::HypothesisNotFound(id));
    }
    Ok(id)
}

fn first_rejected_hypothesis_id(
    response: &Value,
    hypotheses: &[Hypothesis],
) -> Result<HypothesisId, ConflictError> {
    let ids = response
        .get("rejected_hypothesis_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| ConflictError::InvalidResponse("missing rejected_hypothesis_ids".into()))?;
    let raw = ids
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| ConflictError::InvalidResponse("empty rejected_hypothesis_ids".into()))?;
    let id = raw
        .parse::<uuid::Uuid>()
        .map(HypothesisId::from_uuid)
        .map_err(|e| {
            ConflictError::InvalidResponse(format!("invalid rejected hypothesis id: {e}"))
        })?;
    if !hypotheses.iter().any(|h| h.id == id) {
        return Err(ConflictError::HypothesisNotFound(id));
    }
    Ok(id)
}

/// Emits the policy-driven commands implied by a conflict resolution.
fn emit_commands(
    project: ProjectId,
    subject: EntityId,
    resolution: &ConflictResolution,
    mappings: &[GeneratedSourceMapping],
) -> Vec<ApplicationCommand> {
    let mut commands = Vec::new();

    commands.push(ApplicationCommand::AcceptHypothesisPolicyDriven(
        AcceptHypothesisPolicyDrivenRequest {
            project,
            hypothesis_id: resolution.target_hypothesis_id,
            policy_decision: resolution.decision,
            justification: resolution.rationale.clone(),
            superseding_hypothesis_id: resolution.superseding_hypothesis_id,
        },
    ));

    if resolution.decision == PolicyDecision::Supersede {
        for mapping in mappings.iter().filter(|m| m.target_entity == subject) {
            let mapping_id = mapping.id.to_string();
            commands.push(ApplicationCommand::InvalidateGeneratedSource(
                InvalidateGeneratedSourceRequest {
                    project,
                    mapping_id,
                },
            ));
        }
    }

    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use autore_schema::domain::records::HypothesisStatus;
    use autore_schema::domain::{Confidence, EvidenceValue, NamespacedId};

    struct MockConflictLlm {
        response: Value,
    }

    impl ConflictLlm for MockConflictLlm {
        fn analyze_conflict(&self, _bundle: &InvestigationBundle) -> Result<Value, ConflictError> {
            Ok(self.response.clone())
        }
    }

    fn project() -> ProjectId {
        ProjectId::new()
    }

    fn entity() -> EntityId {
        EntityId::new()
    }

    fn hypothesis(id: HypothesisId, subject: EntityId, predicate: &str) -> Hypothesis {
        Hypothesis {
            id,
            project: project(),
            subject,
            predicate: NamespacedId::parse(predicate).unwrap(),
            candidate: EvidenceValue::String("{}".into()),
            supporting_evidence: vec![],
            contradicting_evidence: vec![],
            derived_from: vec![],
            confidence: Confidence::new(0.5).unwrap(),
            status: HypothesisStatus::Proposed,
            created_at: autore_schema::domain::Timestamp::now(),
            updated_at: autore_schema::domain::Timestamp::now(),
        }
    }

    fn mapping(target: EntityId) -> GeneratedSourceMapping {
        GeneratedSourceMapping {
            id: autore_schema::ids::GeneratedSourceMappingId::new(),
            campaign: autore_schema::ids::ReconstructionCampaignId::new(),
            generated_artifact: autore_schema::ids::ArtifactId::new(),
            target_entity: target,
            produced_by: WorkItemId::new(),
            mapping_kind: NamespacedId::parse("mapping.type").unwrap(),
            created_at: autore_schema::domain::Timestamp::now(),
        }
    }

    #[test]
    fn llm_arbitration_produces_accept_command() {
        let subject = entity();
        let accepted = HypothesisId::new();
        let rejected = HypothesisId::new();
        let hypotheses = vec![
            hypothesis(accepted, subject, "llm.analysis.conflict.accepted"),
            hypothesis(rejected, subject, "llm.analysis.conflict.rejected"),
        ];
        let response = serde_json::json!({
            "resolution_kind": "accept",
            "accepted_hypothesis_id": accepted.to_string(),
            "rejected_hypothesis_ids": [rejected.to_string()],
            "rationale": "layout A matches allocation evidence",
            "evidence_references": ["ev-1"],
            "confidence": 0.85
        });
        let llm = MockConflictLlm { response };
        let arbitrator = ConflictArbitrator::new();
        let commands = arbitrator
            .arbitrate(project(), subject, &hypotheses, &[], &[], &[], &llm)
            .unwrap();

        assert_eq!(commands.len(), 1);
        let cmd = &commands[0];
        let ApplicationCommand::AcceptHypothesisPolicyDriven(req) = cmd else {
            panic!("expected AcceptHypothesisPolicyDriven, got {cmd:?}");
        };
        assert_eq!(req.hypothesis_id, accepted);
        assert_eq!(req.policy_decision, PolicyDecision::Accept);
        assert!(req.superseding_hypothesis_id.is_none());
        assert_eq!(req.justification, "layout A matches allocation evidence");
    }

    #[test]
    fn llm_arbitration_produces_reject_command() {
        let subject = entity();
        let accepted = HypothesisId::new();
        let rejected = HypothesisId::new();
        let hypotheses = vec![
            hypothesis(accepted, subject, "llm.analysis.conflict.accepted"),
            hypothesis(rejected, subject, "llm.analysis.conflict.rejected"),
        ];
        let response = serde_json::json!({
            "resolution_kind": "reject",
            "accepted_hypothesis_id": accepted.to_string(),
            "rejected_hypothesis_ids": [rejected.to_string()],
            "rationale": "layout B contradicts field offset evidence",
            "evidence_references": ["ev-2"],
            "confidence": 0.7
        });
        let llm = MockConflictLlm { response };
        let arbitrator = ConflictArbitrator::new();
        let commands = arbitrator
            .arbitrate(project(), subject, &hypotheses, &[], &[], &[], &llm)
            .unwrap();

        assert_eq!(commands.len(), 1);
        let ApplicationCommand::AcceptHypothesisPolicyDriven(req) = &commands[0] else {
            panic!(
                "expected AcceptHypothesisPolicyDriven, got {:?}",
                commands[0]
            );
        };
        assert_eq!(req.hypothesis_id, rejected);
        assert_eq!(req.policy_decision, PolicyDecision::Reject);
        assert!(req.superseding_hypothesis_id.is_none());
    }

    #[test]
    fn llm_arbitration_produces_supersede_command() {
        let subject = entity();
        let accepted = HypothesisId::new();
        let superseded = HypothesisId::new();
        let hypotheses = vec![
            hypothesis(accepted, subject, "llm.analysis.conflict.accepted"),
            hypothesis(superseded, subject, "llm.analysis.conflict.superseded"),
        ];
        let response = serde_json::json!({
            "resolution_kind": "supersede",
            "accepted_hypothesis_id": accepted.to_string(),
            "rejected_hypothesis_ids": [superseded.to_string()],
            "rationale": "layout A refines layout B with additional fields",
            "evidence_references": ["ev-3"],
            "confidence": 0.9
        });
        let llm = MockConflictLlm { response };
        let arbitrator = ConflictArbitrator::new();
        let commands = arbitrator
            .arbitrate(project(), subject, &hypotheses, &[], &[], &[], &llm)
            .unwrap();

        assert_eq!(commands.len(), 1);
        let ApplicationCommand::AcceptHypothesisPolicyDriven(req) = &commands[0] else {
            panic!(
                "expected AcceptHypothesisPolicyDriven, got {:?}",
                commands[0]
            );
        };
        assert_eq!(req.hypothesis_id, superseded);
        assert_eq!(req.policy_decision, PolicyDecision::Supersede);
        assert_eq!(req.superseding_hypothesis_id, Some(accepted));
    }

    #[test]
    fn supersede_invalidates_affected_generated_sources() {
        let subject = entity();
        let accepted = HypothesisId::new();
        let superseded = HypothesisId::new();
        let hypotheses = vec![
            hypothesis(accepted, subject, "llm.analysis.conflict.accepted"),
            hypothesis(superseded, subject, "llm.analysis.conflict.superseded"),
        ];
        let affected = mapping(subject);
        let unaffected = mapping(EntityId::new());
        let mappings = vec![affected.clone(), unaffected];

        let response = serde_json::json!({
            "resolution_kind": "supersede",
            "accepted_hypothesis_id": accepted.to_string(),
            "rejected_hypothesis_ids": [superseded.to_string()],
            "rationale": "layout A refines layout B",
            "evidence_references": ["ev-4"],
            "confidence": 0.9
        });
        let llm = MockConflictLlm { response };
        let arbitrator = ConflictArbitrator::new();
        let commands = arbitrator
            .arbitrate(project(), subject, &hypotheses, &[], &[], &mappings, &llm)
            .unwrap();

        assert_eq!(commands.len(), 2);
        let ApplicationCommand::AcceptHypothesisPolicyDriven(req) = &commands[0] else {
            panic!(
                "expected AcceptHypothesisPolicyDriven, got {:?}",
                commands[0]
            );
        };
        assert_eq!(req.hypothesis_id, superseded);
        assert_eq!(req.policy_decision, PolicyDecision::Supersede);

        let ApplicationCommand::InvalidateGeneratedSource(inv) = &commands[1] else {
            panic!("expected InvalidateGeneratedSource, got {:?}", commands[1]);
        };
        assert_eq!(inv.mapping_id, affected.id.to_string());
    }
}
