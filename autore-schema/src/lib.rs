pub mod domain;
pub mod ids;
pub mod manifest;
pub mod worker_output;

pub use domain::*;
pub use ids::{
    BinaryArtifactId, BinaryId, BinaryRevisionId, BuildAttemptId, BuildDiagnosticId, CampaignId,
    CanonicalTypeHypothesisId, CapabilityDescriptorId, ClaimId, ConflictRecordId, ContradictionId,
    DynamicObservationId, EvidenceId, FunctionId, GeneratedSourceMappingId, GenerationTargetId,
    HypothesisId, ImplementationTargetId, ModuleId, NativeArtifactId, OperationId, PackageId,
    ParsedLlmResultId, ProjectEventId, ProjectId, ProviderId, ProviderInstallationId,
    ProviderInstanceId, ProviderRunId, RawLlmResponseId, ReconstructionCampaignId, RepairAttemptId,
    SourceArtifactId, TaskId, TransactionId, ValidationRunId, VerificationComparisonId,
    VerificationRecordId, VerificationScenarioId, WorkItemId, WorkerRunId,
};
pub use manifest::ProjectManifest;
pub use worker_output::{FunctionAnalysisOutput, ProposedClaim, ProposedEvidence, validate_output};

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    #[test]
    fn content_hash_sha256_deterministic() {
        // SHA-256 of the same input bytes must always produce the same output.
        let input = b"fn main() { println!(\"hello\"); }";
        let hash1 = Sha256::digest(input);
        let hash2 = Sha256::digest(input);
        assert_eq!(
            hash1, hash2,
            "SHA-256 must be deterministic for identical inputs"
        );

        // Known vector: SHA-256 of empty input.
        let empty_hash = Sha256::digest(b"");
        let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(
            format!("{:x}", empty_hash),
            expected,
            "SHA-256 of empty input must match known NIST vector"
        );

        // Different inputs must produce different hashes.
        let a = Sha256::digest(b"alpha");
        let b = Sha256::digest(b"beta");
        assert_ne!(
            a, b,
            "different inputs should produce different SHA-256 hashes"
        );
    }
}
