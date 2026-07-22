//! Deterministic project skeleton builder.
//!
//! [`ProjectSkeletonBuilder`] takes a set of [`SemanticEntity`] objects
//! and emits a managed `generated/openvb/` source tree with explicit
//! stub files per canonical entity. Every generated file is registered
//! as an artifact via `RegisterArtifact`, and each entity gets a
//! `RegisterGeneratedSourceMapping` command linking it to the generated
//! declaration and definition artifacts.
//!
//! Source paths are derived from the canonical [`EntityId`] UUID —
//! never from `display_name` or content-derived names.

use std::fs;
use std::path::{Path, PathBuf};

use autore_app::application_service::requests::RegisterArtifactRequest;
use autore_app::{ApplicationCommand, AutoReClient};
use autore_schema::domain::records::SemanticEntity;
use autore_schema::ids::{EntityId, ProjectId};

use super::mapping::GeneratedSourceMappingIntent;
use super::stub::{
    self, StubPolicy, entity_id_to_relpath, generation_order, render_cmake,
    render_reconstruction_toml,
};

/// A single generated file in the skeleton output tree.
#[derive(Debug, Clone)]
pub struct GeneratedFile {
    pub path: PathBuf,
    pub entity_id: Option<EntityId>,
    pub file_role: FileRole,
}

/// The role of a generated file within the skeleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRole {
    Header,
    Definition,
    Metadata,
    GitKeep,
}

/// The manifest produced by [`ProjectSkeletonBuilder::build`].
#[derive(Debug, Clone)]
pub struct SkeletonManifest {
    pub output_path: PathBuf,
    pub project_id: ProjectId,
    pub generated_files: Vec<GeneratedFile>,
    pub entity_count: usize,
}

/// Builds a deterministic project skeleton from canonical entities.
///
/// The builder collects entities, sorts them by generation order
/// (spec §11.2), writes stub files, and issues `RegisterArtifact`
/// and `RegisterGeneratedSourceMapping` commands for each entity.
pub struct ProjectSkeletonBuilder<'a> {
    output_path: PathBuf,
    project_id: ProjectId,
    client: &'a dyn AutoReClient,
    entities: Vec<SemanticEntity>,
    policy: StubPolicy,
}

impl<'a> ProjectSkeletonBuilder<'a> {
    pub fn new(output_path: PathBuf, project_id: ProjectId, client: &'a dyn AutoReClient) -> Self {
        Self {
            output_path,
            project_id,
            client,
            entities: Vec::new(),
            policy: StubPolicy::StaticAssert,
        }
    }

    /// Override the default stub policy.
    pub fn with_policy(mut self, policy: StubPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Register one canonical entity for skeleton generation.
    pub fn add_entity(&mut self, entity: &SemanticEntity) {
        self.entities.push(entity.clone());
    }

    /// Write the skeleton tree to disk and issue commands.
    pub fn build(mut self) -> autore_core::Result<SkeletonManifest> {
        self.entities.sort_by(|a, b| {
            generation_order(&a.kind)
                .cmp(&generation_order(&b.kind))
                .then_with(|| a.id.cmp(&b.id))
        });

        let mut generated_files = Vec::new();

        self.create_directories()?;
        self.write_gitkeeps(&mut generated_files)?;
        self.write_metadata(&mut generated_files)?;

        for entity in &self.entities {
            self.write_entity_stubs(entity, &mut generated_files)?;
        }

        for entity in &self.entities {
            self.issue_commands(entity)?;
        }

        Ok(SkeletonManifest {
            output_path: self.output_path,
            project_id: self.project_id,
            generated_files,
            entity_count: self.entities.len(),
        })
    }

    // -- directory structure --

    fn create_directories(&self) -> autore_core::Result<()> {
        let dirs = [
            "include/recovered",
            "include/platform",
            "include/external",
            "src/runtime",
            "src/generated",
            "src/subsystems",
            "src/entrypoints",
            "tests/unit",
            "tests/differential",
            "tests/scenarios",
            "reports",
        ];
        for dir in &dirs {
            fs::create_dir_all(self.output_path.join(dir))?;
        }
        Ok(())
    }

    fn write_gitkeeps(&self, out: &mut Vec<GeneratedFile>) -> autore_core::Result<()> {
        let gitkeep_dirs = [
            "include/platform",
            "include/external",
            "src/runtime",
            "src/subsystems",
            "src/entrypoints",
            "tests/unit",
            "tests/differential",
            "tests/scenarios",
            "reports",
        ];
        for dir in &gitkeep_dirs {
            let path = self.output_path.join(dir).join(".gitkeep");
            fs::write(&path, "")?;
            out.push(GeneratedFile {
                path,
                entity_id: None,
                file_role: FileRole::GitKeep,
            });
        }
        Ok(())
    }

    fn write_metadata(&self, out: &mut Vec<GeneratedFile>) -> autore_core::Result<()> {
        // CMakeLists.txt
        let cmake_path = self.output_path.join("CMakeLists.txt");
        let cmake_content = render_cmake(&self.entities);
        fs::write(&cmake_path, &cmake_content)?;
        out.push(GeneratedFile {
            path: cmake_path,
            entity_id: None,
            file_role: FileRole::Metadata,
        });

        // reconstruction.toml
        let toml_path = self.output_path.join("reconstruction.toml");
        let toml_content =
            render_reconstruction_toml(self.project_id, self.entities.len(), self.policy);
        fs::write(&toml_path, &toml_content)?;
        out.push(GeneratedFile {
            path: toml_path,
            entity_id: None,
            file_role: FileRole::Metadata,
        });

        Ok(())
    }

    // -- entity stubs --

    fn write_entity_stubs(
        &self,
        entity: &SemanticEntity,
        out: &mut Vec<GeneratedFile>,
    ) -> autore_core::Result<()> {
        let rel = entity_id_to_relpath(&entity.id);

        // Header
        let hpp_path = self
            .output_path
            .join("include/recovered")
            .join(&rel)
            .with_extension("hpp");
        if let Some(parent) = hpp_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let hpp_content = stub::render_stub_header(&entity.id, &entity.kind, self.policy);
        fs::write(&hpp_path, &hpp_content)?;
        out.push(GeneratedFile {
            path: hpp_path,
            entity_id: Some(entity.id),
            file_role: FileRole::Header,
        });

        // Definition
        let cpp_path = self
            .output_path
            .join("src/generated")
            .join(&rel)
            .with_extension("cpp");
        if let Some(parent) = cpp_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let cpp_content = stub::render_stub_cpp(&entity.id, &entity.kind, self.policy);
        fs::write(&cpp_path, &cpp_content)?;
        out.push(GeneratedFile {
            path: cpp_path,
            entity_id: Some(entity.id),
            file_role: FileRole::Definition,
        });

        Ok(())
    }

    // -- commands --

    fn issue_commands(&self, entity: &SemanticEntity) -> autore_core::Result<()> {
        let rel = entity_id_to_relpath(&entity.id);
        let hpp_rel = Path::new("include/recovered")
            .join(&rel)
            .with_extension("hpp");
        let cpp_rel = Path::new("src/generated").join(&rel).with_extension("cpp");

        // RegisterArtifact for .hpp
        self.client.execute(ApplicationCommand::RegisterArtifact(
            RegisterArtifactRequest {
                project: self.project_id,
                source_path: hpp_rel,
                kind: "core.generated-candidate".to_string(),
            },
        ))?;

        // RegisterArtifact for .cpp
        self.client.execute(ApplicationCommand::RegisterArtifact(
            RegisterArtifactRequest {
                project: self.project_id,
                source_path: cpp_rel,
                kind: "core.generated-candidate".to_string(),
            },
        ))?;

        // RegisterGeneratedSourceMapping
        let intent = GeneratedSourceMappingIntent {
            project: self.project_id,
            entity_id: entity.id,
            declaration_path: self
                .output_path
                .join("include/recovered")
                .join(&rel)
                .with_extension("hpp"),
            definition_path: self
                .output_path
                .join("src/generated")
                .join(&rel)
                .with_extension("cpp"),
        };
        intent.execute(self.client)?;

        Ok(())
    }
}
