//! Claim entity — an assertion or finding about a binary entity.
//!
//! Claims are the core unit of analysis output. Each claim asserts a
//! `ClaimPredicate` about a `ClaimSubject` with a `ClaimValue`, supported
//! by `Evidence` and tracked through its `ClaimState`.

use crate::domain::{Confidence, EntityId, Provenance};
use crate::ids::{ClaimId, EvidenceId, FunctionId, WorkerRunId};
use crate::worker::output::{FunctionAnalysisOutput, ProposedClaim};

// ---------------------------------------------------------------------------
// ClaimState
// ---------------------------------------------------------------------------

/// The lifecycle state of a claim.
///
/// Valid transitions:
/// - `Proposed` → `UnderReview`
/// - `UnderReview` → `Accepted` | `Rejected`
/// - `Accepted` → `Superseded`
/// - Any → `Invalidated`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ClaimState {
    /// Claim has been created but not yet reviewed.
    Proposed,
    /// Claim is under review by a verifier.
    UnderReview,
    /// Claim has been verified and accepted.
    Accepted,
    /// Claim has been reviewed and rejected.
    Rejected,
    /// Claim was accepted but has been superseded by a newer claim.
    Superseded,
    /// Claim has been invalidated (e.g., the underlying analysis was wrong).
    Invalidated,
}

impl ClaimState {
    /// Returns `true` if this is a final state (no further productive transitions).
    pub fn is_final(&self) -> bool {
        matches!(
            self,
            ClaimState::Accepted
                | ClaimState::Rejected
                | ClaimState::Superseded
                | ClaimState::Invalidated
        )
    }
}

// ---------------------------------------------------------------------------
// ClaimPredicate
// ---------------------------------------------------------------------------

/// The predicate of a claim — what property is being asserted.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ClaimPredicate {
    /// The function's name.
    FunctionName,
    /// The function's signature (parameter types, return type).
    FunctionSignature,
    /// The function's entry address.
    FunctionAddress,
    /// The function's size (byte range).
    FunctionSize,
    /// Type information for a local variable or parameter.
    TypeRecovery,
    /// Layout of a struct or class.
    StructureLayout,
    /// The calling convention used.
    CallingConvention,
    /// Control-flow graph structure.
    ControlFlowGraph,
    /// Data-flow fact (e.g., taint, constant propagation).
    DataFlowFact,
    /// A cross-reference (caller/callee relationship).
    CrossReference,
    /// A string reference at a given address.
    StringReference,
    /// A global variable reference.
    GlobalReference,
    /// A comment annotation.
    Comment,
    /// A runtime observation (e.g., from tracing or debugger).
    RuntimeObservation,
    /// An assertion about the reimplementation's correctness.
    ReimplementationCorrectness,
    /// Test harness output or status.
    TestResult,
    /// A custom predicate.
    Custom(String),
}

// ---------------------------------------------------------------------------
// ClaimValue
// ---------------------------------------------------------------------------

/// The value of a claim — what is being asserted about the predicate.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ClaimValue {
    /// A string value (names, signatures, annotations).
    String(String),
    /// An integer value (addresses, sizes, counts).
    Integer(u64),
    /// A floating-point value.
    Float(f64),
    /// A boolean assertion.
    Boolean(bool),
    /// A set of bytes (e.g., a byte-level pattern).
    Bytes(Vec<u8>),
    /// A type descriptor.
    TypeDescriptor(String),
    /// A set of related values (e.g., calling convention details).
    Map(Vec<(String, String)>),
    /// A JSON value for structured or complex data.
    Json(serde_json::Value),
}

// ---------------------------------------------------------------------------
// Claim
// ---------------------------------------------------------------------------

/// An assertion or finding about a binary entity.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Claim {
    /// Unique identifier for this claim.
    pub id: ClaimId,
    /// The entity this claim is about.
    pub subject: EntityId,
    /// What property is being asserted.
    pub predicate: ClaimPredicate,
    /// The asserted value.
    pub value: ClaimValue,
    /// Current lifecycle state.
    pub state: ClaimState,
    /// Confidence level of this claim.
    pub confidence: Confidence,
    /// How this claim was produced.
    pub provenance: Provenance,
    /// IDs of evidence supporting this claim.
    pub evidence: Vec<EvidenceId>,
    /// IDs of claims this one depends on.
    pub dependencies: Vec<ClaimId>,
}

impl Claim {
    /// Creates a new claim in the `Proposed` state.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ClaimId,
        subject: EntityId,
        predicate: ClaimPredicate,
        value: ClaimValue,
        confidence: Confidence,
        provenance: Provenance,
    ) -> Self {
        Claim {
            id,
            subject,
            predicate,
            value,
            state: ClaimState::Proposed,
            confidence,
            provenance,
            evidence: Vec::new(),
            dependencies: Vec::new(),
        }
    }

    // -----------------------------------------------------------------------
    // State transitions
    // -----------------------------------------------------------------------

    /// Transitions `Proposed` → `UnderReview`.
    pub fn submit_for_review(&mut self) -> crate::Result<()> {
        match self.state {
            ClaimState::Proposed => {
                self.state = ClaimState::UnderReview;
                Ok(())
            }
            _ => Err(crate::Error::Validation(format!(
                "cannot submit claim {:?} for review in state {:?}",
                self.id, self.state
            ))),
        }
    }

    /// Transitions `UnderReview` → `Accepted`.
    pub fn accept(&mut self) -> crate::Result<()> {
        match self.state {
            ClaimState::UnderReview => {
                self.state = ClaimState::Accepted;
                Ok(())
            }
            _ => Err(crate::Error::Validation(format!(
                "cannot accept claim {:?} in state {:?}",
                self.id, self.state
            ))),
        }
    }

    /// Transitions `UnderReview` → `Rejected`.
    pub fn reject(&mut self) -> crate::Result<()> {
        match self.state {
            ClaimState::UnderReview => {
                self.state = ClaimState::Rejected;
                Ok(())
            }
            _ => Err(crate::Error::Validation(format!(
                "cannot reject claim {:?} in state {:?}",
                self.id, self.state
            ))),
        }
    }

    /// Transitions `Accepted` → `Superseded`.
    pub fn supersede(&mut self) -> crate::Result<()> {
        match self.state {
            ClaimState::Accepted => {
                self.state = ClaimState::Superseded;
                Ok(())
            }
            _ => Err(crate::Error::Validation(format!(
                "cannot supersede claim {:?} in state {:?}",
                self.id, self.state
            ))),
        }
    }

    /// Transitions from any state to `Invalidated`.
    pub fn invalidate(&mut self) -> crate::Result<()> {
        if self.state == ClaimState::Invalidated {
            return Err(crate::Error::Validation(format!(
                "claim {:?} is already invalidated",
                self.id
            )));
        }
        self.state = ClaimState::Invalidated;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Evidence management
    // -----------------------------------------------------------------------

    /// Links an evidence ID to this claim.
    pub fn link_evidence(&mut self, evidence_id: EvidenceId) {
        if !self.evidence.contains(&evidence_id) {
            self.evidence.push(evidence_id);
        }
    }

    /// Removes a previously linked evidence ID.
    pub fn unlink_evidence(&mut self, evidence_id: &EvidenceId) {
        self.evidence.retain(|e| e != evidence_id);
    }

    /// Adds a dependency on another claim.
    pub fn add_dependency(&mut self, claim_id: ClaimId) {
        if !self.dependencies.contains(&claim_id) && claim_id != self.id {
            self.dependencies.push(claim_id);
        }
    }

    // -----------------------------------------------------------------------
    // Conversion from worker output
    // -----------------------------------------------------------------------

    /// Creates a `Claim` in `Proposed` state from a worker's `ProposedClaim`.
    pub fn from_proposed(
        function_id: FunctionId,
        proposed: ProposedClaim,
        worker_run_id: WorkerRunId,
    ) -> crate::Result<Self> {
        Ok(Claim::new(
            ClaimId::new(),
            EntityId::Function(function_id),
            proposed.predicate,
            proposed.value,
            proposed.confidence,
            Provenance::Agent { worker_run_id },
        ))
    }

    /// Converts all proposed claims in a `FunctionAnalysisOutput` into `Claim`
    /// entities in `Proposed` state, resolving intra-output dependencies by
    /// matching `ClaimPredicate` values.
    pub fn from_worker_output(
        function_id: FunctionId,
        output: &FunctionAnalysisOutput,
        worker_run_id: WorkerRunId,
    ) -> crate::Result<Vec<Self>> {
        // First pass: create all claims.
        let mut claims: Vec<Claim> = output
            .claims
            .iter()
            .map(|pc| {
                Claim::new(
                    ClaimId::new(),
                    EntityId::Function(function_id),
                    pc.predicate.clone(),
                    pc.value.clone(),
                    pc.confidence,
                    Provenance::Agent { worker_run_id },
                )
            })
            .collect();

        // Build predicate → ClaimId map for dependency resolution.
        let predicate_to_id: std::collections::HashMap<ClaimPredicate, ClaimId> =
            claims.iter().map(|c| (c.predicate.clone(), c.id)).collect();

        // Second pass: resolve dependencies.
        for (claim, proposed) in claims.iter_mut().zip(output.claims.iter()) {
            for dep_predicate in &proposed.dependencies {
                if let Some(&dep_id) = predicate_to_id.get(dep_predicate) {
                    claim.add_dependency(dep_id);
                }
            }
        }

        Ok(claims)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Provenance;
    use crate::ids::ClaimId;
    use crate::ids::{EvidenceId, FunctionId};

    fn sample_claim() -> Claim {
        Claim::new(
            ClaimId::new(),
            EntityId::Function(FunctionId::new()),
            ClaimPredicate::FunctionName,
            ClaimValue::String("main".into()),
            Confidence::new(0.95).unwrap(),
            Provenance::StaticAnalysis,
        )
    }

    // -- State transition tests --

    #[test]
    fn claim_starts_proposed() {
        let c = sample_claim();
        assert_eq!(c.state, ClaimState::Proposed);
        assert!(!c.state.is_final());
    }

    #[test]
    fn claim_full_acceptance_lifecycle() {
        let mut c = sample_claim();
        c.submit_for_review().unwrap();
        assert_eq!(c.state, ClaimState::UnderReview);

        c.accept().unwrap();
        assert_eq!(c.state, ClaimState::Accepted);
        assert!(c.state.is_final());
    }

    #[test]
    fn claim_full_rejection_lifecycle() {
        let mut c = sample_claim();
        c.submit_for_review().unwrap();
        c.reject().unwrap();
        assert_eq!(c.state, ClaimState::Rejected);
        assert!(c.state.is_final());
    }

    #[test]
    fn claim_state_transitions() {
        // Proposed → UnderReview → Accepted → Superseded
        let mut c = sample_claim();
        c.submit_for_review().unwrap();
        c.accept().unwrap();
        c.supersede().unwrap();
        assert_eq!(c.state, ClaimState::Superseded);
        assert!(c.state.is_final());

        // Any → Invalidated
        let mut c2 = sample_claim();
        c2.invalidate().unwrap();
        assert_eq!(c2.state, ClaimState::Invalidated);

        // Invalidated from Proposed directly
        let mut c3 = sample_claim();
        c3.invalidate().unwrap();
        assert!(c3.state.is_final());
    }

    #[test]
    fn claim_rejects_invalid_transitions() {
        // Cannot accept from Proposed
        let mut c = sample_claim();
        assert!(c.accept().is_err());

        // Cannot reject from Proposed
        let mut c2 = sample_claim();
        assert!(c2.reject().is_err());

        // Cannot supersede from Proposed
        let mut c3 = sample_claim();
        assert!(c3.supersede().is_err());

        // Cannot submit for review twice
        let mut c4 = sample_claim();
        c4.submit_for_review().unwrap();
        assert!(c4.submit_for_review().is_err());

        // Cannot double-invalidate
        let mut c5 = sample_claim();
        c5.invalidate().unwrap();
        assert!(c5.invalidate().is_err());
    }

    // -- Evidence linking tests --

    #[test]
    fn claim_evidence_link() {
        let mut c = sample_claim();
        let ev_id = EvidenceId::new();
        assert!(c.evidence.is_empty());
        c.link_evidence(ev_id);
        assert_eq!(c.evidence.len(), 1);
        assert_eq!(c.evidence[0], ev_id);
    }

    #[test]
    fn claim_evidence_is_deduplicated() {
        let mut c = sample_claim();
        let ev_id = EvidenceId::new();
        c.link_evidence(ev_id);
        c.link_evidence(ev_id); // duplicate
        assert_eq!(c.evidence.len(), 1);
    }

    #[test]
    fn claim_unlink_evidence() {
        let mut c = sample_claim();
        let ev_id = EvidenceId::new();
        c.link_evidence(ev_id);
        c.unlink_evidence(&ev_id);
        assert!(c.evidence.is_empty());
    }

    // -- Dependency tests --

    #[test]
    fn claim_add_dependency() {
        let mut c = sample_claim();
        let dep_id = ClaimId::new();
        c.add_dependency(dep_id);
        assert_eq!(c.dependencies.len(), 1);
        // Self-dependency is rejected
        c.add_dependency(c.id);
        assert_eq!(c.dependencies.len(), 1);
    }

    #[test]
    fn claim_dependency_deduplicated() {
        let mut c = sample_claim();
        let dep_id = ClaimId::new();
        c.add_dependency(dep_id);
        c.add_dependency(dep_id);
        assert_eq!(c.dependencies.len(), 1);
    }

    // -- Serialization tests --

    #[test]
    fn claim_serialize_roundtrip() {
        let mut c = sample_claim();
        c.submit_for_review().unwrap();
        let json = serde_json::to_string(&c).unwrap();
        let deserialized: Claim = serde_json::from_str(&json).unwrap();
        assert_eq!(c.id, deserialized.id);
        assert_eq!(deserialized.state, ClaimState::UnderReview);
    }

    #[test]
    fn claim_with_complex_value() {
        let c = Claim::new(
            ClaimId::new(),
            EntityId::Function(FunctionId::new()),
            ClaimPredicate::FunctionSignature,
            ClaimValue::Json(serde_json::json!({
                "params": ["int", "char*"],
                "returns": "int"
            })),
            Confidence::new(0.8).unwrap(),
            Provenance::StaticAnalysis,
        );
        assert_eq!(c.state, ClaimState::Proposed);
    }

    // -- Worker output conversion tests --

    use crate::domain::{Address, AddressSpace, EvidenceKind, EvidenceLocation, SymbolName};
    use crate::ids::WorkerRunId;
    use crate::worker::output::{FunctionAnalysisOutput, ProposedClaim, ProposedEvidence};

    fn sample_worker_output() -> FunctionAnalysisOutput {
        FunctionAnalysisOutput {
            function_id: FunctionId::new(),
            symbol_name: Some(SymbolName::new("main")),
            address: Address::new(AddressSpace::Virtual, 0x401000),
            confidence: Confidence::new(0.9).unwrap(),
            claims: vec![
                ProposedClaim {
                    predicate: ClaimPredicate::FunctionName,
                    value: ClaimValue::String("main".into()),
                    confidence: Confidence::new(0.95).unwrap(),
                    dependencies: vec![],
                },
                ProposedClaim {
                    predicate: ClaimPredicate::FunctionSignature,
                    value: ClaimValue::String("int main(int, char**)".into()),
                    confidence: Confidence::new(0.8).unwrap(),
                    dependencies: vec![ClaimPredicate::FunctionName],
                },
            ],
            evidence: vec![ProposedEvidence {
                kind: EvidenceKind::Disassembly,
                location: Some(EvidenceLocation::new(
                    Some(Address::new(AddressSpace::Virtual, 0x401000)),
                    None,
                )),
                description: "push rbp; mov rbp, rsp".into(),
                confidence: Confidence::new(0.85).unwrap(),
            }],
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn worker_output_to_proposed_claims() {
        let output = sample_worker_output();
        let function_id = output.function_id;
        let worker_run_id = WorkerRunId::new();

        let claims = Claim::from_worker_output(function_id, &output, worker_run_id).unwrap();

        assert_eq!(claims.len(), 2);
        assert_eq!(claims[0].predicate, ClaimPredicate::FunctionName);
        assert_eq!(claims[0].value, ClaimValue::String("main".into()));
        assert_eq!(claims[1].predicate, ClaimPredicate::FunctionSignature);
    }

    #[test]
    fn claims_start_in_proposed_state() {
        let output = sample_worker_output();
        let function_id = output.function_id;
        let worker_run_id = WorkerRunId::new();

        let claims = Claim::from_worker_output(function_id, &output, worker_run_id).unwrap();

        for claim in &claims {
            assert_eq!(claim.state, ClaimState::Proposed);
            assert_eq!(claim.subject, EntityId::Function(function_id));
            assert_eq!(claim.provenance, Provenance::Agent { worker_run_id });
        }
    }

    #[test]
    fn evidence_links_to_claims() {
        let output = sample_worker_output();
        let function_id = output.function_id;
        let worker_run_id = WorkerRunId::new();

        let evidence =
            crate::domain::Evidence::from_worker_output(function_id, &output, worker_run_id)
                .unwrap();

        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].kind, EvidenceKind::Disassembly);
        assert_eq!(evidence[0].entity, Some(EntityId::Function(function_id)));
        assert_eq!(evidence[0].provenance, Provenance::Agent { worker_run_id });
    }

    #[test]
    fn claim_dependencies_recorded() {
        let output = sample_worker_output();
        let function_id = output.function_id;
        let worker_run_id = WorkerRunId::new();

        let claims = Claim::from_worker_output(function_id, &output, worker_run_id).unwrap();

        // First claim (FunctionName) has no dependencies.
        assert!(claims[0].dependencies.is_empty());

        // Second claim (FunctionSignature) depends on FunctionName.
        assert_eq!(claims[1].dependencies.len(), 1);
        assert_eq!(claims[1].dependencies[0], claims[0].id);
    }
}
