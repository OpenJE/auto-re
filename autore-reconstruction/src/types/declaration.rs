//! Deterministic C++ declaration generator for accepted canonical type hypotheses.
//!
//! [`DeclarationGenerator`] consumes accepted [`CanonicalTypeHypothesis`] records,
//! renders C++ forward declarations under `generated/openvb/include/recovered/`,
//! and registers the generated artifacts through [`ApplicationCommand`] variants.
//! No LLM calls are made — all output is deterministic.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use autore_app::application_service::requests::{
    CreateWorkItemsRequest, RegisterArtifactRequest, RegisterArtifactResponse,
    RegisterGeneratedSourceMappingRequest, RegisterGeneratedSourceMappingResponse,
};
use autore_app::{ApplicationCommand, AutoReClient, CommandResult};
use autore_core::{Error, Result};
use autore_schema::domain::records::{CanonicalTypeHypothesis, HypothesisStatus};
use autore_schema::ids::{ArtifactId, EntityId, GeneratedSourceMappingId, ProjectId};

use super::reconciler::{ReconciledLayout, ReconciledVtableSlot};

/// Prefix used in work-item descriptions to encode a build-failure intent,
/// because `CreateWorkItemsRequest` has no dedicated `kind` field.
pub const BUILD_FAILURE_PREFIX: &str = "BuildFailure:";

/// One declaration file produced by [`DeclarationGenerator`].
#[derive(Debug, Clone, PartialEq)]
pub struct DeclarationOutput {
    pub entity_id: EntityId,
    pub file_path: PathBuf,
    pub artifact_id: ArtifactId,
    pub mapping_id: GeneratedSourceMappingId,
}

/// Generates deterministic C++ declarations from accepted canonical type hypotheses.
pub struct DeclarationGenerator<'a> {
    project: ProjectId,
    campaign_id: String,
    output_root: PathBuf,
    client: &'a dyn AutoReClient,
}

impl<'a> DeclarationGenerator<'a> {
    /// Creates a new declaration generator for the given project context.
    pub fn new(
        project: ProjectId,
        campaign_id: String,
        output_root: PathBuf,
        client: &'a dyn AutoReClient,
    ) -> Self {
        Self {
            project,
            campaign_id,
            output_root,
            client,
        }
    }

    /// Generates/updates `include/recovered/<entity>.hpp` declarations for every
    /// accepted canonical type hypothesis.
    ///
    /// If two accepted hypotheses for the same entity have different computed
    /// sizes, a `CreateWorkItems` command with a `BuildFailure:` description is
    /// issued instead of generating for that entity.
    pub fn generate_accepted_types(
        &self,
        hypotheses: &[CanonicalTypeHypothesis],
    ) -> Result<Vec<DeclarationOutput>> {
        let by_entity = group_accepted_hypotheses(hypotheses);
        let mut outputs = Vec::new();

        for (entity_id, entity_hypotheses) in by_entity {
            if let Some(conflict) = detect_size_conflict(entity_id, &entity_hypotheses)? {
                self.issue_build_failure_work(&conflict)?;
                continue;
            }

            // Use the first accepted layout deterministically; conflicts already handled.
            let layout = parse_layout(entity_hypotheses[0])?;
            let output = self.generate_type_declaration(entity_id, &layout)?;
            outputs.push(output);
        }

        Ok(outputs)
    }

    /// Generates `include/recovered/<entity>_vtable.hpp` vtable scaffolding for
    /// every accepted hypothesis that has virtual-method slot targets.
    pub fn generate_vtables(
        &self,
        hypotheses: &[CanonicalTypeHypothesis],
    ) -> Result<Vec<DeclarationOutput>> {
        let by_entity = group_accepted_hypotheses(hypotheses);
        let mut outputs = Vec::new();

        for (entity_id, entity_hypotheses) in by_entity {
            if detect_size_conflict(entity_id, &entity_hypotheses)?.is_some() {
                // Skip vtable generation for entities with conflicting accepted layouts;
                // the conflict was already reported by `generate_accepted_types`.
                continue;
            }

            let layout = parse_layout(entity_hypotheses[0])?;
            if layout.vtable_slot_targets.is_empty() {
                continue;
            }

            let output =
                self.generate_vtable_declaration(entity_id, &layout.vtable_slot_targets)?;
            outputs.push(output);
        }

        Ok(outputs)
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn generate_type_declaration(
        &self,
        entity_id: EntityId,
        layout: &ReconciledLayout,
    ) -> Result<DeclarationOutput> {
        let rel_path = entity_to_source_path(entity_id);
        let hpp_rel = Path::new("include/recovered")
            .join(&rel_path)
            .with_extension("hpp");
        let hpp_path = self.output_root.join(&hpp_rel);

        if let Some(parent) = hpp_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = render_struct_decl(entity_id, layout);
        fs::write(&hpp_path, content)?;

        let artifact_id = self.register_artifact(&hpp_rel)?;
        let mapping_id = self.register_source_mapping(entity_id)?;

        Ok(DeclarationOutput {
            entity_id,
            file_path: hpp_path,
            artifact_id,
            mapping_id,
        })
    }

    fn generate_vtable_declaration(
        &self,
        entity_id: EntityId,
        slots: &[ReconciledVtableSlot],
    ) -> Result<DeclarationOutput> {
        let rel_path = {
            let mut p = entity_to_source_path(entity_id);
            let stem = p
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            p.set_file_name(format!("{stem}_vtable"));
            p.set_extension("hpp");
            p
        };
        let hpp_rel = Path::new("include/recovered").join(&rel_path);
        let hpp_path = self.output_root.join(&hpp_rel);

        if let Some(parent) = hpp_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = render_vtable_decl(entity_id, slots);
        fs::write(&hpp_path, content)?;

        let artifact_id = self.register_artifact(&hpp_rel)?;
        let mapping_id = self.register_source_mapping(entity_id)?;

        Ok(DeclarationOutput {
            entity_id,
            file_path: hpp_path,
            artifact_id,
            mapping_id,
        })
    }

    fn register_artifact(&self, source_path: &Path) -> Result<ArtifactId> {
        let req = RegisterArtifactRequest {
            project: self.project,
            source_path: source_path.to_path_buf(),
            kind: "core.generated-candidate".to_string(),
        };
        match self
            .client
            .execute(ApplicationCommand::RegisterArtifact(req))?
        {
            CommandResult::ArtifactRegistered(RegisterArtifactResponse { artifact }) => {
                Ok(artifact.id)
            }
            other => Err(Error::Validation(format!(
                "RegisterArtifact returned unexpected result: {other:?}"
            ))),
        }
    }

    fn register_source_mapping(&self, entity_id: EntityId) -> Result<GeneratedSourceMappingId> {
        let req = RegisterGeneratedSourceMappingRequest {
            project: self.project,
            work_item_id: entity_id.to_string(),
        };
        match self
            .client
            .execute(ApplicationCommand::RegisterGeneratedSourceMapping(req))?
        {
            CommandResult::GeneratedSourceMappingRegistered(
                RegisterGeneratedSourceMappingResponse { mapping_id },
            ) => {
                let uuid = uuid::Uuid::parse_str(&mapping_id)
                    .map_err(|e| Error::Validation(format!("invalid mapping id: {e}")))?;
                Ok(GeneratedSourceMappingId::from_uuid(uuid))
            }
            other => Err(Error::Validation(format!(
                "RegisterGeneratedSourceMapping returned unexpected result: {other:?}"
            ))),
        }
    }

    fn issue_build_failure_work(&self, description: &str) -> Result<()> {
        let req = CreateWorkItemsRequest {
            project: self.project,
            campaign_id: self.campaign_id.clone(),
            descriptions: vec![description.to_string()],
        };
        self.client
            .execute(ApplicationCommand::CreateWorkItems(req))
            .map(|_| ())
    }
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

/// Derives a stable relative path from the entity's UUID.
///
/// Format: `<2hex>/<2hex>/<2hex>/<full-uuid>` — same rule as skeleton generation.
pub fn entity_to_source_path(entity_id: EntityId) -> PathBuf {
    let hex = entity_id.as_uuid().as_simple().to_string();
    PathBuf::from(&hex[0..2])
        .join(&hex[2..4])
        .join(&hex[4..6])
        .join(&hex)
}

fn short_id(entity_id: EntityId) -> String {
    entity_id.as_uuid().as_simple().to_string()[..8].to_string()
}

/// Renders a C++ struct/class declaration for the given entity and layout.
///
/// Output is a header with `#pragma once`, namespace `recovered`, optional base
/// classes, an optional vtable pointer, and `uint8_t` placeholder fields with
/// explicit padding to preserve offsets.
pub fn render_struct_decl(entity_id: EntityId, layout: &ReconciledLayout) -> String {
    let sid = short_id(entity_id);
    let mut out = String::new();
    out.push_str("#pragma once\n\n");
    out.push_str("namespace recovered {\n\n");

    let base_decls: Vec<String> = layout
        .base_adjustments
        .iter()
        .map(|adj| format!("public autore_recovered_{}", short_id(adj.base_entity)))
        .collect();

    if base_decls.is_empty() {
        out.push_str(&format!("struct autore_recovered_{sid} {{\n"));
    } else {
        out.push_str(&format!(
            "struct autore_recovered_{sid} : {} {{\n",
            base_decls.join(", ")
        ));
    }

    // If virtual slots exist, reserve a vtable pointer. The reconciler already
    // records the vtable pointer location as a field, so only add one when it
    // is not already explicitly represented at offset 0.
    if !layout.vtable_slot_targets.is_empty()
        && !layout
            .fields
            .iter()
            .any(|f| f.offset == 0 && f.width_bytes == Some(8))
    {
        out.push_str("    void** vtable;\n");
    }

    let mut cursor: usize = 0;
    for field in &layout.fields {
        if field.offset > cursor {
            let pad = field.offset - cursor;
            out.push_str(&format!("    uint8_t pad_{cursor}[{pad}];\n"));
            cursor = field.offset;
        }
        let width = field.width_bytes.unwrap_or(1);
        out.push_str(&format!("    uint8_t field_{cursor}[{width}];\n"));
        cursor += width;
    }

    if let Some(size) = layout.computed_size_bytes
        && size > cursor
    {
        let pad = size - cursor;
        out.push_str(&format!("    uint8_t pad_{cursor}[{pad}];\n"));
    }

    out.push_str("};\n\n");
    out.push_str("} // namespace recovered\n");
    out
}

/// Renders a C++ vtable struct declaration with function-pointer slots in
/// canonical slot-index order.
pub fn render_vtable_decl(entity_id: EntityId, slots: &[ReconciledVtableSlot]) -> String {
    let sid = short_id(entity_id);
    let mut sorted: Vec<&ReconciledVtableSlot> = slots.iter().collect();
    sorted.sort_by_key(|s| s.slot_idx);

    let mut out = String::new();
    out.push_str("#pragma once\n\n");
    out.push_str("namespace recovered {\n\n");
    out.push_str(&format!("struct autore_recovered_{sid}_vtable {{\n"));
    for slot in sorted {
        out.push_str(&format!(
            "    void (*slot_{idx})(); // calls entity {called}\n",
            idx = slot.slot_idx,
            called = slot.called
        ));
    }
    out.push_str("};\n\n");
    out.push_str("} // namespace recovered\n");
    out
}

// ---------------------------------------------------------------------------
// Hypothesis grouping and conflict detection
// ---------------------------------------------------------------------------

fn group_accepted_hypotheses(
    hypotheses: &[CanonicalTypeHypothesis],
) -> BTreeMap<EntityId, Vec<&CanonicalTypeHypothesis>> {
    let mut map: BTreeMap<EntityId, Vec<&CanonicalTypeHypothesis>> = BTreeMap::new();
    for h in hypotheses {
        if h.status == HypothesisStatus::Accepted {
            map.entry(h.entity_id).or_default().push(h);
        }
    }
    map
}

fn detect_size_conflict(
    entity_id: EntityId,
    hypotheses: &[&CanonicalTypeHypothesis],
) -> Result<Option<String>> {
    if hypotheses.len() < 2 {
        return Ok(None);
    }

    let sizes: Vec<Option<usize>> = hypotheses
        .iter()
        .map(|h| Ok(parse_layout(h)?.computed_size_bytes))
        .collect::<Result<Vec<_>>>()?;

    let first = sizes[0];
    if sizes.iter().any(|s| *s != first) {
        Ok(Some(format!(
            "{BUILD_FAILURE_PREFIX} conflicting accepted layouts for entity {entity_id}"
        )))
    } else {
        Ok(None)
    }
}

fn parse_layout(hypothesis: &CanonicalTypeHypothesis) -> Result<ReconciledLayout> {
    serde_json::from_str(&hypothesis.layout_json)
        .map_err(|e| Error::Serialization(format!("invalid layout_json: {e}")))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::RecordingAutoReClient;
    use autore_app::ApplicationCommand;
    use autore_schema::domain::Timestamp;
    use autore_schema::domain::records::HypothesisStatus;
    use autore_schema::ids::ProjectId;

    fn make_layout(entity: EntityId, size: usize, fields: &[(usize, usize)]) -> String {
        let layout = ReconciledLayout {
            entity_id: entity,
            computed_size_bytes: Some(size),
            computed_alignment: None,
            fields: fields
                .iter()
                .map(
                    |(offset, width)| super::super::reconciler::ReconciledField {
                        offset: *offset,
                        width_bytes: Some(*width),
                    },
                )
                .collect(),
            vtable_slot_targets: vec![],
            base_adjustments: vec![],
            array_stride: None,
            parameter_usages: vec![],
            return_value_use: None,
            source_constraints: vec![],
        };
        serde_json::to_string(&layout).unwrap()
    }

    fn make_vtable_layout(entity: EntityId, size: usize, slots: &[(usize, EntityId)]) -> String {
        let layout = ReconciledLayout {
            entity_id: entity,
            computed_size_bytes: Some(size),
            computed_alignment: None,
            fields: vec![],
            vtable_slot_targets: slots
                .iter()
                .map(
                    |(idx, called)| super::super::reconciler::ReconciledVtableSlot {
                        slot_idx: *idx,
                        called: *called,
                    },
                )
                .collect(),
            base_adjustments: vec![],
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
        status: HypothesisStatus,
    ) -> CanonicalTypeHypothesis {
        let mut h = CanonicalTypeHypothesis::new(project, entity, layout);
        h.status = status;
        h.updated_at = Timestamp::now();
        h
    }

    fn make_generator<'a>(
        client: &'a RecordingAutoReClient,
        project: ProjectId,
        output_root: PathBuf,
    ) -> DeclarationGenerator<'a> {
        DeclarationGenerator::new(project, "campaign-1".into(), output_root, client)
    }

    #[test]
    fn declaration_generator_emits_hpp_for_accepted_type() {
        let dir = tempfile::tempdir().unwrap();
        let client = RecordingAutoReClient::new();
        let project = ProjectId::new();
        let entity = EntityId::new();
        let generator = make_generator(&client, project, dir.path().to_path_buf());

        let layout = make_layout(entity, 32, &[(8, 4)]);
        let hypothesis = make_hypothesis(project, entity, layout, HypothesisStatus::Accepted);

        let outputs = generator.generate_accepted_types(&[hypothesis]).unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].entity_id, entity);
        assert!(outputs[0].file_path.exists());

        let content = fs::read_to_string(&outputs[0].file_path).unwrap();
        assert!(content.contains("#pragma once"));
        assert!(content.contains("namespace recovered"));
        assert!(content.contains("struct autore_recovered_"));
        assert!(content.contains("uint8_t field_8[4]"));
        assert!(content.contains("uint8_t pad_12[20]"));

        let commands = client.commands();
        assert!(commands.iter().any(|c| matches!(
            c,
            ApplicationCommand::RegisterArtifact(RegisterArtifactRequest { kind, .. })
            if kind == "core.generated-candidate"
        )));
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, ApplicationCommand::RegisterGeneratedSourceMapping(_)))
        );
    }

    #[test]
    fn declaration_generator_replaces_stub_on_accept() {
        let dir = tempfile::tempdir().unwrap();
        let client = RecordingAutoReClient::new();
        let project = ProjectId::new();
        let entity = EntityId::new();
        let generator = make_generator(&client, project, dir.path().to_path_buf());

        // Pre-seed the expected path with a stub marker.
        let rel = entity_to_source_path(entity);
        let hpp_path = dir
            .path()
            .join("include/recovered")
            .join(&rel)
            .with_extension("hpp");
        fs::create_dir_all(hpp_path.parent().unwrap()).unwrap();
        fs::write(&hpp_path, "// reconstruction stub\n").unwrap();

        let layout = make_layout(entity, 16, &[(0, 4), (8, 4)]);
        let hypothesis = make_hypothesis(project, entity, layout, HypothesisStatus::Accepted);

        let outputs = generator.generate_accepted_types(&[hypothesis]).unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].file_path, hpp_path);

        let content = fs::read_to_string(&hpp_path).unwrap();
        assert!(!content.contains("reconstruction stub"));
        assert!(content.contains("namespace recovered"));
        assert!(content.contains("uint8_t field_0[4]"));
        assert!(content.contains("uint8_t field_8[4]"));

        assert!(
            client
                .commands()
                .iter()
                .any(|c| matches!(c, ApplicationCommand::RegisterGeneratedSourceMapping(_)))
        );
    }

    #[test]
    fn declaration_generator_emits_vtable_in_canonical_slot_order() {
        let dir = tempfile::tempdir().unwrap();
        let client = RecordingAutoReClient::new();
        let project = ProjectId::new();
        let entity = EntityId::new();
        let target_a = EntityId::new();
        let target_b = EntityId::new();
        let generator = make_generator(&client, project, dir.path().to_path_buf());

        let layout = make_vtable_layout(entity, 8, &[(1, target_a), (0, target_b)]);
        let hypothesis = make_hypothesis(project, entity, layout, HypothesisStatus::Accepted);

        let outputs = generator.generate_vtables(&[hypothesis]).unwrap();
        assert_eq!(outputs.len(), 1);
        assert!(outputs[0].file_path.exists());

        let content = fs::read_to_string(&outputs[0].file_path).unwrap();
        assert!(content.contains("#pragma once"));
        assert!(content.contains("namespace recovered"));
        assert!(content.contains("struct autore_recovered_"));
        assert!(content.contains("void (*slot_0)()"));
        assert!(content.contains("void (*slot_1)()"));

        // Slots must appear in canonical slot-index order, not input order.
        let slot_0_pos = content.find("slot_0").unwrap();
        let slot_1_pos = content.find("slot_1").unwrap();
        assert!(slot_0_pos < slot_1_pos);
    }

    #[test]
    fn conflicting_accepted_layouts_create_build_failure_work() {
        let dir = tempfile::tempdir().unwrap();
        let client = RecordingAutoReClient::new();
        let project = ProjectId::new();
        let entity = EntityId::new();
        let generator = make_generator(&client, project, dir.path().to_path_buf());

        let layout_small = make_layout(entity, 16, &[(0, 4)]);
        let layout_large = make_layout(entity, 32, &[(0, 4)]);
        let hypotheses = vec![
            make_hypothesis(project, entity, layout_small, HypothesisStatus::Accepted),
            make_hypothesis(project, entity, layout_large, HypothesisStatus::Accepted),
        ];

        let outputs = generator.generate_accepted_types(&hypotheses).unwrap();
        assert!(outputs.is_empty());

        let commands = client.commands();
        let work_items: Vec<_> = commands
            .iter()
            .filter(|c| matches!(c, ApplicationCommand::CreateWorkItems(_)))
            .collect();
        assert_eq!(work_items.len(), 1);
        if let ApplicationCommand::CreateWorkItems(req) = &work_items[0] {
            assert_eq!(req.descriptions.len(), 1);
            assert!(req.descriptions[0].starts_with(BUILD_FAILURE_PREFIX));
            assert!(req.descriptions[0].contains(&entity.to_string()));
        } else {
            panic!("expected CreateWorkItems");
        }

        // No declaration file should be written for the conflicting entity.
        let rel = entity_to_source_path(entity);
        let hpp_path = dir
            .path()
            .join("include/recovered")
            .join(&rel)
            .with_extension("hpp");
        assert!(!hpp_path.exists());
    }
}
