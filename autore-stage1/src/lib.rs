pub use autore_schema::{
    domain,
    domain::{
        Address, AddressSpace, ArtifactId, Campaign, CampaignState, Claim, ClaimPredicate,
        ClaimState, ClaimValue, Confidence, ContentHash, EntityId, Evidence, EvidenceKind,
        EvidenceLocation, Function, Provenance, RequiredCapabilities, SymbolName, Task, TaskKind,
        TaskPriority, TaskState, TaskSubject,
    },
    ids,
    ids::{
        BinaryId, BinaryRevisionId, CampaignId, ClaimId, EvidenceId, FunctionId,
        ImplementationTargetId, ModuleId, ProjectId, TaskId, TransactionId, ValidationRunId,
        WorkerRunId,
    },
};
pub mod storage;

mod error;
pub use error::{Error, Result};

#[cfg(feature = "tui")]
pub use autore_events;
#[cfg(feature = "tui")]
pub use autore_tui::{runtime, tui};

pub mod analysis;
pub mod cli;
pub mod model;
pub mod scheduler;
pub mod worker;

#[cfg(feature = "ida")]
mod engine;
#[cfg(feature = "ida")]
mod store;
