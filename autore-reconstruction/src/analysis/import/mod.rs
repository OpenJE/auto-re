//! `autore-reconstruction::analysis::import` — 3-level LLM import boundary.
//!
//! Level 1: persist the raw response artifact unconditionally.
//! Level 2: validate the parsed result against the capability's JSON Schema
//!          and the spec §8.6 invariants.
//! Level 3: on success, convert validated claims into canonical
//!          `AddHypothesis` commands; on failure, either request repair
//!          (first attempt) or fail the work item (second attempt).

mod repair;
mod validate;

#[cfg(test)]
mod tests;

use autore_app::application_service::requests::{
    AddEvidenceRequest, AddEvidenceResponse, AddHypothesisRequest, BlockWorkWithReasonRequest,
    FailWorkItemRequest,
};
use autore_app::{ApplicationCommand, AutoReClient, CommandResult};
use autore_schema::domain::records::{EvidenceRecord, HypothesisStatus};
use autore_schema::domain::values::{DerivationMethod, EvidenceValue};
use autore_schema::domain::{Derivation, NamespacedId, Timestamp};
use autore_schema::ids::{ArtifactId, EvidenceRecordId, ProjectId};
use serde_json::Value;

use crate::analysis::bundle::InvestigationBundle;
use crate::analysis::import::repair::build_repair_prompt;
use crate::analysis::import::validate::ImportValidation;
use crate::analysis::schemas::response_schema_for;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Outcome of a single import attempt.
#[derive(Debug, Clone)]
pub enum LlmImportResult {
    /// Validation passed; canonical hypotheses and follow-up work commands
    /// have been issued through the client.
    Success {
        hypotheses: Vec<ApplicationCommand>,
        follow_up_work: Vec<ApplicationCommand>,
    },
    /// The parsed output was invalid and the retry budget is exhausted.
    /// `FailWorkItem` and `BlockWorkWithReason` commands have been issued.
    InvalidOutput { reason: String },
    /// The parsed output was invalid but the caller may retry with the
    /// returned repair prompt.
    RepairRequested { repair_prompt: String },
}

/// Errors that prevent the importer from running at all (as opposed to
/// validation failures, which are reported through `LlmImportResult`).
#[derive(Debug, thiserror::Error)]
pub enum LlmImportError {
    #[error("unknown capability: {0}")]
    UnknownCapability(String),
    #[error("client error: {0}")]
    Client(#[from] autore_core::Error),
    #[error("serialization: {0}")]
    Serialization(String),
}

// ---------------------------------------------------------------------------
// LlmImporter
// ---------------------------------------------------------------------------

/// Imports a parsed LLM response into the canonical event store through
/// `ApplicationCommand` variants.
pub struct LlmImporter<'a> {
    bundle: &'a InvestigationBundle,
    capability_id: String,
    _raw_response_artifact_id: ArtifactId,
    /// Handle for the parsed result artifact (used by consumers to look up
    /// the stored parsed JSON). Not directly embedded in evidence records.
    _parsed_response_artifact_id: ArtifactId,
    attempt_count: u32,
    client: &'a dyn AutoReClient,
    project_id: ProjectId,
    raw_response_text: String,
    parsed_response: Value,
}

impl<'a> LlmImporter<'a> {
    /// Constructs a new importer.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bundle: &'a InvestigationBundle,
        capability_id: &str,
        raw_response_artifact_id: ArtifactId,
        parsed_response_artifact_id: ArtifactId,
        attempt_count: u32,
        client: &'a dyn AutoReClient,
        project_id: ProjectId,
        raw_response_text: String,
        parsed_response: Value,
    ) -> Self {
        Self {
            bundle,
            capability_id: capability_id.to_owned(),
            _raw_response_artifact_id: raw_response_artifact_id,
            _parsed_response_artifact_id: parsed_response_artifact_id,
            attempt_count,
            client,
            project_id,
            raw_response_text,
            parsed_response,
        }
    }

    /// Runs the 3-level import boundary.
    pub fn import(self) -> Result<LlmImportResult, LlmImportError> {
        // Level 1: always persist the raw response.
        let evidence_id = self.persist_raw_response()?;

        // Level 2: validate parsed response.
        let validation =
            ImportValidation::validate(&self.capability_id, &self.parsed_response, self.bundle);

        if validation.is_valid() {
            // Level 3a: convert to canonical hypotheses.
            let (hypotheses, follow_up_work) = self.build_canonical_commands(evidence_id)?;
            for cmd in hypotheses.iter().chain(follow_up_work.iter()) {
                self.client.execute(cmd.clone())?;
            }
            Ok(LlmImportResult::Success {
                hypotheses,
                follow_up_work,
            })
        } else if self.attempt_count == 0 {
            // First failure: offer repair.
            let schema = response_schema_for(&self.capability_id).unwrap_or(Value::Null);
            let bundle_json = serde_json::to_string_pretty(self.bundle).unwrap_or_default();
            let repair_prompt = build_repair_prompt(
                &self.capability_id,
                &validation.errors,
                &serde_json::to_string_pretty(&schema).unwrap_or_default(),
                &bundle_json,
            );
            Ok(LlmImportResult::RepairRequested { repair_prompt })
        } else {
            // Second+ failure: fail the work item.
            let reason = validation.errors.join("; ");
            self.issue_failure(&reason)?;
            Ok(LlmImportResult::InvalidOutput { reason })
        }
    }

    // -- private helpers --------------------------------------------------

    fn persist_raw_response(&self) -> Result<EvidenceRecordId, LlmImportError> {
        let predicate = NamespacedId::parse("llm.raw-response").expect("valid namespaced id");
        let derivation = Derivation::new(
            DerivationMethod::LlmInference,
            NamespacedId::parse(&self.capability_id)
                .unwrap_or_else(|_| NamespacedId::parse("llm.unknown").expect("fallback")),
            vec![],
            vec![],
        );
        let subject = self.bundle.subject_entity_id.unwrap_or_default();
        let record = EvidenceRecord {
            id: EvidenceRecordId::new(),
            project: self.project_id,
            subject,
            predicate,
            value: EvidenceValue::String(self.raw_response_text.clone()),
            derivation,
            provider_run: None,
            native_artifacts: vec![],
            assumptions: vec![],
            created_at: Timestamp::now(),
        };
        let result = self
            .client
            .execute(ApplicationCommand::AddEvidence(AddEvidenceRequest {
                project: self.project_id,
                record,
            }))?;
        match result {
            CommandResult::EvidenceAdded(AddEvidenceResponse { id }) => Ok(id),
            _ => Err(LlmImportError::Serialization(
                "unexpected command result from AddEvidence".into(),
            )),
        }
    }

    fn build_canonical_commands(
        &self,
        evidence_id: EvidenceRecordId,
    ) -> Result<(Vec<ApplicationCommand>, Vec<ApplicationCommand>), LlmImportError> {
        let subject = self.bundle.subject_entity_id.unwrap_or_default();
        let confidence = self
            .parsed_response
            .get("confidence")
            .and_then(Value::as_f64)
            .unwrap_or(0.5);

        let mut hypotheses = Vec::new();
        let mut follow_up_work = Vec::new();

        let mk = |predicate_suffix: &str, text: String| -> ApplicationCommand {
            ApplicationCommand::AddHypothesis(AddHypothesisRequest {
                project: self.project_id,
                subject,
                predicate: format!("{}.{predicate_suffix}", self.capability_id),
                candidate: EvidenceValue::String(text),
                confidence_score: confidence,
                confidence_rationale: None,
                supporting_evidence: vec![evidence_id],
                contradicting_evidence: vec![],
                derived_from: vec![],
                status: HypothesisStatus::Proposed,
            })
        };

        Self::collect_strings(&self.parsed_response, "behavior_claims", |text| {
            hypotheses.push(mk("behavior-claim", text));
        });
        Self::collect_strings(&self.parsed_response, "side_effects", |text| {
            hypotheses.push(mk("side-effect-claim", text));
        });
        if let Some(name) = self
            .parsed_response
            .get("proposed_name")
            .and_then(Value::as_str)
        {
            hypotheses.push(mk("function-name", name.to_owned()));
        }
        if let Some(layout) = self
            .parsed_response
            .get("proposed_layout")
            .and_then(Value::as_array)
        {
            let layout_json = serde_json::to_string(layout)
                .map_err(|e| LlmImportError::Serialization(e.to_string()))?;
            hypotheses.push(mk("type-proposal", layout_json));
        }
        if let Some(stmt) = self
            .parsed_response
            .get("hypothesis_statement")
            .and_then(Value::as_str)
        {
            hypotheses.push(mk("experiment-hypothesis", stmt.to_owned()));
        }
        Self::collect_strings(
            &self.parsed_response,
            "recommended_follow_up_work",
            |text| {
                follow_up_work.push(mk("follow-up-work", text));
            },
        );

        Ok((hypotheses, follow_up_work))
    }

    fn collect_strings(value: &Value, key: &str, mut f: impl FnMut(String)) {
        if let Some(items) = value.get(key).and_then(Value::as_array) {
            for item in items {
                if let Some(text) = item.as_str() {
                    f(text.to_owned());
                }
            }
        }
    }

    fn issue_failure(&self, reason: &str) -> Result<(), LlmImportError> {
        let work_item_id = self.bundle.subject_identity.to_string();
        self.client
            .execute(ApplicationCommand::FailWorkItem(FailWorkItemRequest {
                project: self.project_id,
                work_item_id: work_item_id.clone(),
                reason: reason.to_owned(),
            }))?;
        self.client
            .execute(ApplicationCommand::BlockWorkWithReason(
                BlockWorkWithReasonRequest {
                    project: self.project_id,
                    reason: format!("InvalidOutput: {reason}"),
                },
            ))?;
        Ok(())
    }
}
