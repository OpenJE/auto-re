use std::path::PathBuf;
use std::sync::Arc;

use autore_schema::domain::records::{
    Artifact, CancellationRequest, Contradiction, EvidenceLifecycleEvent, EvidenceRecord,
    Hypothesis, NativeArtifact, Operation, ProgressUpdate, Provider, ProviderEntityAlias,
    ProviderRun, SemanticEntity, VerificationRecord,
};
use autore_schema::domain::{Confidence, NamespacedId};
use autore_schema::ids::{
    ArtifactId, ContradictionId, EntityId, EvidenceRecordId, HypothesisId, NativeArtifactId,
    OperationId, ProjectId, ProviderId, ProviderRunId, VerificationRecordId,
};
use autore_store::{
    ArtifactStore, ContradictionStore, Database, EntityPage, EntityStore, EvidenceStore,
    HypothesisStore, NativeArtifactStore, OperationStore, Page, ProjectStore, ProviderAliasStore,
    ProviderStore, RunQuery, SqliteAliasStore, SqliteArtifactStore, SqliteContradictionStore,
    SqliteEntityStore, SqliteEvidenceStore, SqliteHypothesisStore, SqliteOperationStore,
    SqliteProjectStore, SqliteProviderStore, SqliteVerificationStore, VerificationStore,
};

macro_rules! delegate_store {
    ($wrapper:ident, $trait:ty, $inner:path, $($method:ident($($arg:ident: $ty:ty),*) $(-> $ret:ty)?);* $(;)?) => {
        impl $trait for $wrapper {
            $(
                fn $method(&self, $($arg: $ty),*) $(-> $ret)? {
                    let store = $inner(&self.0);
                    store.$method($($arg),*)
                }
            )*
        }
    };
}

pub struct ProjectStoreImpl(Arc<Database>);

impl ProjectStoreImpl {
    pub fn new(db: Arc<Database>) -> Self {
        Self(db)
    }
}

delegate_store!(ProjectStoreImpl, ProjectStore, SqliteProjectStore::new,
    insert_project(p: &autore_schema::domain::records::Project) -> crate::Result<()>;
    get_project(id: ProjectId) -> crate::Result<Option<autore_schema::domain::records::Project>>;
    list_projects(page: Page) -> crate::Result<Vec<autore_schema::domain::records::Project>>;
);

pub struct EntityStoreImpl(Arc<Database>);

impl EntityStoreImpl {
    pub fn new(db: Arc<Database>) -> Self {
        Self(db)
    }
}

delegate_store!(EntityStoreImpl, EntityStore, SqliteEntityStore::new,
    insert(entity: &SemanticEntity) -> crate::Result<()>;
    get(id: EntityId) -> crate::Result<Option<SemanticEntity>>;
    list_by_project(project_id: ProjectId, page: EntityPage, kind_filter: Option<&NamespacedId>) -> crate::Result<Vec<SemanticEntity>>;
    list_by_stable_key(project_id: ProjectId, stable_key: &autore_schema::domain::StableEntityKey) -> crate::Result<Vec<SemanticEntity>>;
    count_by_project_kind(project_id: ProjectId, kind: &NamespacedId) -> crate::Result<u64>;
    register_kind(kind: &NamespacedId);
);

pub struct ProviderStoreImpl(Arc<Database>);

impl ProviderStoreImpl {
    pub fn new(db: Arc<Database>) -> Self {
        Self(db)
    }
}

delegate_store!(ProviderStoreImpl, ProviderStore, SqliteProviderStore::new,
    insert_provider(provider: &Provider) -> crate::Result<()>;
    get_provider(id: ProviderId) -> crate::Result<Option<Provider>>;
    list_providers() -> crate::Result<Vec<Provider>>;
    start_run(run: &ProviderRun) -> crate::Result<()>;
    complete_run(run_id: ProviderRunId, target: autore_schema::domain::records::ProviderRunStatus) -> crate::Result<()>;
    get_run(id: ProviderRunId) -> crate::Result<Option<ProviderRun>>;
    list_runs(query: RunQuery) -> crate::Result<Vec<ProviderRun>>;
);

pub struct EvidenceStoreImpl(Arc<Database>);

impl EvidenceStoreImpl {
    pub fn new(db: Arc<Database>) -> Self {
        Self(db)
    }
}

delegate_store!(EvidenceStoreImpl, EvidenceStore, SqliteEvidenceStore::new,
    insert_evidence(record: &EvidenceRecord) -> crate::Result<()>;
    get_evidence(id: EvidenceRecordId) -> crate::Result<Option<EvidenceRecord>>;
    list_by_project(project_id: ProjectId) -> crate::Result<Vec<EvidenceRecord>>;
    list_by_subject(subject: EntityId) -> crate::Result<Vec<EvidenceRecord>>;
    list_by_provider_run(run_id: ProviderRunId) -> crate::Result<Vec<EvidenceRecord>>;
    record_lifecycle_event(event: &EvidenceLifecycleEvent) -> crate::Result<()>;
    list_lifecycle_for_evidence(evidence_id: EvidenceRecordId) -> crate::Result<Vec<EvidenceLifecycleEvent>>;
);

pub struct HypothesisStoreImpl(Arc<Database>);

impl HypothesisStoreImpl {
    pub fn new(db: Arc<Database>) -> Self {
        Self(db)
    }
}

delegate_store!(HypothesisStoreImpl, HypothesisStore, SqliteHypothesisStore::new,
    insert(hypothesis: &Hypothesis) -> crate::Result<()>;
    get(id: HypothesisId) -> crate::Result<Option<Hypothesis>>;
    list_by_project(project_id: ProjectId) -> crate::Result<Vec<Hypothesis>>;
    list_by_subject(subject: EntityId) -> crate::Result<Vec<Hypothesis>>;
    list_by_status(project_id: ProjectId, status_kind: &str) -> crate::Result<Vec<Hypothesis>>;
    update_status(id: HypothesisId, target: autore_schema::domain::records::HypothesisStatus) -> crate::Result<()>;
    update_confidence(id: HypothesisId, confidence: Confidence) -> crate::Result<()>;
    get_competing(subject: EntityId, predicate: &NamespacedId) -> crate::Result<Vec<Hypothesis>>;
);

pub struct ContradictionStoreImpl(Arc<Database>);

impl ContradictionStoreImpl {
    pub fn new(db: Arc<Database>) -> Self {
        Self(db)
    }
}

delegate_store!(ContradictionStoreImpl, ContradictionStore, SqliteContradictionStore::new,
    insert(contradiction: &Contradiction) -> crate::Result<()>;
    get(id: ContradictionId) -> crate::Result<Option<Contradiction>>;
    list_by_project(project_id: ProjectId) -> crate::Result<Vec<Contradiction>>;
    list_by_subject(subject: EntityId) -> crate::Result<Vec<Contradiction>>;
    list_by_status(project_id: ProjectId, status_kind: &str) -> crate::Result<Vec<Contradiction>>;
    resolve(id: ContradictionId, resolution: autore_schema::domain::records::ContradictionResolution) -> crate::Result<()>;
);

pub struct VerificationStoreImpl(Arc<Database>);

impl VerificationStoreImpl {
    pub fn new(db: Arc<Database>) -> Self {
        Self(db)
    }
}

delegate_store!(VerificationStoreImpl, VerificationStore, SqliteVerificationStore::new,
    insert(record: &VerificationRecord) -> crate::Result<()>;
    get(id: VerificationRecordId) -> crate::Result<Option<VerificationRecord>>;
    list_by_project(project_id: ProjectId) -> crate::Result<Vec<VerificationRecord>>;
    list_by_subject(subject: autore_schema::domain::records::VerificationSubject) -> crate::Result<Vec<VerificationRecord>>;
    list_by_check(check: &NamespacedId) -> crate::Result<Vec<VerificationRecord>>;
    multi_check_per_subject_supported() -> bool;
);

pub struct OperationStoreImpl(Arc<Database>);

impl OperationStoreImpl {
    pub fn new(db: Arc<Database>) -> Self {
        Self(db)
    }
}

delegate_store!(OperationStoreImpl, OperationStore, SqliteOperationStore::new,
    insert(operation: &Operation) -> crate::Result<()>;
    get(id: OperationId) -> crate::Result<Option<Operation>>;
    list_by_project(project_id: ProjectId) -> crate::Result<Vec<Operation>>;
    list_by_state(project_id: ProjectId, state: autore_core::operation::OperationState) -> crate::Result<Vec<Operation>>;
    transition(id: OperationId, target: autore_core::operation::OperationState, failure: Option<autore_schema::domain::records::OperationFailure>) -> crate::Result<()>;
    record_progress(update: &ProgressUpdate) -> crate::Result<()>;
    list_progress(operation_id: OperationId) -> crate::Result<Vec<ProgressUpdate>>;
    request_cancellation(request: &CancellationRequest) -> crate::Result<()>;
    list_cancellation_requests(operation_id: OperationId) -> crate::Result<Vec<CancellationRequest>>;
);

pub struct ArtifactStoreImpl {
    db: Arc<Database>,
    base_dir: PathBuf,
}

impl ArtifactStoreImpl {
    pub fn new(db: Arc<Database>, base_dir: impl Into<PathBuf>) -> Self {
        Self {
            db,
            base_dir: base_dir.into(),
        }
    }
}

impl ArtifactStore for ArtifactStoreImpl {
    fn register_managed(
        &self,
        project_id: ProjectId,
        source_path: &std::path::Path,
        kind: NamespacedId,
    ) -> crate::Result<Artifact> {
        SqliteArtifactStore::new(&self.db, self.base_dir.clone()).register_managed(
            project_id,
            source_path,
            kind,
        )
    }

    fn register_managed_blake3(
        &self,
        project_id: ProjectId,
        source_path: &std::path::Path,
        kind: NamespacedId,
    ) -> crate::Result<Artifact> {
        SqliteArtifactStore::new(&self.db, self.base_dir.clone()).register_managed_blake3(
            project_id,
            source_path,
            kind,
        )
    }

    fn register_external(
        &self,
        project_id: ProjectId,
        canonical_path: &std::path::Path,
        kind: NamespacedId,
    ) -> crate::Result<Artifact> {
        SqliteArtifactStore::new(&self.db, self.base_dir.clone()).register_external(
            project_id,
            canonical_path,
            kind,
        )
    }

    fn verify_artifact(
        &self,
        project_id: ProjectId,
        artifact: &Artifact,
    ) -> crate::Result<autore_store::ArtifactIntegrity> {
        SqliteArtifactStore::new(&self.db, self.base_dir.clone())
            .verify_artifact(project_id, artifact)
    }

    fn read_managed_blob(
        &self,
        project_id: ProjectId,
        artifact: &Artifact,
    ) -> crate::Result<Vec<u8>> {
        SqliteArtifactStore::new(&self.db, self.base_dir.clone())
            .read_managed_blob(project_id, artifact)
    }

    fn get_artifact(&self, id: ArtifactId) -> crate::Result<Option<Artifact>> {
        SqliteArtifactStore::new(&self.db, self.base_dir.clone()).get_artifact(id)
    }

    fn list_by_project(&self, project_id: ProjectId) -> crate::Result<Vec<Artifact>> {
        SqliteArtifactStore::new(&self.db, self.base_dir.clone()).list_by_project(project_id)
    }
}

pub struct NativeArtifactStoreImpl(Arc<Database>);

impl NativeArtifactStoreImpl {
    pub fn new(db: Arc<Database>) -> Self {
        Self(db)
    }
}

impl NativeArtifactStore for NativeArtifactStoreImpl {
    fn insert(&self, artifact: &NativeArtifact) -> crate::Result<()> {
        SqliteAliasStore::new(&self.0).insert(artifact)
    }

    fn get(&self, id: NativeArtifactId) -> crate::Result<Option<NativeArtifact>> {
        SqliteAliasStore::new(&self.0).get(id)
    }

    fn list_by_run(&self, run_id: ProviderRunId) -> crate::Result<Vec<NativeArtifact>> {
        SqliteAliasStore::new(&self.0).list_by_run(run_id)
    }

    fn list_by_subject_entity(&self, entity_id: EntityId) -> crate::Result<Vec<NativeArtifact>> {
        SqliteAliasStore::new(&self.0).list_by_subject_entity(entity_id)
    }
}

pub struct ProviderAliasStoreImpl(Arc<Database>);

impl ProviderAliasStoreImpl {
    pub fn new(db: Arc<Database>) -> Self {
        Self(db)
    }
}

impl ProviderAliasStore for ProviderAliasStoreImpl {
    fn insert_alias(&self, alias: &ProviderEntityAlias) -> crate::Result<()> {
        SqliteAliasStore::new(&self.0).insert_alias(alias)
    }

    fn list_aliases_for_run(
        &self,
        run_id: ProviderRunId,
    ) -> crate::Result<Vec<ProviderEntityAlias>> {
        SqliteAliasStore::new(&self.0).list_aliases_for_run(run_id)
    }

    fn find_alias(
        &self,
        run_id: ProviderRunId,
        provider_identifier: &str,
    ) -> crate::Result<Option<ProviderEntityAlias>> {
        SqliteAliasStore::new(&self.0).find_alias(run_id, provider_identifier)
    }

    fn list_aliases_for_entity(
        &self,
        entity_id: EntityId,
    ) -> crate::Result<Vec<ProviderEntityAlias>> {
        SqliteAliasStore::new(&self.0).list_aliases_for_entity(entity_id)
    }
}
