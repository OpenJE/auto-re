pub use autore_core::{Error, Result};
pub use autore_events;
pub use autore_schema::{domain, ids};
pub use autore_store::storage;

pub mod lifecycle;

#[cfg(test)]
mod operation;

pub use lifecycle::{close_project, create_project, open_project, open_project_client};

pub mod application_service;

pub use application_service::{
    ApplicationCommand, ApplicationQuery, ApplicationService, AutoReClient, CommandResult,
    LocalAutoReClient, QueryResult,
};

pub use application_service::requests::{
    AddEvidenceRequest, AddEvidenceResponse, AddHypothesisRequest, AddHypothesisResponse,
    AddVerificationRequest, AddVerificationResponse, ArtifactResponse, ArtifactsResponse,
    CancelOperationRequest, CancelOperationResponse, ChangeHypothesisStatusRequest,
    ChangeHypothesisStatusResponse, ContradictionResponse, ContradictionsResponse,
    CreateProjectRequest, CreateProjectResponse, EntitiesResponse, EntityResponse, EventsResponse,
    EvidenceListResponse, EvidenceResponse, GetArtifactQuery, GetContradictionQuery,
    GetEntityQuery, GetEvidenceQuery, GetHypothesisQuery, GetOperationQuery,
    GetProjectSummaryQuery, GetProviderQuery, GetProviderRunQuery, GetValidationReportQuery,
    GetVerificationQuery, HypothesesResponse, HypothesisResponse, ListArtifactsQuery,
    ListContradictionsQuery, ListEntitiesQuery, ListEventsQuery, ListEvidenceQuery,
    ListHypothesesQuery, ListOperationsQuery, ListProviderRunsQuery, ListProvidersQuery,
    ListVerificationsQuery, MigrateProjectRequest, MigrateProjectResponse, OperationResponse,
    OperationsResponse, ProviderResponse, ProviderRunResponse, ProviderRunsResponse,
    ProvidersResponse, RebuildIndexesRequest, RebuildIndexesResponse, RecordContradictionRequest,
    RecordContradictionResponse, RegisterArtifactRequest, RegisterArtifactResponse,
    RegisterEntityRequest, RegisterEntityResponse, RegisterProviderRequest,
    RegisterProviderResponse, StartProviderRunRequest, StartProviderRunResponse,
    ValidateProjectRequest, ValidateProjectResponse, ValidationFinding, ValidationReport,
    ValidationReportResponse, ValidationResult, ValidationSeverity, VerificationResponse,
    VerificationsResponse,
};
