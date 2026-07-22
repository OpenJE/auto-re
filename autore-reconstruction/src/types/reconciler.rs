//! Deterministic layout reconciliation.
//!
//! The [`Reconciler`] groups layout constraints by their primary entity,
//! checks them for compatibility, and either proposes a single deterministic
//! layout hypothesis (confidence 1.0) or emits a conflict-resolution work
//! item. It never silently forks duplicate types.

use std::collections::{BTreeMap, HashMap};

use autore_app::application_service::requests::{AddHypothesisRequest, CreateWorkItemsRequest};
use autore_app::{ApplicationCommand, AutoReClient};
use autore_core::{Error, Result};
use autore_schema::domain::EvidenceValue;
use autore_schema::domain::records::HypothesisStatus;
use autore_schema::ids::{ArtifactId, EntityId, EvidenceRecordId, ProjectId};

use super::constraint::{LayoutConstraint, LayoutConstraintKind};

/// Predicate string used for deterministic layout hypotheses.
pub const LAYOUT_HYPOTHESIS_PREDICATE: &str = "proposes-deterministic-layout";

/// Prefix used in work-item descriptions to encode a conflict-resolution
/// intent, because `CreateWorkItemsRequest` has no dedicated `kind` field.
pub const CONFLICT_RESOLUTION_PREFIX: &str = "ConflictResolution:";

/// A field entry within a reconciled layout.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReconciledField {
    pub offset: usize,
    pub width_bytes: Option<usize>,
}

/// A virtual slot target within a reconciled vtable.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReconciledVtableSlot {
    pub slot_idx: usize,
    pub called: EntityId,
}

/// A base-class adjustment recorded for an entity.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReconciledBaseAdjustment {
    pub base_entity: EntityId,
    pub offset: usize,
}

/// A function parameter usage recorded in a reconciled layout.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReconciledParameterUsage {
    pub param_idx: usize,
    pub kind: String,
}

/// A return-value usage recorded in a reconciled layout.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReconciledReturnValueUse {
    pub kind: String,
}

/// The deterministic result of reconciling all constraints for one entity.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReconciledLayout {
    pub entity_id: EntityId,
    pub computed_size_bytes: Option<usize>,
    pub computed_alignment: Option<usize>,
    pub fields: Vec<ReconciledField>,
    pub vtable_slot_targets: Vec<ReconciledVtableSlot>,
    pub base_adjustments: Vec<ReconciledBaseAdjustment>,
    pub array_stride: Option<usize>,
    pub parameter_usages: Vec<ReconciledParameterUsage>,
    pub return_value_use: Option<ReconciledReturnValueUse>,
    pub source_constraints: Vec<ArtifactId>,
}

/// Errors that can occur during deterministic layout reconciliation.
#[derive(Debug, thiserror::Error)]
pub enum ReconcilerError {
    #[error("deterministic layout conflict for entity {entity}: {reason}")]
    Conflict { entity: EntityId, reason: String },
}

/// Deterministic layout reconciler.
///
/// Holds references to the [`AutoReClient`] and the project/campaign context
/// needed to issue canonical commands.
pub struct Reconciler<'a> {
    client: &'a dyn AutoReClient,
    project: ProjectId,
    campaign_id: String,
}

impl<'a> Reconciler<'a> {
    /// Creates a new reconciler for the given client and project context.
    pub fn new(client: &'a dyn AutoReClient, project: ProjectId, campaign_id: String) -> Self {
        Self {
            client,
            project,
            campaign_id,
        }
    }

    /// Reconciles the supplied constraints and returns the successfully
    /// reconciled layouts.
    ///
    /// For each entity whose constraints are compatible, exactly one
    /// `AddHypothesis` command is issued with confidence `1.0`. For each
    /// entity with conflicting constraints, a `CreateWorkItems` command is
    /// issued with a `ConflictResolution` description and no layout
    /// hypothesis is emitted for that entity.
    pub fn reconcile(&self, constraints: &[LayoutConstraint]) -> Result<Vec<ReconciledLayout>> {
        let mut by_entity: HashMap<EntityId, Vec<&LayoutConstraint>> = HashMap::new();
        for constraint in constraints {
            by_entity
                .entry(constraint.primary_entity())
                .or_default()
                .push(constraint);
        }

        // Deterministic iteration order for reproducible command issuance.
        let mut entities: Vec<EntityId> = by_entity.keys().copied().collect();
        entities.sort_by_key(|e| *e.as_uuid());

        let mut results = Vec::new();
        for entity in entities {
            let entity_constraints = by_entity.remove(&entity).expect("entity exists");
            match Self::reconcile_entity(entity, &entity_constraints) {
                Ok(layout) => {
                    self.issue_layout_hypothesis(&layout, &entity_constraints)?;
                    results.push(layout);
                }
                Err(ReconcilerError::Conflict { reason, .. }) => {
                    self.issue_conflict_work_item(entity, &reason)?;
                }
            }
        }

        Ok(results)
    }

    fn reconcile_entity(
        entity: EntityId,
        constraints: &[&LayoutConstraint],
    ) -> std::result::Result<ReconciledLayout, ReconcilerError> {
        let mut size: Option<usize> = None;
        let mut alignment: Option<usize> = None;
        let mut stride: Option<usize> = None;
        let mut fields: BTreeMap<usize, ReconciledField> = BTreeMap::new();
        let mut field_width_sources: BTreeMap<usize, (usize, Vec<ArtifactId>)> = BTreeMap::new();
        let mut vtable_slots: Vec<ReconciledVtableSlot> = Vec::new();
        let mut base_adjustments: Vec<ReconciledBaseAdjustment> = Vec::new();
        let mut parameter_usages: Vec<ReconciledParameterUsage> = Vec::new();
        let mut return_value_use: Option<ReconciledReturnValueUse> = None;
        let mut source_artifacts: Vec<ArtifactId> = Vec::new();

        let mut record_artifact = |artifact: Option<ArtifactId>| {
            if let Some(id) = artifact
                && !source_artifacts.contains(&id)
            {
                source_artifacts.push(id);
            }
        };

        for constraint in constraints {
            record_artifact(constraint.evidence_artifact_id);
            match &constraint.kind {
                LayoutConstraintKind::ObjectAllocationSize { size_bytes, .. } => {
                    if let Some(existing) = size
                        && existing != *size_bytes
                    {
                        return Err(ReconcilerError::Conflict {
                            entity,
                            reason: format!("conflicting object sizes: {existing} vs {size_bytes}"),
                        });
                    }
                    size = Some(*size_bytes);
                }
                LayoutConstraintKind::AlignmentRequirement { alignment: a, .. } => {
                    if let Some(existing) = alignment
                        && existing != *a
                    {
                        return Err(ReconcilerError::Conflict {
                            entity,
                            reason: format!(
                                "conflicting alignment requirements: {existing} vs {a}"
                            ),
                        });
                    }
                    alignment = Some(*a);
                }
                LayoutConstraintKind::ArrayStride { stride_bytes, .. } => {
                    if let Some(existing) = stride
                        && existing != *stride_bytes
                    {
                        return Err(ReconcilerError::Conflict {
                            entity,
                            reason: format!(
                                "conflicting array strides: {existing} vs {stride_bytes}"
                            ),
                        });
                    }
                    stride = Some(*stride_bytes);
                }
                LayoutConstraintKind::FieldObservedAtOffset { offset, .. } => {
                    fields.entry(*offset).or_insert(ReconciledField {
                        offset: *offset,
                        width_bytes: None,
                    });
                }
                LayoutConstraintKind::ReadWidth {
                    width_bytes,
                    offset,
                    ..
                }
                | LayoutConstraintKind::WriteWidth {
                    width_bytes,
                    offset,
                    ..
                } => {
                    let (existing_width, sources) = field_width_sources
                        .entry(*offset)
                        .or_insert((*width_bytes, Vec::new()));
                    if *existing_width != *width_bytes {
                        return Err(ReconcilerError::Conflict {
                            entity,
                            reason: format!(
                                "conflicting read/write widths at offset {offset}: {existing_width} vs {width_bytes}"
                            ),
                        });
                    }
                    if let Some(id) = constraint.evidence_artifact_id {
                        sources.push(id);
                    }
                    fields.entry(*offset).or_insert(ReconciledField {
                        offset: *offset,
                        width_bytes: Some(*width_bytes),
                    });
                }
                LayoutConstraintKind::VtablePointerLocation { offset, .. } => {
                    fields.entry(*offset).or_insert(ReconciledField {
                        offset: *offset,
                        width_bytes: None,
                    });
                }
                LayoutConstraintKind::VirtualSlotTarget {
                    slot_idx, called, ..
                } => {
                    vtable_slots.push(ReconciledVtableSlot {
                        slot_idx: *slot_idx,
                        called: *called,
                    });
                }
                LayoutConstraintKind::BaseAdjustment {
                    base_entity,
                    offset,
                    ..
                } => {
                    base_adjustments.push(ReconciledBaseAdjustment {
                        base_entity: *base_entity,
                        offset: *offset,
                    });
                }
                LayoutConstraintKind::FunctionParameterUsage {
                    param_idx, kind, ..
                } => {
                    parameter_usages.push(ReconciledParameterUsage {
                        param_idx: *param_idx,
                        kind: kind.clone(),
                    });
                }
                LayoutConstraintKind::ReturnValueUse { kind, .. } => {
                    if return_value_use.is_some() {
                        return Err(ReconcilerError::Conflict {
                            entity,
                            reason: "multiple return-value use constraints".into(),
                        });
                    }
                    return_value_use = Some(ReconciledReturnValueUse { kind: kind.clone() });
                }
            }
        }

        // Apply recorded widths to fields observed without widths.
        for (offset, field) in &mut fields {
            if field.width_bytes.is_none()
                && let Some((width, _)) = field_width_sources.get(offset)
            {
                field.width_bytes = Some(*width);
            }
        }

        // Validate that the object is large enough for all observed fields.
        if let Some(obj_size) = size {
            for field in fields.values() {
                let end = field.offset.saturating_add(field.width_bytes.unwrap_or(1));
                if end > obj_size {
                    return Err(ReconcilerError::Conflict {
                        entity,
                        reason: format!(
                            "field at offset {} with width {:?} extends past object size {obj_size}",
                            field.offset, field.width_bytes
                        ),
                    });
                }
            }
        }

        // Deterministic ordering for vector contents.
        let mut fields: Vec<ReconciledField> = fields.into_values().collect();
        fields.sort_by_key(|f| f.offset);
        vtable_slots.sort_by_key(|s| s.slot_idx);
        base_adjustments.sort_by(|a, b| {
            a.base_entity
                .cmp(&b.base_entity)
                .then(a.offset.cmp(&b.offset))
        });
        parameter_usages.sort_by_key(|p| p.param_idx);
        source_artifacts.sort_by_key(|a| *a.as_uuid());

        Ok(ReconciledLayout {
            entity_id: entity,
            computed_size_bytes: size,
            computed_alignment: alignment,
            fields,
            vtable_slot_targets: vtable_slots,
            base_adjustments,
            array_stride: stride,
            parameter_usages,
            return_value_use,
            source_constraints: source_artifacts,
        })
    }

    fn issue_layout_hypothesis(
        &self,
        layout: &ReconciledLayout,
        constraints: &[&LayoutConstraint],
    ) -> Result<()> {
        let candidate_json = serde_json::to_string(layout)
            .map_err(|e| Error::Serialization(format!("layout serialization: {e}")))?;
        let supporting_evidence: Vec<EvidenceRecordId> = constraints
            .iter()
            .filter_map(|c| c.evidence_artifact_id)
            .map(|_| EvidenceRecordId::new())
            .collect();
        let request = AddHypothesisRequest {
            project: self.project,
            subject: layout.entity_id,
            predicate: LAYOUT_HYPOTHESIS_PREDICATE.into(),
            candidate: EvidenceValue::String(candidate_json),
            confidence_score: 1.0,
            confidence_rationale: Some("deterministic layout reconciliation".into()),
            supporting_evidence,
            contradicting_evidence: vec![],
            derived_from: vec![],
            status: HypothesisStatus::Proposed,
        };
        self.client
            .execute(ApplicationCommand::AddHypothesis(request))
            .map(|_| ())
    }

    fn issue_conflict_work_item(&self, entity: EntityId, reason: &str) -> Result<()> {
        let description = format!(
            "{CONFLICT_RESOLUTION_PREFIX} deterministic layout conflict for entity {entity}: {reason}"
        );
        let request = CreateWorkItemsRequest {
            project: self.project,
            campaign_id: self.campaign_id.clone(),
            descriptions: vec![description],
        };
        self.client
            .execute(ApplicationCommand::CreateWorkItems(request))
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::RecordingAutoReClient;
    use autore_app::ApplicationCommand;

    fn make_reconciler<'a>(
        client: &'a RecordingAutoReClient,
        project: ProjectId,
    ) -> Reconciler<'a> {
        Reconciler::new(client, project, "campaign-1".into())
    }

    #[test]
    fn reconciler_produces_layout_hypothesis_for_compatible_constraints() {
        let client = RecordingAutoReClient::new();
        let project = ProjectId::new();
        let entity = EntityId::new();
        let reconciler = make_reconciler(&client, project);

        let constraints = vec![
            LayoutConstraint::new(LayoutConstraintKind::ObjectAllocationSize {
                entity,
                size_bytes: 32,
            }),
            LayoutConstraint::new(LayoutConstraintKind::FieldObservedAtOffset {
                entity,
                offset: 8,
            }),
            LayoutConstraint::new(LayoutConstraintKind::ReadWidth {
                entity,
                offset: 8,
                width_bytes: 4,
            }),
        ];

        let layouts = reconciler.reconcile(&constraints).expect("reconcile");
        assert_eq!(layouts.len(), 1);
        assert_eq!(layouts[0].entity_id, entity);
        assert_eq!(layouts[0].computed_size_bytes, Some(32));
        assert_eq!(layouts[0].fields.len(), 1);
        assert_eq!(layouts[0].fields[0].offset, 8);
        assert_eq!(layouts[0].fields[0].width_bytes, Some(4));

        let commands = client.commands();
        let hypotheses: Vec<_> = commands
            .iter()
            .filter(|c| matches!(c, ApplicationCommand::AddHypothesis(_)))
            .collect();
        assert_eq!(hypotheses.len(), 1);
        if let ApplicationCommand::AddHypothesis(req) = &hypotheses[0] {
            assert_eq!(req.subject, entity);
            assert_eq!(req.predicate, LAYOUT_HYPOTHESIS_PREDICATE);
            assert_eq!(req.confidence_score, 1.0);
            assert!(matches!(req.candidate, EvidenceValue::String(_)));
        } else {
            panic!("expected AddHypothesis");
        }
        assert!(
            !commands
                .iter()
                .any(|c| matches!(c, ApplicationCommand::CreateWorkItems(_)))
        );
    }

    #[test]
    fn reconciler_creates_conflict_work_item_for_incompatible_size_offset() {
        let client = RecordingAutoReClient::new();
        let project = ProjectId::new();
        let entity = EntityId::new();
        let reconciler = make_reconciler(&client, project);

        let constraints = vec![
            LayoutConstraint::new(LayoutConstraintKind::ObjectAllocationSize {
                entity,
                size_bytes: 32,
            }),
            LayoutConstraint::new(LayoutConstraintKind::FieldObservedAtOffset {
                entity,
                offset: 64,
            }),
        ];

        let layouts = reconciler.reconcile(&constraints).expect("reconcile");
        assert!(layouts.is_empty());

        let commands = client.commands();
        let work_items: Vec<_> = commands
            .iter()
            .filter(|c| matches!(c, ApplicationCommand::CreateWorkItems(_)))
            .collect();
        assert_eq!(work_items.len(), 1);
        if let ApplicationCommand::CreateWorkItems(req) = &work_items[0] {
            assert_eq!(req.descriptions.len(), 1);
            assert!(req.descriptions[0].starts_with(CONFLICT_RESOLUTION_PREFIX));
            assert!(req.descriptions[0].contains(&entity.to_string()));
        } else {
            panic!("expected CreateWorkItems");
        }
        assert!(
            !commands
                .iter()
                .any(|c| matches!(c, ApplicationCommand::AddHypothesis(_)))
        );
    }

    #[test]
    fn reconciler_handles_array_stride() {
        let client = RecordingAutoReClient::new();
        let project = ProjectId::new();
        let entity = EntityId::new();
        let reconciler = make_reconciler(&client, project);

        let constraints = vec![
            LayoutConstraint::new(LayoutConstraintKind::ObjectAllocationSize {
                entity,
                size_bytes: 32,
            }),
            LayoutConstraint::new(LayoutConstraintKind::ArrayStride {
                entity,
                stride_bytes: 16,
            }),
        ];

        let layouts = reconciler.reconcile(&constraints).expect("reconcile");
        assert_eq!(layouts.len(), 1);
        assert_eq!(layouts[0].array_stride, Some(16));

        let commands = client.commands();
        assert_eq!(
            commands
                .iter()
                .filter(|c| matches!(c, ApplicationCommand::AddHypothesis(_)))
                .count(),
            1
        );
    }

    #[test]
    fn reconciler_does_not_fork_duplicate_types_on_conflict() {
        let client = RecordingAutoReClient::new();
        let project = ProjectId::new();
        let entity = EntityId::new();
        let reconciler = make_reconciler(&client, project);

        let constraints = vec![
            LayoutConstraint::new(LayoutConstraintKind::ObjectAllocationSize {
                entity,
                size_bytes: 32,
            }),
            LayoutConstraint::new(LayoutConstraintKind::ObjectAllocationSize {
                entity,
                size_bytes: 64,
            }),
        ];

        let layouts = reconciler.reconcile(&constraints).expect("reconcile");
        assert!(layouts.is_empty());

        let commands = client.commands();
        let work_item_count = commands
            .iter()
            .filter(|c| matches!(c, ApplicationCommand::CreateWorkItems(_)))
            .count();
        let hypothesis_count = commands
            .iter()
            .filter(|c| matches!(c, ApplicationCommand::AddHypothesis(_)))
            .count();
        let register_count = commands
            .iter()
            .filter(|c| matches!(c, ApplicationCommand::RegisterEntity(_)))
            .count();

        assert_eq!(work_item_count, 1, "exactly one conflict work item");
        assert_eq!(hypothesis_count, 0, "no layout hypothesis on conflict");
        assert_eq!(register_count, 0, "never fork duplicate entity types");
    }
}
