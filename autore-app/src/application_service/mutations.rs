use std::path::Path;

use autore_core::{Error, Result};
use autore_schema::domain::records::{
    Artifact, ArtifactStorage, CancellationRequest, Contradiction, EvidenceRecord, Hypothesis,
    HypothesisStatus, Operation, OperationFailure, SemanticEntity, VerificationRecord,
};
use autore_schema::domain::{ContentHash, MetadataMap, NamespacedId, Timestamp};
use autore_schema::ids::{ArtifactId, HypothesisId, OperationId, ProjectId};
use autore_store::Transaction;

pub fn insert_project(
    txn: &Transaction<'_>,
    project: &autore_schema::domain::records::Project,
) -> Result<()> {
    let id_bytes = project.id.as_uuid().as_bytes();
    let schema_version = project.schema_version.to_string();
    let created_at = project.created_at.to_string();
    let updated_at = project.updated_at.to_string();
    let metadata = serde_json::to_string(&project.metadata)
        .map_err(|e| Error::Serialization(e.to_string()))?;

    txn.conn()
        .execute(
            "INSERT INTO projects (id, name, schema_version, created_at, updated_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id_bytes.as_slice(),
                &project.name,
                schema_version,
                created_at,
                updated_at,
                metadata,
            ],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
    Ok(())
}

pub fn insert_artifact_managed(
    txn: &Transaction<'_>,
    project_dir: &Path,
    project_id: ProjectId,
    source_path: &Path,
    kind: NamespacedId,
    artifact_id: ArtifactId,
) -> Result<Artifact> {
    let data = std::fs::read(source_path).map_err(Error::Io)?;
    let size = data.len() as u64;
    let hash = ContentHash::sha256(&data);
    let digest_hex = hash.digest_hex();

    let blob_path = project_dir
        .join("artifacts")
        .join(hash.algorithm.to_string())
        .join(&digest_hex[..2])
        .join(&digest_hex);

    if !blob_path.exists() {
        if let Some(parent) = blob_path.parent() {
            std::fs::create_dir_all(parent).map_err(Error::Io)?;
        }
        std::fs::write(&blob_path, &data).map_err(Error::Io)?;
    }

    let relative = blob_path
        .strip_prefix(project_dir)
        .unwrap_or(&blob_path)
        .to_path_buf();

    let artifact = Artifact {
        id: artifact_id,
        project: project_id,
        kind,
        content_hash: hash,
        size,
        storage: ArtifactStorage::ManagedBlob {
            relative_path: relative,
        },
        created_at: Timestamp::now(),
        metadata: MetadataMap::new(),
    };

    insert_artifact_row(txn, &artifact)?;
    Ok(artifact)
}

fn insert_artifact_row(txn: &Transaction<'_>, a: &Artifact) -> Result<()> {
    let id_bytes = a.id.as_uuid().as_bytes().to_vec();
    let project_bytes = a.project.as_uuid().as_bytes().to_vec();
    let kind = a.kind.to_string();
    let algo = a.content_hash.algorithm.to_string();
    let digest = a.content_hash.digest.clone();
    let size = a.size as i64;
    let (storage_kind, storage_path) = match &a.storage {
        ArtifactStorage::ManagedBlob { relative_path } => (
            "managed".to_string(),
            relative_path.to_string_lossy().to_string(),
        ),
        ArtifactStorage::ExternalFile { canonical_path } => (
            "external".to_string(),
            canonical_path.to_string_lossy().to_string(),
        ),
    };
    let created_at = a.created_at.to_string();
    let metadata =
        serde_json::to_string(&a.metadata).map_err(|e| Error::Serialization(e.to_string()))?;

    txn.conn()
        .execute(
            "INSERT INTO stage0_artifacts \
             (id, project_id, kind, hash_algorithm, hash_digest, size, \
              storage_kind, storage_path, created_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                kind,
                algo,
                digest,
                size,
                storage_kind,
                storage_path,
                created_at,
                metadata,
            ],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
    Ok(())
}

pub fn insert_entity(txn: &Transaction<'_>, entity: &SemanticEntity) -> Result<()> {
    let id_bytes = entity.id.as_uuid().as_bytes().to_vec();
    let project_bytes = entity.project.as_uuid().as_bytes().to_vec();
    let kind = entity.kind.to_string();
    let stable_key_json = entity
        .stable_key
        .as_ref()
        .map(|k| serde_json::to_string(k).map_err(|e| Error::Serialization(e.to_string())))
        .transpose()?;
    let created_at = entity.created_at.to_string();
    let metadata =
        serde_json::to_string(&entity.metadata).map_err(|e| Error::Serialization(e.to_string()))?;

    txn.conn()
        .execute(
            "INSERT INTO semantic_entities \
             (id, project_id, kind, stable_key, display_name, created_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                kind,
                stable_key_json,
                entity.display_name.as_ref(),
                created_at,
                metadata,
            ],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
    Ok(())
}

pub fn insert_evidence(txn: &Transaction<'_>, record: &EvidenceRecord) -> Result<()> {
    let id_bytes = record.id.as_uuid().as_bytes().to_vec();
    let project_bytes = record.project.as_uuid().as_bytes().to_vec();
    let subject_bytes = record.subject.as_uuid().as_bytes().to_vec();
    let predicate = record.predicate.to_string();
    let value_json =
        serde_json::to_string(&record.value).map_err(|e| Error::Serialization(e.to_string()))?;
    let derivation_json = serde_json::to_string(&record.derivation)
        .map_err(|e| Error::Serialization(e.to_string()))?;
    let provider_run_bytes = record
        .provider_run
        .map(|id| id.as_uuid().as_bytes().to_vec());
    let native_json = serde_json::to_string(&record.native_artifacts)
        .map_err(|e| Error::Serialization(e.to_string()))?;
    let assumptions_json = serde_json::to_string(&record.assumptions)
        .map_err(|e| Error::Serialization(e.to_string()))?;
    let created_at = record.created_at.to_string();

    txn.conn()
        .execute(
            "INSERT INTO evidence_records \
             (id, project_id, subject, predicate, value, derivation, \
              provider_run, native_artifacts, assumptions, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                subject_bytes,
                predicate,
                value_json,
                derivation_json,
                provider_run_bytes,
                native_json,
                assumptions_json,
                created_at,
            ],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
    Ok(())
}

fn hypothesis_status_to_db(status: &HypothesisStatus) -> (&'static str, Option<Vec<u8>>) {
    match status {
        HypothesisStatus::Proposed => ("Proposed", None),
        HypothesisStatus::UnderInvestigation => ("UnderInvestigation", None),
        HypothesisStatus::Accepted => ("Accepted", None),
        HypothesisStatus::Rejected => ("Rejected", None),
        HypothesisStatus::Superseded { by } => {
            ("Superseded", Some(by.as_uuid().as_bytes().to_vec()))
        }
    }
}

fn hypothesis_status_from_db(
    status_str: &str,
    superseded_by_bytes: Option<Vec<u8>>,
) -> Result<HypothesisStatus> {
    match status_str {
        "Proposed" => Ok(HypothesisStatus::Proposed),
        "UnderInvestigation" => Ok(HypothesisStatus::UnderInvestigation),
        "Accepted" => Ok(HypothesisStatus::Accepted),
        "Rejected" => Ok(HypothesisStatus::Rejected),
        "Superseded" => {
            let bytes = superseded_by_bytes.ok_or_else(|| {
                Error::Database("Superseded status missing superseded_by bytes".into())
            })?;
            let uuid = uuid::Uuid::from_slice(&bytes)
                .map_err(|e| Error::Database(format!("invalid superseded_by UUID: {e}")))?;
            Ok(HypothesisStatus::Superseded {
                by: HypothesisId::from_uuid(uuid),
            })
        }
        other => Err(Error::Database(format!(
            "unknown hypothesis status: {other}"
        ))),
    }
}

pub fn insert_hypothesis(txn: &Transaction<'_>, hypothesis: &Hypothesis) -> Result<()> {
    let id_bytes = hypothesis.id.as_uuid().as_bytes().to_vec();
    let project_bytes = hypothesis.project.as_uuid().as_bytes().to_vec();
    let subject_bytes = hypothesis.subject.as_uuid().as_bytes().to_vec();
    let predicate = hypothesis.predicate.to_string();
    let candidate_json = serde_json::to_string(&hypothesis.candidate)
        .map_err(|e| Error::Serialization(e.to_string()))?;
    let supporting_json = serde_json::to_string(&hypothesis.supporting_evidence)
        .map_err(|e| Error::Serialization(e.to_string()))?;
    let contradicting_json = serde_json::to_string(&hypothesis.contradicting_evidence)
        .map_err(|e| Error::Serialization(e.to_string()))?;
    let derived_json = serde_json::to_string(&hypothesis.derived_from)
        .map_err(|e| Error::Serialization(e.to_string()))?;
    let confidence_json = serde_json::to_string(&hypothesis.confidence)
        .map_err(|e| Error::Serialization(e.to_string()))?;
    let (status_str, superseded_by_bytes) = hypothesis_status_to_db(&hypothesis.status);
    let created_at = hypothesis.created_at.to_string();
    let updated_at = hypothesis.updated_at.to_string();

    txn.conn()
        .execute(
            "INSERT INTO hypotheses \
             (id, project_id, subject, predicate, candidate, \
              supporting_evidence, contradicting_evidence, derived_from, \
              confidence, status, superseded_by, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                subject_bytes,
                predicate,
                candidate_json,
                supporting_json,
                contradicting_json,
                derived_json,
                confidence_json,
                status_str,
                superseded_by_bytes,
                created_at,
                updated_at,
            ],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
    Ok(())
}

pub fn update_hypothesis_status(
    txn: &Transaction<'_>,
    id: HypothesisId,
    target: HypothesisStatus,
) -> Result<()> {
    let id_bytes = id.as_uuid().as_bytes().to_vec();

    let (current_status_str, current_superseded_bytes): (String, Option<Vec<u8>>) = txn
        .conn()
        .query_row(
            "SELECT status, superseded_by FROM hypotheses WHERE id = ?1",
            rusqlite::params![id_bytes],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                Error::NotFound(format!("hypothesis {id} not found"))
            }
            other => Error::Database(other.to_string()),
        })?;

    let current_status = hypothesis_status_from_db(&current_status_str, current_superseded_bytes)?;
    current_status.transition(&target)?;

    let (status_str, superseded_by_bytes) = hypothesis_status_to_db(&target);
    let updated_at = Timestamp::now().to_string();

    txn.conn()
        .execute(
            "UPDATE hypotheses \
             SET status = ?1, superseded_by = ?2, updated_at = ?3 \
             WHERE id = ?4",
            rusqlite::params![status_str, superseded_by_bytes, updated_at, id_bytes],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
    Ok(())
}

pub fn insert_contradiction(txn: &Transaction<'_>, contradiction: &Contradiction) -> Result<()> {
    let id_bytes = contradiction.id.as_uuid().as_bytes().to_vec();
    let project_bytes = contradiction.project.as_uuid().as_bytes().to_vec();
    let subject_bytes = contradiction.subject.as_uuid().as_bytes().to_vec();
    let predicate = contradiction.predicate.to_string();
    let evidence_json = serde_json::to_string(&contradiction.evidence)
        .map_err(|e| Error::Serialization(e.to_string()))?;
    let hypotheses_json = serde_json::to_string(&contradiction.hypotheses)
        .map_err(|e| Error::Serialization(e.to_string()))?;
    let status_str = contradiction.status.kind();
    let resolution_json = contradiction
        .resolution
        .as_ref()
        .map(|r| serde_json::to_string(r).map_err(|e| Error::Serialization(e.to_string())))
        .transpose()?;
    let created_at = contradiction.created_at.to_string();
    let updated_at = contradiction.updated_at.to_string();

    txn.conn()
        .execute(
            "INSERT INTO contradictions \
             (id, project_id, subject, predicate, evidence, hypotheses, \
              status, resolution, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                subject_bytes,
                predicate,
                evidence_json,
                hypotheses_json,
                status_str,
                resolution_json,
                created_at,
                updated_at,
            ],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
    Ok(())
}

pub fn insert_verification(txn: &Transaction<'_>, record: &VerificationRecord) -> Result<()> {
    let id_bytes = record.id.as_uuid().as_bytes().to_vec();
    let project_bytes = record.project.as_uuid().as_bytes().to_vec();
    let subject_kind = record.subject.kind();
    let subject_id = record.subject.id_uuid().as_bytes().to_vec();
    let check_str = record.check.to_string();
    let state_str = record.state.kind();
    let provider_run_bytes = record
        .provider_run
        .map(|id| id.as_uuid().as_bytes().to_vec());
    let evidence_json =
        serde_json::to_string(&record.evidence).map_err(|e| Error::Serialization(e.to_string()))?;
    let details_json = record
        .details
        .as_ref()
        .map(|d| serde_json::to_string(d).map_err(|e| Error::Serialization(e.to_string())))
        .transpose()?;
    let created_at = record.created_at.to_string();

    txn.conn()
        .execute(
            "INSERT INTO verification_records \
             (id, project_id, subject_kind, subject_id, check_kind, state, \
              provider_run, evidence, details, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                subject_kind,
                subject_id,
                check_str,
                state_str,
                provider_run_bytes,
                evidence_json,
                details_json,
                created_at,
            ],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
    Ok(())
}

fn subject_to_json(
    s: &Option<autore_schema::domain::records::EventSubject>,
) -> Result<Option<String>> {
    match s {
        Some(subject) => serde_json::to_string(subject)
            .map(Some)
            .map_err(|e| Error::Serialization(e.to_string())),
        None => Ok(None),
    }
}

pub fn insert_operation(txn: &Transaction<'_>, operation: &Operation) -> Result<()> {
    let id_bytes = operation.id.as_uuid().as_bytes().to_vec();
    let project_bytes = operation.project.as_uuid().as_bytes().to_vec();
    let kind = operation.kind.to_string();
    let state = operation.state.kind();
    let subject_json = subject_to_json(&operation.subject)?;
    let parent_bytes = operation.parent.map(|p| p.as_uuid().as_bytes().to_vec());
    let failure_json = operation
        .failure
        .as_ref()
        .map(|f| serde_json::to_string(f).map_err(|e| Error::Serialization(e.to_string())))
        .transpose()?;
    let created_at = operation.created_at.to_string();
    let updated_at = operation.updated_at.to_string();

    txn.conn()
        .execute(
            "INSERT INTO operations \
             (id, project_id, kind, state, subject, requested_by, parent, failure, \
              created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                kind,
                state,
                subject_json,
                &operation.requested_by,
                parent_bytes,
                failure_json,
                created_at,
                updated_at,
            ],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
    Ok(())
}

pub fn insert_cancellation_request(
    txn: &Transaction<'_>,
    request: &CancellationRequest,
) -> Result<()> {
    let id_bytes = request.id.as_bytes().to_vec();
    let op_bytes = request.operation_id.as_uuid().as_bytes().to_vec();
    let created_at = request.created_at.to_string();

    txn.conn()
        .execute(
            "INSERT INTO cancellation_requests \
             (id, operation_id, requested_by, reason, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                id_bytes,
                op_bytes,
                &request.requested_by,
                request.reason.as_ref(),
                created_at,
            ],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
    Ok(())
}

pub fn transition_operation(
    txn: &Transaction<'_>,
    id: OperationId,
    target: autore_core::operation::OperationState,
    failure: Option<OperationFailure>,
) -> Result<()> {
    let id_bytes = id.as_uuid().as_bytes().to_vec();

    let current_state_str: String = txn
        .conn()
        .query_row(
            "SELECT state FROM operations WHERE id = ?1",
            rusqlite::params![id_bytes],
            |row| row.get(0),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                Error::NotFound(format!("operation {id} not found"))
            }
            other => Error::Database(other.to_string()),
        })?;

    let current_state = autore_core::operation::operation_state_from_str(&current_state_str)
        .map_err(Error::Database)?;
    current_state.transition(&target)?;

    let failure_json = failure
        .as_ref()
        .map(|f| serde_json::to_string(f).map_err(|e| Error::Serialization(e.to_string())))
        .transpose()?;
    let updated_at = Timestamp::now().to_string();

    txn.conn()
        .execute(
            "UPDATE operations \
             SET state = ?1, failure = COALESCE(?2, failure), updated_at = ?3 \
             WHERE id = ?4",
            rusqlite::params![target.kind(), failure_json, updated_at, id_bytes],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
    Ok(())
}
