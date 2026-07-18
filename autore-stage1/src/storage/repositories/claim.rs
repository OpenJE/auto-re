//! SQLite implementation of `ClaimRepository`.
//!
//! `SqliteClaimRepository` provides persistent storage for `Claim` entities
//! using `rusqlite`. Complex domain types are JSON-encoded.

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::{OptionalExtension, params};

use crate::domain::{
    Claim, ClaimPredicate, ClaimState, ClaimValue, Confidence, EntityId, Provenance,
};
use crate::ids::{ClaimId, EvidenceId};
use crate::storage::Database;
use crate::storage::repositories::ClaimRepository;

/// SQLite-backed claim repository.
pub struct SqliteClaimRepository {
    database: Arc<Database>,
}

impl SqliteClaimRepository {
    /// Creates a new repository backed by the given database.
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn claim_state_to_str(state: &ClaimState) -> &'static str {
    match state {
        ClaimState::Proposed => "Proposed",
        ClaimState::UnderReview => "UnderReview",
        ClaimState::Accepted => "Accepted",
        ClaimState::Rejected => "Rejected",
        ClaimState::Superseded => "Superseded",
        ClaimState::Invalidated => "Invalidated",
    }
}

fn claim_state_from_str(s: &str) -> ClaimState {
    match s {
        "Proposed" => ClaimState::Proposed,
        "UnderReview" => ClaimState::UnderReview,
        "Accepted" => ClaimState::Accepted,
        "Rejected" => ClaimState::Rejected,
        "Superseded" => ClaimState::Superseded,
        "Invalidated" => ClaimState::Invalidated,
        _ => ClaimState::Proposed,
    }
}

fn claim_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Claim> {
    let id_str: String = row.get(0)?;
    let subject_json: String = row.get(1)?;
    let predicate_json: String = row.get(2)?;
    let value_json: String = row.get(3)?;
    let state_str: String = row.get(4)?;
    let confidence_val: f64 = row.get(5)?;
    let provenance_json: String = row.get(6)?;
    let evidence_json: String = row.get(7)?;
    let deps_json: String = row.get(8)?;

    let id = ClaimId::from_uuid(uuid::Uuid::parse_str(&id_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?);

    let subject: EntityId = serde_json::from_str(&subject_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let predicate: ClaimPredicate = serde_json::from_str(&predicate_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let value: ClaimValue = serde_json::from_str(&value_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let provenance: Provenance = serde_json::from_str(&provenance_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let evidence: Vec<EvidenceId> = serde_json::from_str::<Vec<uuid::Uuid>>(&evidence_json)
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e))
        })?
        .into_iter()
        .map(EvidenceId::from_uuid)
        .collect();

    let dependencies: Vec<ClaimId> = serde_json::from_str::<Vec<uuid::Uuid>>(&deps_json)
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
        })?
        .into_iter()
        .map(ClaimId::from_uuid)
        .collect();

    let confidence = Confidence::new(confidence_val as f32).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Real,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )),
        )
    })?;

    let mut claim = Claim::new(id, subject, predicate, value, confidence, provenance);
    claim.state = claim_state_from_str(&state_str);
    claim.evidence = evidence;
    claim.dependencies = dependencies;
    Ok(claim)
}

// ---------------------------------------------------------------------------
// Trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl ClaimRepository for SqliteClaimRepository {
    async fn create(&self, claim: &Claim) -> autore_core::Result<ClaimId> {
        let conn = self.database.connection()?;
        let subject_json = serde_json::to_string(&claim.subject)
            .map_err(|e| autore_core::Error::Database(e.to_string()))?;
        let predicate_json = serde_json::to_string(&claim.predicate)
            .map_err(|e| autore_core::Error::Database(e.to_string()))?;
        let value_json = serde_json::to_string(&claim.value)
            .map_err(|e| autore_core::Error::Database(e.to_string()))?;
        let provenance_json = serde_json::to_string(&claim.provenance)
            .map_err(|e| autore_core::Error::Database(e.to_string()))?;
        let evidence_uuids: Vec<uuid::Uuid> = claim.evidence.iter().map(|e| *e.as_uuid()).collect();
        let evidence_json = serde_json::to_string(&evidence_uuids)
            .map_err(|e| autore_core::Error::Database(e.to_string()))?;
        let dep_uuids: Vec<uuid::Uuid> = claim.dependencies.iter().map(|d| *d.as_uuid()).collect();
        let deps_json =
            serde_json::to_string(&dep_uuids).map_err(|e| autore_core::Error::Database(e.to_string()))?;

        conn.execute(
            "INSERT INTO claims (id, subject, predicate, value, state, confidence, provenance, evidence, dependencies) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                claim.id.to_string(),
                subject_json,
                predicate_json,
                value_json,
                claim_state_to_str(&claim.state),
                f64::from(claim.confidence.value()),
                provenance_json,
                evidence_json,
                deps_json,
            ],
        )
        .map_err(|e| autore_core::Error::Database(e.to_string()))?;
        Ok(claim.id)
    }

    async fn find_by_id(&self, id: ClaimId) -> autore_core::Result<Option<Claim>> {
        let conn = self.database.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, subject, predicate, value, state, confidence, provenance, evidence, dependencies \
                 FROM claims WHERE id = ?1",
            )
            .map_err(|e| autore_core::Error::Database(e.to_string()))?;
        let result = stmt
            .query_row([id.to_string()], claim_from_row)
            .optional()
            .map_err(|e| autore_core::Error::Database(e.to_string()))?;
        Ok(result)
    }
}
