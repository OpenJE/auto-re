//! Routing helpers: observation kind ↔ entity kind ↔ work-item kind.

use autore_schema::domain::NamespacedId;
use autore_schema::domain::records::{
    ENTITY_KIND_EXTERNAL_FUNCTION, ENTITY_KIND_FUNCTION, ENTITY_KIND_GLOBAL,
    ENTITY_KIND_SOURCE_SYMBOL, ENTITY_KIND_STRING, ENTITY_KIND_TYPE, WorkItemKind,
};

/// Maps a provider observation kind segment (e.g. `"functions"` from
/// `ida.ingest.functions`) to the corresponding `core.*` entity kind.
pub fn entity_kind_from_observation(kind_segment: &str) -> NamespacedId {
    match kind_segment {
        "functions" | "function" => ENTITY_KIND_FUNCTION.clone(),
        "types" | "type" | "structs" | "struct" => ENTITY_KIND_TYPE.clone(),
        "globals" | "global" => ENTITY_KIND_GLOBAL.clone(),
        "strings" | "string" => ENTITY_KIND_STRING.clone(),
        "imports" | "externals" | "external-functions" => ENTITY_KIND_EXTERNAL_FUNCTION.clone(),
        _ => ENTITY_KIND_SOURCE_SYMBOL.clone(),
    }
}

/// Picks the canonical entity kind for an entire observation kind string.
///
/// Walks `.`-separated segments from right to left, returning the first
/// segment that resolves to a known `core.*` entity kind. Falls back to
/// `core.source-symbol` when no segment is recognised.
pub fn entity_kind_for_observation_kind(observation_kind: &str) -> NamespacedId {
    for segment in observation_kind.rsplit('.') {
        let candidate = entity_kind_from_observation(segment);
        let is_source_symbol_fallback =
            candidate == *ENTITY_KIND_SOURCE_SYMBOL && segment != "strings" && segment != "string";
        if !is_source_symbol_fallback {
            return candidate;
        }
    }
    ENTITY_KIND_SOURCE_SYMBOL.clone()
}

/// Maps an entity kind to the corresponding [`WorkItemKind`] for the
/// `Investigation` work item spawned on stale observations.
pub fn work_item_kind_for_entity(entity_kind: &NamespacedId) -> WorkItemKind {
    if *entity_kind == *ENTITY_KIND_FUNCTION {
        WorkItemKind::Function
    } else if *entity_kind == *ENTITY_KIND_TYPE {
        WorkItemKind::Structure
    } else if *entity_kind == *ENTITY_KIND_GLOBAL {
        WorkItemKind::Global
    } else if *entity_kind == *ENTITY_KIND_STRING {
        WorkItemKind::Investigation
    } else if *entity_kind == *ENTITY_KIND_EXTERNAL_FUNCTION {
        WorkItemKind::ExternalDependency
    } else {
        WorkItemKind::Investigation
    }
}
