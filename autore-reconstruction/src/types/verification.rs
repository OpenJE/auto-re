//! Per-field verification tracking for canonical type/class hypotheses.
//!
//! A `CanonicalTypeHypothesis` (from `autore_schema`) is not fully verified
//! just because its size is known. Verification is split across individual
//! layout aspects per spec §10.4; this module implements the store that marks
//! those aspects and updates confidence deterministically.

use std::collections::BTreeMap;

use autore_app::application_service::requests::{AddVerificationRequest, AddVerificationResponse};
use autore_app::{ApplicationCommand, AutoReClient, CommandResult};
use autore_core::{Error, Result};
use autore_schema::domain::records::{
    CanonicalTypeHypothesis, VERIFICATION_CHECK_ALIGNMENT, VERIFICATION_CHECK_CALLING_CONVENTION,
    VERIFICATION_CHECK_FIELD_INTERPRETATION, VERIFICATION_CHECK_FIELD_OFFSET,
    VERIFICATION_CHECK_INHERITANCE, VERIFICATION_CHECK_SIZE, VERIFICATION_CHECK_VTABLE_SLOT,
    VerificationRecord, VerificationState, VerificationSubject,
};
use autore_schema::domain::{ExtensionData, NamespacedId};
use autore_schema::ids::{EntityId, ProjectId};

use super::reconciler::ReconciledLayout;

/// In-memory canonical type hypothesis store.
///
/// Holds the set of hypotheses for a project and issues `AddVerification`
/// commands when a field is marked verified.
#[derive(Debug, Clone, Default)]
pub struct CanonicalTypeStore {
    project: ProjectId,
    hypotheses: BTreeMap<EntityId, CanonicalTypeHypothesis>,
}

impl CanonicalTypeStore {
    /// Creates an empty store for the given project.
    pub fn new(project: ProjectId) -> Self {
        Self {
            project,
            hypotheses: BTreeMap::new(),
        }
    }

    /// Creates a store seeded with existing hypotheses.
    pub fn with_hypotheses(
        project: ProjectId,
        hypotheses: impl IntoIterator<Item = CanonicalTypeHypothesis>,
    ) -> Self {
        let mut map = BTreeMap::new();
        for h in hypotheses {
            map.insert(h.entity_id, h);
        }
        Self {
            project,
            hypotheses: map,
        }
    }

    /// Returns a reference to the stored hypothesis for `entity_id`, if any.
    pub fn get(&self, entity_id: EntityId) -> Option<&CanonicalTypeHypothesis> {
        self.hypotheses.get(&entity_id)
    }

    /// Inserts or replaces a hypothesis in the store.
    pub fn insert(&mut self, hypothesis: CanonicalTypeHypothesis) {
        self.hypotheses.insert(hypothesis.entity_id, hypothesis);
    }

    /// Marks a verification field on `hypothesis` as verified, recalculates
    /// confidence, and issues an `AddVerification` command through `client`.
    ///
    /// Returns the ID from the command response on success.
    ///
    /// `InheritanceRelation` can only be marked when the base entity's
    /// hypothesis is already fully verified.
    pub fn mark_verified(
        &self,
        client: &dyn AutoReClient,
        hypothesis: &mut CanonicalTypeHypothesis,
        field: &VerificationField,
    ) -> Result<AddVerificationResponse> {
        let applicable = applicable_verification_fields(hypothesis)?;
        if !applicable.contains(field) {
            return Err(Error::Validation(format!(
                "{field:?} is not applicable to hypothesis {}",
                hypothesis.entity_id
            )));
        }

        if let VerificationField::InheritanceRelation(base_entity) = field {
            let base_verified = self
                .get(*base_entity)
                .map(|h| {
                    is_fully_verified(h, &applicable_verification_fields(h).unwrap_or_default())
                })
                .unwrap_or(false);
            if !base_verified {
                return Err(Error::Validation(format!(
                    "cannot verify inheritance relation for {}: base entity {base_entity} is not fully verified",
                    hypothesis.entity_id
                )));
            }
        }

        set_verified_flag(hypothesis, field);
        hypothesis.confidence = compute_confidence(hypothesis, &applicable);
        hypothesis.touch();

        let record = build_verification_record(self.project, hypothesis, field)?;
        let command = ApplicationCommand::AddVerification(AddVerificationRequest {
            project: self.project,
            record,
        });
        match client.execute(command)? {
            CommandResult::VerificationAdded(resp) => Ok(resp),
            other => Err(Error::Validation(format!(
                "AddVerification returned unexpected result: {other:?}"
            ))),
        }
    }
}

/// One independently-verifiable aspect of a canonical type hypothesis.
///
/// Re-exported from `autore_schema` for convenience.
pub use autore_schema::domain::records::VerificationField;

/// Returns the list of verification fields that apply to `hypothesis` based
/// on its parsed layout.
///
/// The list is deterministic and derived from the JSON layout payload.
pub fn applicable_verification_fields(
    hypothesis: &CanonicalTypeHypothesis,
) -> Result<Vec<VerificationField>> {
    let layout: ReconciledLayout = if hypothesis.layout_json.is_empty() {
        ReconciledLayout {
            entity_id: hypothesis.entity_id,
            computed_size_bytes: None,
            computed_alignment: None,
            fields: vec![],
            vtable_slot_targets: vec![],
            base_adjustments: vec![],
            array_stride: None,
            parameter_usages: vec![],
            return_value_use: None,
            source_constraints: vec![],
        }
    } else {
        serde_json::from_str(&hypothesis.layout_json)
            .map_err(|e| Error::Serialization(format!("invalid layout_json: {e}")))?
    };

    let mut fields = Vec::new();
    if layout.computed_size_bytes.is_some() {
        fields.push(VerificationField::Size);
    }
    if layout.computed_alignment.is_some() {
        fields.push(VerificationField::Alignment);
    }
    for f in &layout.fields {
        let key = f.offset.to_string();
        fields.push(VerificationField::IndividualFieldOffset(key.clone()));
        fields.push(VerificationField::FieldInterpretation(key));
    }
    for adj in &layout.base_adjustments {
        fields.push(VerificationField::InheritanceRelation(adj.base_entity));
    }
    for slot in &layout.vtable_slot_targets {
        fields.push(VerificationField::VtableSlot(slot.slot_idx));
    }
    if !layout.parameter_usages.is_empty() || layout.return_value_use.is_some() {
        fields.push(VerificationField::CallingConvention);
    }
    Ok(fields)
}

/// Returns `true` only if every applicable verification field on `hypothesis`
/// is marked verified.
///
/// A class is NOT fully verified only because its size is known.
pub fn is_fully_verified(
    hypothesis: &CanonicalTypeHypothesis,
    applicable: &[VerificationField],
) -> bool {
    if applicable.is_empty() {
        return false;
    }
    applicable
        .iter()
        .all(|field| is_field_verified(hypothesis, field))
}

/// Returns the current confidence of `hypothesis` as a simple average of
/// applicable fields that are verified.
pub fn compute_confidence(
    hypothesis: &CanonicalTypeHypothesis,
    applicable: &[VerificationField],
) -> f64 {
    if applicable.is_empty() {
        return 0.0;
    }
    let verified = applicable
        .iter()
        .filter(|f| is_field_verified(hypothesis, f))
        .count();
    verified as f64 / applicable.len() as f64
}

fn set_verified_flag(hypothesis: &mut CanonicalTypeHypothesis, field: &VerificationField) {
    match field {
        VerificationField::Size => hypothesis.verified_size = true,
        VerificationField::Alignment => hypothesis.verified_alignment = true,
        VerificationField::IndividualFieldOffset(key) => {
            hypothesis.verified_field_offsets.insert(key.clone(), true);
        }
        VerificationField::FieldInterpretation(key) => {
            hypothesis
                .verified_field_interpretations
                .insert(key.clone(), true);
        }
        VerificationField::InheritanceRelation(base) => {
            hypothesis
                .verified_inheritance_relations
                .insert(*base, true);
        }
        VerificationField::VtableSlot(idx) => {
            hypothesis.verified_vtable_slots.insert(*idx, true);
        }
        VerificationField::CallingConvention => hypothesis.verified_calling_convention = true,
    }
}

fn is_field_verified(hypothesis: &CanonicalTypeHypothesis, field: &VerificationField) -> bool {
    match field {
        VerificationField::Size => hypothesis.verified_size,
        VerificationField::Alignment => hypothesis.verified_alignment,
        VerificationField::IndividualFieldOffset(key) => hypothesis
            .verified_field_offsets
            .get(key)
            .copied()
            .unwrap_or(false),
        VerificationField::FieldInterpretation(key) => hypothesis
            .verified_field_interpretations
            .get(key)
            .copied()
            .unwrap_or(false),
        VerificationField::InheritanceRelation(base) => hypothesis
            .verified_inheritance_relations
            .get(base)
            .copied()
            .unwrap_or(false),
        VerificationField::VtableSlot(idx) => hypothesis
            .verified_vtable_slots
            .get(idx)
            .copied()
            .unwrap_or(false),
        VerificationField::CallingConvention => hypothesis.verified_calling_convention,
    }
}

fn check_for_field(field: &VerificationField) -> Result<NamespacedId> {
    match field {
        VerificationField::Size => Ok(VERIFICATION_CHECK_SIZE.clone()),
        VerificationField::Alignment => Ok(VERIFICATION_CHECK_ALIGNMENT.clone()),
        VerificationField::IndividualFieldOffset(_) => Ok(VERIFICATION_CHECK_FIELD_OFFSET.clone()),
        VerificationField::FieldInterpretation(_) => {
            Ok(VERIFICATION_CHECK_FIELD_INTERPRETATION.clone())
        }
        VerificationField::InheritanceRelation(_) => Ok(VERIFICATION_CHECK_INHERITANCE.clone()),
        VerificationField::VtableSlot(_) => Ok(VERIFICATION_CHECK_VTABLE_SLOT.clone()),
        VerificationField::CallingConvention => Ok(VERIFICATION_CHECK_CALLING_CONVENTION.clone()),
    }
}

fn build_verification_record(
    project: ProjectId,
    hypothesis: &CanonicalTypeHypothesis,
    field: &VerificationField,
) -> Result<VerificationRecord> {
    let check = check_for_field(field)?;
    let schema = NamespacedId::parse("verification.canonical-type.details")
        .map_err(|e| Error::Validation(e.0))?;
    let details = ExtensionData::new(
        schema,
        1,
        serde_json::json!({
            "entity_id": hypothesis.entity_id.to_string(),
            "field": field,
            "verified": true,
            "confidence": hypothesis.confidence,
        }),
    );
    Ok(VerificationRecord {
        id: autore_schema::ids::VerificationRecordId::new(),
        project,
        subject: VerificationSubject::Entity(hypothesis.entity_id),
        check,
        state: VerificationState::Passed,
        provider_run: None,
        evidence: vec![],
        details: Some(details),
        created_at: autore_schema::domain::Timestamp::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::RecordingAutoReClient;
    use autore_app::ApplicationCommand;
    use autore_schema::ids::{EntityId, ProjectId};

    fn layout_with_field(entity: EntityId, size: usize, offset: usize) -> String {
        let layout = ReconciledLayout {
            entity_id: entity,
            computed_size_bytes: Some(size),
            computed_alignment: None,
            fields: vec![super::super::reconciler::ReconciledField {
                offset,
                width_bytes: Some(4),
            }],
            vtable_slot_targets: vec![],
            base_adjustments: vec![],
            array_stride: None,
            parameter_usages: vec![],
            return_value_use: None,
            source_constraints: vec![],
        };
        serde_json::to_string(&layout).unwrap()
    }

    fn layout_with_inheritance(entity: EntityId, base: EntityId) -> String {
        let layout = ReconciledLayout {
            entity_id: entity,
            computed_size_bytes: Some(16),
            computed_alignment: None,
            fields: vec![],
            vtable_slot_targets: vec![],
            base_adjustments: vec![super::super::reconciler::ReconciledBaseAdjustment {
                base_entity: base,
                offset: 0,
            }],
            array_stride: None,
            parameter_usages: vec![],
            return_value_use: None,
            source_constraints: vec![],
        };
        serde_json::to_string(&layout).unwrap()
    }

    fn make_hypothesis(
        project: ProjectId,
        entity: EntityId,
        layout: String,
    ) -> CanonicalTypeHypothesis {
        CanonicalTypeHypothesis::new(project, entity, layout)
    }

    #[test]
    fn size_verified_does_not_implicitly_verify_offset() {
        let client = RecordingAutoReClient::new();
        let project = ProjectId::new();
        let entity = EntityId::new();
        let store = CanonicalTypeStore::new(project);
        let mut hypothesis = make_hypothesis(project, entity, layout_with_field(entity, 32, 8));

        store
            .mark_verified(&client, &mut hypothesis, &VerificationField::Size)
            .expect("mark size verified");

        assert!(hypothesis.verified_size);
        assert!(
            !hypothesis
                .verified_field_offsets
                .get("8")
                .copied()
                .unwrap_or(false)
        );
        assert!(!is_fully_verified(
            &hypothesis,
            &applicable_verification_fields(&hypothesis).unwrap()
        ));

        let commands = client.commands();
        let verifications: Vec<_> = commands
            .iter()
            .filter(|c| matches!(c, ApplicationCommand::AddVerification(_)))
            .collect();
        assert_eq!(verifications.len(), 1);
    }

    #[test]
    fn offset_verified_does_not_implicitly_verify_field_interpretation() {
        let client = RecordingAutoReClient::new();
        let project = ProjectId::new();
        let entity = EntityId::new();
        let store = CanonicalTypeStore::new(project);
        let mut hypothesis = make_hypothesis(project, entity, layout_with_field(entity, 32, 8));

        store
            .mark_verified(
                &client,
                &mut hypothesis,
                &VerificationField::IndividualFieldOffset("8".into()),
            )
            .expect("mark offset verified");

        assert!(
            hypothesis
                .verified_field_offsets
                .get("8")
                .copied()
                .unwrap_or(false)
        );
        assert!(
            !hypothesis
                .verified_field_interpretations
                .get("8")
                .copied()
                .unwrap_or(false)
        );
        assert!(!is_fully_verified(
            &hypothesis,
            &applicable_verification_fields(&hypothesis).unwrap()
        ));
    }

    #[test]
    fn inheritance_relation_requires_base_verified() {
        let client = RecordingAutoReClient::new();
        let project = ProjectId::new();
        let entity = EntityId::new();
        let base = EntityId::new();
        let mut hypothesis =
            make_hypothesis(project, entity, layout_with_inheritance(entity, base));

        // No base hypothesis in the store -> should fail.
        let store = CanonicalTypeStore::new(project);
        let result = store.mark_verified(
            &client,
            &mut hypothesis,
            &VerificationField::InheritanceRelation(base),
        );
        assert!(
            result.is_err(),
            "inheritance relation must require base verified"
        );
        assert!(
            !hypothesis
                .verified_inheritance_relations
                .get(&base)
                .copied()
                .unwrap_or(false)
        );

        // Base hypothesis fully verified -> should succeed.
        let mut base_hypothesis = make_hypothesis(project, base, layout_with_field(base, 8, 0));
        let store = CanonicalTypeStore::with_hypotheses(project, [base_hypothesis.clone()]);
        store
            .mark_verified(&client, &mut base_hypothesis, &VerificationField::Size)
            .unwrap();
        store
            .mark_verified(
                &client,
                &mut base_hypothesis,
                &VerificationField::IndividualFieldOffset("0".into()),
            )
            .unwrap();
        store
            .mark_verified(
                &client,
                &mut base_hypothesis,
                &VerificationField::FieldInterpretation("0".into()),
            )
            .unwrap();
        assert!(is_fully_verified(
            &base_hypothesis,
            &applicable_verification_fields(&base_hypothesis).unwrap()
        ));

        let store = CanonicalTypeStore::with_hypotheses(project, [base_hypothesis]);
        let result = store.mark_verified(
            &client,
            &mut hypothesis,
            &VerificationField::InheritanceRelation(base),
        );
        assert!(
            result.is_ok(),
            "inheritance relation should succeed once base is verified"
        );
        assert!(
            hypothesis
                .verified_inheritance_relations
                .get(&base)
                .copied()
                .unwrap_or(false)
        );
    }

    #[test]
    fn confidence_is_average_of_applicable_fields() {
        let project = ProjectId::new();
        let entity = EntityId::new();
        let hypothesis = make_hypothesis(project, entity, layout_with_field(entity, 32, 8));
        let applicable = applicable_verification_fields(&hypothesis).unwrap();
        // size + field_offset + field_interpretation = 3 fields
        assert_eq!(applicable.len(), 3);
        assert_eq!(compute_confidence(&hypothesis, &applicable), 0.0);
    }

    #[test]
    fn fully_verified_requires_all_applicable_fields() {
        let client = RecordingAutoReClient::new();
        let project = ProjectId::new();
        let entity = EntityId::new();
        let store = CanonicalTypeStore::new(project);
        let mut hypothesis = make_hypothesis(project, entity, layout_with_field(entity, 32, 8));

        store
            .mark_verified(&client, &mut hypothesis, &VerificationField::Size)
            .unwrap();
        store
            .mark_verified(
                &client,
                &mut hypothesis,
                &VerificationField::IndividualFieldOffset("8".into()),
            )
            .unwrap();
        store
            .mark_verified(
                &client,
                &mut hypothesis,
                &VerificationField::FieldInterpretation("8".into()),
            )
            .unwrap();

        assert!(is_fully_verified(
            &hypothesis,
            &applicable_verification_fields(&hypothesis).unwrap()
        ));
        assert!((hypothesis.confidence - 1.0).abs() < f64::EPSILON);
    }
}
