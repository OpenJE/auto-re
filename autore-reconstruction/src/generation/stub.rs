//! Stub rendering policy for generated source files.
//!
//! Every generated `.hpp` and `.cpp` contains an explicit marker
//! indicating it is a reconstruction stub. The [`StubPolicy`] enum
//! controls whether function bodies use `static_assert(false, ...)`
//! or empty bodies to avoid compile contamination.

use std::path::PathBuf;

use autore_schema::domain::NamespacedId;
use autore_schema::domain::records::{
    ENTITY_KIND_EXTERNAL_FUNCTION, ENTITY_KIND_FUNCTION, ENTITY_KIND_GLOBAL, ENTITY_KIND_TYPE,
    SemanticEntity,
};
use autore_schema::ids::{EntityId, ProjectId};

use crate::work_graph::kind::{
    ENTITY_KIND_CLASS, ENTITY_KIND_ENTRYPOINT, ENTITY_KIND_ENUM, ENTITY_KIND_STATIC_INITIALIZER,
    ENTITY_KIND_VTABLE,
};

/// Controls how function stub bodies are rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StubPolicy {
    /// Function bodies contain `static_assert(false, ...)` so that
    /// compilation fails loudly if the stub is called.
    StaticAssert,
    /// Function bodies are empty — compiles but produces no-op
    /// definitions. Used when compile contamination would break the
    /// skeleton build.
    EmptyBody,
}

/// Renders the standard header comment present in every stub file.
pub fn render_stub_comment(entity_id: &EntityId) -> String {
    format!(
        "// AUTO-RE RECONSTRUCTION STUB\n\
         // entity-id: {entity_id}\n\
         // [[reconstruction_status = \"stubbed\"]]\n\
         // Deterministic skeleton stub — not LLM-generated.\n"
    )
}

/// Renders a `.hpp` header stub for the given entity kind.
pub fn render_stub_header(
    entity_id: &EntityId,
    kind: &NamespacedId,
    _policy: StubPolicy,
) -> String {
    let comment = render_stub_comment(entity_id);
    let guard = include_guard(entity_id);
    let decl = header_declaration(entity_id, kind);
    format!(
        "{comment}\n#ifndef {guard}\n#define {guard}\n\n\
         // reconstruction_status = \"stubbed\"\n\n\
         {decl}\n\n#endif // {guard}\n"
    )
}

/// Renders a `.cpp` definition stub for the given entity kind.
pub fn render_stub_cpp(entity_id: &EntityId, kind: &NamespacedId, policy: StubPolicy) -> String {
    let comment = render_stub_comment(entity_id);
    let hex = entity_id_hex(entity_id);
    let include = format!(
        "#include \"recovered/{}/{}/{}/{}.hpp\"",
        &hex[0..2],
        &hex[2..4],
        &hex[4..6],
        hex
    );
    let body = cpp_body(entity_id, kind, policy);
    format!("{comment}\n{include}\n\n{body}\n")
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn entity_id_hex(entity_id: &EntityId) -> String {
    entity_id.as_uuid().as_simple().to_string()
}

fn short_id(entity_id: &EntityId) -> String {
    entity_id_hex(entity_id)[..8].to_string()
}

fn include_guard(entity_id: &EntityId) -> String {
    format!(
        "AUTORE_STUB_{}_HPP",
        entity_id_hex(entity_id).to_uppercase()
    )
}

fn header_declaration(entity_id: &EntityId, kind: &NamespacedId) -> String {
    let sid = short_id(entity_id);
    if *kind == *ENTITY_KIND_FUNCTION || *kind == *ENTITY_KIND_ENTRYPOINT {
        format!("void autore_stub_{sid}(void);")
    } else if *kind == *ENTITY_KIND_EXTERNAL_FUNCTION {
        format!("void autore_stub_{sid}(void); // external — no definition")
    } else if *kind == *ENTITY_KIND_TYPE {
        format!("struct autore_stub_{sid}; // forward declaration")
    } else if *kind == *ENTITY_KIND_CLASS {
        format!("struct autore_stub_{sid} {{ /* reconstruction stub */ }};")
    } else if *kind == *ENTITY_KIND_ENUM {
        format!("enum autore_stub_{sid} {{ AUTORE_STUB_{sid}_PLACEHOLDER = 0 }};")
    } else if *kind == *ENTITY_KIND_GLOBAL {
        format!("extern int autore_stub_{sid};")
    } else if *kind == *ENTITY_KIND_VTABLE {
        format!("struct autore_stub_{sid}_vtable {{ void* entries[1]; }};")
    } else if *kind == *ENTITY_KIND_STATIC_INITIALIZER {
        format!("void autore_stub_{sid}_init(void);")
    } else {
        format!("// unknown entity kind — stub placeholder for {sid}")
    }
}

fn cpp_body(entity_id: &EntityId, kind: &NamespacedId, policy: StubPolicy) -> String {
    let sid = short_id(entity_id);
    if *kind == *ENTITY_KIND_FUNCTION || *kind == *ENTITY_KIND_ENTRYPOINT {
        let inner = match policy {
            StubPolicy::StaticAssert => {
                format!("    static_assert(false, \"reconstruction-stub: {entity_id}\");")
            }
            StubPolicy::EmptyBody => "    // empty body — stub policy: EmptyBody".to_string(),
        };
        format!("void autore_stub_{sid}(void) {{\n{inner}\n}}")
    } else if *kind == *ENTITY_KIND_EXTERNAL_FUNCTION {
        "// external function — no definition generated".to_string()
    } else if *kind == *ENTITY_KIND_GLOBAL {
        format!("int autore_stub_{sid} = 0; // reconstruction stub")
    } else if *kind == *ENTITY_KIND_STATIC_INITIALIZER {
        format!("void autore_stub_{sid}_init(void) {{\n    // static initializer stub\n}}")
    } else if *kind == *ENTITY_KIND_TYPE
        || *kind == *ENTITY_KIND_CLASS
        || *kind == *ENTITY_KIND_ENUM
        || *kind == *ENTITY_KIND_VTABLE
    {
        format!("// type/class/enum/vtable stub — header-only for {sid}")
    } else {
        format!("// unknown entity kind — stub placeholder for {sid}")
    }
}

// ---------------------------------------------------------------------------
// Path derivation and generation order
// ---------------------------------------------------------------------------

/// Derives a stable relative path from the entity's UUID.
///
/// Format: `<2hex>/<2hex>/<2hex>/<full-uuid>`
pub(crate) fn entity_id_to_relpath(entity_id: &EntityId) -> PathBuf {
    let hex = entity_id.as_uuid().as_simple().to_string();
    PathBuf::from(&hex[0..2])
        .join(&hex[2..4])
        .join(&hex[4..6])
        .join(&hex)
}

/// Generation order per spec §11.2.
pub(crate) fn generation_order(kind: &NamespacedId) -> u8 {
    if *kind == *ENTITY_KIND_EXTERNAL_FUNCTION {
        1
    } else if *kind == *ENTITY_KIND_ENUM {
        2
    } else if *kind == *ENTITY_KIND_TYPE {
        3
    } else if *kind == *ENTITY_KIND_GLOBAL {
        4
    } else if *kind == *ENTITY_KIND_FUNCTION {
        5
    } else if *kind == *ENTITY_KIND_CLASS {
        6
    } else if *kind == *ENTITY_KIND_VTABLE {
        7
    } else if *kind == *ENTITY_KIND_STATIC_INITIALIZER {
        8
    } else if *kind == *ENTITY_KIND_ENTRYPOINT {
        9
    } else {
        10
    }
}

// ---------------------------------------------------------------------------
// Metadata rendering
// ---------------------------------------------------------------------------

pub(crate) fn render_cmake(entities: &[SemanticEntity]) -> String {
    let mut out = String::new();
    out.push_str(
        "# AUTO-RE Reconstruction Skeleton\n\
         # Generated deterministically from canonical entity identities.\n\
         # DO NOT EDIT — regenerated by the skeleton builder.\n\n\
         cmake_minimum_required(VERSION 3.20)\n\
         project(autore_reconstruction CXX)\n\n\
         set(CMAKE_CXX_STANDARD 17)\n\
         set(CMAKE_CXX_STANDARD_REQUIRED ON)\n\n",
    );
    if !entities.is_empty() {
        out.push_str("# Generated source files\n");
        for entity in entities {
            out.push_str(&format!("# entity: {id}\n", id = entity.id));
        }
        out.push_str(
            "\nfile(GLOB_RECURSE GENERATED_SOURCES \"src/generated/*.cpp\")\n\
             file(GLOB_RECURSE RECOVERED_HEADERS \"include/recovered/*.hpp\")\n\n\
             add_library(reconstruction_skeleton STATIC ${GENERATED_SOURCES})\n\
             target_include_directories(reconstruction_skeleton PUBLIC include)\n",
        );
    } else {
        out.push_str("# No entities — skeleton contains metadata only.\n");
    }
    out
}

pub(crate) fn render_reconstruction_toml(
    project_id: ProjectId,
    entity_count: usize,
    policy: StubPolicy,
) -> String {
    let policy_str = match policy {
        StubPolicy::StaticAssert => "static-assert",
        StubPolicy::EmptyBody => "empty-body",
    };
    format!(
        "# AUTO-RE Reconstruction Manifest\n\
         schema_version = \"1.0\"\n\
         project_id = \"{project_id}\"\n\
         generator = \"autore-reconstruction::generation::ProjectSkeletonBuilder\"\n\
         entity_count = {entity_count}\n\
         stub_policy = \"{policy_str}\"\n"
    )
}
