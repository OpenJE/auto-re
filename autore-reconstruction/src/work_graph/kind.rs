//! Dependency edge kinds and entity-kind constants for work-graph construction.

use autore_schema::domain::NamespacedId;
use autore_schema::domain::records::WorkItemKind;

// ---------------------------------------------------------------------------
// Entity kind constants (not yet in autore-schema)
// ---------------------------------------------------------------------------

/// Entity kind: a C++ class (synthetic or from debug info).
pub static ENTITY_KIND_CLASS: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.class").unwrap());

/// Entity kind: a virtual-function table.
pub static ENTITY_KIND_VTABLE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.vtable").unwrap());

/// Entity kind: an enumeration type.
pub static ENTITY_KIND_ENUM: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.enum").unwrap());

/// Entity kind: a static initializer / global constructor.
pub static ENTITY_KIND_STATIC_INITIALIZER: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.static-initializer").unwrap());

/// Entity kind: an address-space entrypoint (e.g. `main`, TLS init).
pub static ENTITY_KIND_ENTRYPOINT: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.entrypoint").unwrap());

// ---------------------------------------------------------------------------
// DependencyEdgeKind (§7.3)
// ---------------------------------------------------------------------------

/// The kind of dependency edge between two work items in the reconstruction
/// graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DependencyEdgeKind {
    DirectCall,
    IndirectCallHypothesis,
    TypeUsage,
    GlobalAccess,
    VtableMembership,
    CtorDtor,
    StaticInit,
    GeneratedDeclRequirement,
    BuildDependency,
    VerificationDependency,
    /// Synthetic edge from a `FunctionCluster` to each member function.
    ClusterMember,
}

impl std::fmt::Display for DependencyEdgeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            DependencyEdgeKind::DirectCall => "direct-call",
            DependencyEdgeKind::IndirectCallHypothesis => "indirect-call-hypothesis",
            DependencyEdgeKind::TypeUsage => "type-usage",
            DependencyEdgeKind::GlobalAccess => "global-access",
            DependencyEdgeKind::VtableMembership => "vtable-membership",
            DependencyEdgeKind::CtorDtor => "ctor-dtor",
            DependencyEdgeKind::StaticInit => "static-init",
            DependencyEdgeKind::GeneratedDeclRequirement => "generated-decl-requirement",
            DependencyEdgeKind::BuildDependency => "build-dependency",
            DependencyEdgeKind::VerificationDependency => "verification-dependency",
            DependencyEdgeKind::ClusterMember => "cluster-member",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------------------
// Entity-kind → WorkItemKind mapping
// ---------------------------------------------------------------------------

/// Returns the [`WorkItemKind`] that corresponds to the given entity kind,
/// or `None` if the entity kind does not map to a work-item kind.
pub fn work_item_kind_for_entity_kind(kind: &NamespacedId) -> Option<WorkItemKind> {
    use autore_schema::domain::records::{
        ENTITY_KIND_EXTERNAL_FUNCTION, ENTITY_KIND_FUNCTION, ENTITY_KIND_GLOBAL, ENTITY_KIND_TYPE,
    };
    if *kind == *ENTITY_KIND_FUNCTION {
        Some(WorkItemKind::Function)
    } else if *kind == *ENTITY_KIND_TYPE {
        Some(WorkItemKind::Structure)
    } else if *kind == *ENTITY_KIND_CLASS {
        Some(WorkItemKind::Class)
    } else if *kind == *ENTITY_KIND_VTABLE {
        Some(WorkItemKind::Vtable)
    } else if *kind == *ENTITY_KIND_GLOBAL {
        Some(WorkItemKind::Global)
    } else if *kind == *ENTITY_KIND_ENUM {
        Some(WorkItemKind::Enum)
    } else if *kind == *ENTITY_KIND_EXTERNAL_FUNCTION {
        Some(WorkItemKind::ExternalDependency)
    } else if *kind == *ENTITY_KIND_STATIC_INITIALIZER {
        Some(WorkItemKind::StaticInitializer)
    } else if *kind == *ENTITY_KIND_ENTRYPOINT {
        Some(WorkItemKind::Entrypoint)
    } else {
        None
    }
}
