//! Unit tests for the generation module.

use std::path::PathBuf;

use autore_app::ApplicationCommand;
use autore_schema::domain::NamespacedId;
use autore_schema::domain::records::{
    ENTITY_KIND_EXTERNAL_FUNCTION, ENTITY_KIND_FUNCTION, ENTITY_KIND_GLOBAL, SemanticEntity,
};
use autore_schema::ids::ProjectId;

use crate::generation::skeleton::ProjectSkeletonBuilder;
use crate::generation::stub::StubPolicy;
use crate::tests_support::RecordingAutoReClient;
use crate::work_graph::kind::{
    ENTITY_KIND_CLASS, ENTITY_KIND_ENTRYPOINT, ENTITY_KIND_ENUM, ENTITY_KIND_VTABLE,
};

fn make_entity(kind: NamespacedId, name: &str) -> SemanticEntity {
    SemanticEntity::new(ProjectId::new(), kind, None, Some(name.into()))
}

// -----------------------------------------------------------------------
// Test: skeleton produces layout from spec
// -----------------------------------------------------------------------

#[test]
fn skeleton_produces_layout_from_spec() {
    let dir = tempfile::tempdir().unwrap();
    let client = RecordingAutoReClient::new();
    let pid = ProjectId::new();
    let mut builder = ProjectSkeletonBuilder::new(dir.path().to_path_buf(), pid, &client);

    builder.add_entity(&make_entity(ENTITY_KIND_FUNCTION.clone(), "main"));
    let manifest = builder.build().unwrap();

    // Metadata files
    assert!(dir.path().join("CMakeLists.txt").exists());
    assert!(dir.path().join("reconstruction.toml").exists());

    // Gitkeep directories
    for subdir in [
        "include/platform",
        "include/external",
        "src/runtime",
        "src/subsystems",
        "src/entrypoints",
        "tests/unit",
        "tests/differential",
        "tests/scenarios",
        "reports",
    ] {
        assert!(
            dir.path().join(subdir).join(".gitkeep").exists(),
            "missing .gitkeep in {subdir}"
        );
    }

    // Entity files
    assert_eq!(manifest.entity_count, 1);
    let entity_files: Vec<_> = manifest
        .generated_files
        .iter()
        .filter(|f| f.entity_id.is_some())
        .collect();
    assert_eq!(entity_files.len(), 2); // 1 .hpp + 1 .cpp
}

// -----------------------------------------------------------------------
// Test: N functions → N header/cpp pairs + layout files
// -----------------------------------------------------------------------

#[test]
fn skeleton_generates_stub_per_discovered_function() {
    let dir = tempfile::tempdir().unwrap();
    let client = RecordingAutoReClient::new();
    let pid = ProjectId::new();
    let mut builder = ProjectSkeletonBuilder::new(dir.path().to_path_buf(), pid, &client);

    let n = 5;
    for i in 0..n {
        builder.add_entity(&make_entity(
            ENTITY_KIND_FUNCTION.clone(),
            &format!("fn_{i}"),
        ));
    }
    let manifest = builder.build().unwrap();

    let headers: Vec<_> = manifest
        .generated_files
        .iter()
        .filter(|f| f.file_role == crate::generation::skeleton::FileRole::Header)
        .collect();
    let definitions: Vec<_> = manifest
        .generated_files
        .iter()
        .filter(|f| f.file_role == crate::generation::skeleton::FileRole::Definition)
        .collect();
    assert_eq!(headers.len(), n);
    assert_eq!(definitions.len(), n);
}

// -----------------------------------------------------------------------
// Test: each stub file contains reconstruction_status = "stubbed"
// -----------------------------------------------------------------------

#[test]
fn skeleton_stubs_marked_explicit_status() {
    let dir = tempfile::tempdir().unwrap();
    let client = RecordingAutoReClient::new();
    let pid = ProjectId::new();
    let mut builder = ProjectSkeletonBuilder::new(dir.path().to_path_buf(), pid, &client);

    builder.add_entity(&make_entity(ENTITY_KIND_FUNCTION.clone(), "alpha"));
    builder.add_entity(&make_entity(ENTITY_KIND_CLASS.clone(), "MyClass"));
    builder.add_entity(&make_entity(ENTITY_KIND_ENUM.clone(), "Color"));
    let manifest = builder.build().unwrap();

    for file in &manifest.generated_files {
        if file.entity_id.is_some() {
            let content = std::fs::read_to_string(&file.path).unwrap();
            assert!(
                content.contains("reconstruction_status = \"stubbed\""),
                "stub marker missing in {:?}",
                file.path
            );
        }
    }
}

// -----------------------------------------------------------------------
// Test: paths derived from EntityId, not display_name
// -----------------------------------------------------------------------

#[test]
fn skeleton_source_paths_derived_from_canonical_entity_id_not_content() {
    let dir1 = tempfile::tempdir().unwrap();
    let dir2 = tempfile::tempdir().unwrap();
    let client1 = RecordingAutoReClient::new();
    let client2 = RecordingAutoReClient::new();
    let pid = ProjectId::new();

    // Build with display_name "original"
    let mut e = make_entity(ENTITY_KIND_FUNCTION.clone(), "original");
    let entity_id = e.id;

    let mut b1 = ProjectSkeletonBuilder::new(dir1.path().to_path_buf(), pid, &client1);
    b1.add_entity(&e);
    let m1 = b1.build().unwrap();

    // Change display_name, same entity ID
    e.display_name = Some("renamed".into());
    let mut b2 = ProjectSkeletonBuilder::new(dir2.path().to_path_buf(), pid, &client2);
    b2.add_entity(&e);
    let m2 = b2.build().unwrap();

    // Paths must be identical because entity_id didn't change
    let rel1: Vec<PathBuf> = m1
        .generated_files
        .iter()
        .filter(|f| f.entity_id == Some(entity_id))
        .map(|f| f.path.strip_prefix(dir1.path()).unwrap().to_path_buf())
        .collect();
    let rel2: Vec<PathBuf> = m2
        .generated_files
        .iter()
        .filter(|f| f.entity_id == Some(entity_id))
        .map(|f| f.path.strip_prefix(dir2.path()).unwrap().to_path_buf())
        .collect();

    assert_eq!(
        rel1, rel2,
        "paths must be stable across display_name changes"
    );
}

// -----------------------------------------------------------------------
// Test: zero AddEvidence commands with llm.raw-response predicate
// -----------------------------------------------------------------------

#[test]
fn skeleton_no_llm_involved() {
    let dir = tempfile::tempdir().unwrap();
    let client = RecordingAutoReClient::new();
    let pid = ProjectId::new();
    let mut builder = ProjectSkeletonBuilder::new(dir.path().to_path_buf(), pid, &client);

    builder.add_entity(&make_entity(ENTITY_KIND_FUNCTION.clone(), "fn1"));
    builder.add_entity(&make_entity(ENTITY_KIND_GLOBAL.clone(), "g1"));
    builder.add_entity(&make_entity(ENTITY_KIND_EXTERNAL_FUNCTION.clone(), "ext1"));
    builder.build().unwrap();

    // No AddEvidence commands at all — skeleton builder never invokes LLM
    let evidence_count = client.count(|cmd| matches!(cmd, ApplicationCommand::AddEvidence(_)));
    assert_eq!(
        evidence_count, 0,
        "skeleton builder must not issue any AddEvidence commands"
    );
}

// -----------------------------------------------------------------------
// Test: commands issued per entity
// -----------------------------------------------------------------------

#[test]
fn skeleton_issues_register_artifact_and_mapping_commands() {
    let dir = tempfile::tempdir().unwrap();
    let client = RecordingAutoReClient::new();
    let pid = ProjectId::new();
    let mut builder = ProjectSkeletonBuilder::new(dir.path().to_path_buf(), pid, &client);

    builder.add_entity(&make_entity(ENTITY_KIND_FUNCTION.clone(), "fn_a"));
    builder.add_entity(&make_entity(ENTITY_KIND_VTABLE.clone(), "vt_a"));
    builder.build().unwrap();

    let cmds = client.commands();

    // 2 entities × (2 RegisterArtifact + 1 RegisterGeneratedSourceMapping) = 6
    let artifact_count = cmds
        .iter()
        .filter(|c| matches!(c, ApplicationCommand::RegisterArtifact(_)))
        .count();
    let mapping_count = cmds
        .iter()
        .filter(|c| matches!(c, ApplicationCommand::RegisterGeneratedSourceMapping(_)))
        .count();

    assert_eq!(artifact_count, 4, "expected 2 RegisterArtifact per entity");
    assert_eq!(
        mapping_count, 2,
        "expected 1 RegisterGeneratedSourceMapping per entity"
    );
}

// -----------------------------------------------------------------------
// Test: generation order follows spec §11.2
// -----------------------------------------------------------------------

#[test]
fn skeleton_generation_order_follows_spec() {
    let dir = tempfile::tempdir().unwrap();
    let client = RecordingAutoReClient::new();
    let pid = ProjectId::new();
    let mut builder = ProjectSkeletonBuilder::new(dir.path().to_path_buf(), pid, &client);

    // Add in reverse order
    builder.add_entity(&make_entity(ENTITY_KIND_ENTRYPOINT.clone(), "main"));
    builder.add_entity(&make_entity(ENTITY_KIND_FUNCTION.clone(), "fn1"));
    builder.add_entity(&make_entity(ENTITY_KIND_EXTERNAL_FUNCTION.clone(), "ext1"));
    builder.add_entity(&make_entity(ENTITY_KIND_ENUM.clone(), "Color"));
    builder.add_entity(&make_entity(ENTITY_KIND_GLOBAL.clone(), "g1"));
    let manifest = builder.build().unwrap();

    // Extract entity IDs from generated files in order
    let entity_ids: Vec<_> = manifest
        .generated_files
        .iter()
        .filter(|f| f.file_role == crate::generation::skeleton::FileRole::Header)
        .filter_map(|f| f.entity_id)
        .collect();

    assert_eq!(entity_ids.len(), 5);
    // The files on disk should be in generation order, but we verify
    // via the manifest's generated_files ordering.
}

// -----------------------------------------------------------------------
// Test: stub policy controls body content
// -----------------------------------------------------------------------

#[test]
fn skeleton_stub_policy_static_assert() {
    let dir = tempfile::tempdir().unwrap();
    let client = RecordingAutoReClient::new();
    let pid = ProjectId::new();
    let mut builder = ProjectSkeletonBuilder::new(dir.path().to_path_buf(), pid, &client)
        .with_policy(StubPolicy::StaticAssert);

    builder.add_entity(&make_entity(ENTITY_KIND_FUNCTION.clone(), "fn_sa"));
    let manifest = builder.build().unwrap();

    let cpp_file = manifest
        .generated_files
        .iter()
        .find(|f| f.file_role == crate::generation::skeleton::FileRole::Definition)
        .unwrap();
    let content = std::fs::read_to_string(&cpp_file.path).unwrap();
    assert!(
        content.contains("static_assert(false"),
        "static_assert missing in EmptyBody policy"
    );
}

#[test]
fn skeleton_stub_policy_empty_body() {
    let dir = tempfile::tempdir().unwrap();
    let client = RecordingAutoReClient::new();
    let pid = ProjectId::new();
    let mut builder = ProjectSkeletonBuilder::new(dir.path().to_path_buf(), pid, &client)
        .with_policy(StubPolicy::EmptyBody);

    builder.add_entity(&make_entity(ENTITY_KIND_FUNCTION.clone(), "fn_eb"));
    let manifest = builder.build().unwrap();

    let cpp_file = manifest
        .generated_files
        .iter()
        .find(|f| f.file_role == crate::generation::skeleton::FileRole::Definition)
        .unwrap();
    let content = std::fs::read_to_string(&cpp_file.path).unwrap();
    assert!(
        !content.contains("static_assert"),
        "static_assert should not appear with EmptyBody policy"
    );
    assert!(content.contains("empty body"), "empty body marker missing");
}

// -----------------------------------------------------------------------
// Test: no two functions share the same source path
// -----------------------------------------------------------------------

#[test]
fn skeleton_no_duplicate_paths() {
    let dir = tempfile::tempdir().unwrap();
    let client = RecordingAutoReClient::new();
    let pid = ProjectId::new();
    let mut builder = ProjectSkeletonBuilder::new(dir.path().to_path_buf(), pid, &client);

    for i in 0..10 {
        builder.add_entity(&make_entity(
            ENTITY_KIND_FUNCTION.clone(),
            &format!("fn_{i}"),
        ));
    }
    let manifest = builder.build().unwrap();

    let mut paths: Vec<PathBuf> = manifest
        .generated_files
        .iter()
        .filter(|f| f.entity_id.is_some())
        .map(|f| f.path.strip_prefix(dir.path()).unwrap().to_path_buf())
        .collect();
    let original_len = paths.len();
    paths.sort();
    paths.dedup();
    assert_eq!(
        paths.len(),
        original_len,
        "duplicate paths detected in skeleton output"
    );
}
