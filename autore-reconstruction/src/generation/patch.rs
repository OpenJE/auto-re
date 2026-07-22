//! Controlled staged source-patching pipeline (spec §11.5).
//!
//! Every candidate change is validated, staged, syntax-checked, diffed,
//! imported through the canonical `ImportGeneratedSourceCandidates` command,
//! built, and then either accepted (registered as artifacts + mappings) or
//! rolled back (prior source restored, staged data discarded).
//!
//! The pipeline never writes canonical data before a successful build.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use autore_app::application_service::requests::{
    ImportGeneratedSourceCandidatesRequest, RegisterArtifactRequest,
    RegisterGeneratedSourceMappingRequest,
};
use autore_app::{ApplicationCommand, AutoReClient};
use autore_schema::ids::{EntityId, ProjectId};

use crate::build::{
    BuildLogs, BuildProviderError, BuildProviderTrait, CompileUnit, GeneratorManifest,
};

const MAX_CANDIDATE_BYTES: usize = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Candidate + outcome
// ---------------------------------------------------------------------------

/// A single proposed change to a generated source file.
#[derive(Debug, Clone)]
pub struct CandidatePatch {
    /// Path relative to the generated project root (e.g.
    /// `src/generated/aa/bb/cc/aabbcc.../entity.cpp`).
    pub relative_path: PathBuf,
    /// New file content the model proposes.
    pub new_content_bytes: Vec<u8>,
    /// Content of the file before the patch, used for rollback + diff.
    pub prior_content_bytes: Vec<u8>,
    /// Evidence references that justify this change (trait (c) placeholder).
    pub source_evidence_refs: Vec<String>,
}

/// Result of a patch attempt.
#[derive(Debug, Clone)]
pub struct PatchOutcome {
    /// Whether the patch was accepted.
    pub accepted: bool,
    /// Whether the build step reported success.
    pub build_success: bool,
    /// Staging directory that held candidate bytes (removed on rollback).
    pub staging_dir: Option<PathBuf>,
    /// Unified diff text produced for each candidate.
    pub diff_texts: Vec<String>,
    /// Typed diagnostics collected from the build step.
    pub diagnostics: Vec<crate::build::types::BuildDiagnostic>,
}

/// Result of a build invocation inside the patch pipeline.
#[derive(Debug, Clone)]
pub struct BuildOutcome {
    /// Whether the build succeeded.
    pub success: bool,
    /// Typed diagnostics collected from the build logs.
    pub diagnostics: Vec<crate::build::types::BuildDiagnostic>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Failure modes of the controlled patch pipeline.
#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    /// A candidate path is empty.
    #[error("blank target path")]
    BlankPath,
    /// A candidate path escapes the generated source tree.
    #[error("target path {0:?} is outside the generated source tree")]
    PathOutsideGeneratedTree(PathBuf),
    /// A candidate requests deletion of a file not declared for deletion.
    #[error("deletion of undeclared file: {0:?}")]
    UndeclaredDeletion(PathBuf),
    /// A candidate's content exceeds the binary-output limit.
    #[error("content exceeds {MAX_CANDIDATE_BYTES} bytes for {0:?}")]
    ContentTooLarge(PathBuf),
    /// A path segment identifies the auto-re repository itself.
    #[error("path contains auto-re segment: {0:?}")]
    AutoReRepoPath(PathBuf),
    /// A path is outside the work item's source-path prefix.
    #[error("path unrelated to work item: {0:?}")]
    UnrelatedToWorkItem(PathBuf),
    /// The lightweight C++ syntax checker rejected the candidate.
    #[error("syntax check failed for {0:?}: {1}")]
    SyntaxCheckFailed(PathBuf, String),
    /// The build provider returned an error.
    #[error("build provider error: {0}")]
    Build(#[from] BuildProviderError),
    /// File system error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Application command error.
    #[error("application error: {0}")]
    Application(#[from] autore_core::Error),
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// Orchestrates validate → stage → syntax-check → diff → import → build → accept/rollback.
pub struct PatchPipeline<'a> {
    output_root: PathBuf,
    project_id: ProjectId,
    build_provider: &'a dyn BuildProviderTrait,
    client: &'a dyn AutoReClient,
}

impl<'a> PatchPipeline<'a> {
    /// Creates a new patch pipeline.
    pub fn new(
        output_root: PathBuf,
        project_id: ProjectId,
        build_provider: &'a dyn BuildProviderTrait,
        client: &'a dyn AutoReClient,
    ) -> Self {
        Self {
            output_root,
            project_id,
            build_provider,
            client,
        }
    }

    /// Validates all candidate target paths and content.
    ///
    /// `declared_paths` contains the relative paths already known to the work
    /// item; deleting a path outside this set is rejected.
    /// `subject_entity` scopes paths to the entity's generated source prefix.
    pub fn validate_file_targets(
        &self,
        candidates: &[CandidatePatch],
        declared_paths: &HashSet<PathBuf>,
        subject_entity: EntityId,
    ) -> Result<(), PatchError> {
        let allowed_prefixes: Vec<PathBuf> = allowed_source_prefixes(&subject_entity);

        for candidate in candidates {
            let rel = &candidate.relative_path;

            // Blank path.
            if rel.as_os_str().is_empty() {
                return Err(PatchError::BlankPath);
            }

            // No absolute paths.
            if rel.is_absolute() {
                return Err(PatchError::PathOutsideGeneratedTree(rel.clone()));
            }

            let rel_str = rel.to_string_lossy();

            // Guard against touching the auto-re repo itself.
            if rel_str.contains("auto-re/") || rel.components().any(|c| c.as_os_str() == "auto-re")
            {
                return Err(PatchError::AutoReRepoPath(rel.clone()));
            }

            // Must be inside the generated source tree.
            if !is_under_generated_tree(rel) {
                return Err(PatchError::PathOutsideGeneratedTree(rel.clone()));
            }

            // Must be related to the assigned work item.
            if !allowed_prefixes
                .iter()
                .any(|prefix| rel.starts_with(prefix))
                && !declared_paths.contains(rel)
            {
                return Err(PatchError::UnrelatedToWorkItem(rel.clone()));
            }

            // Size limit.
            if candidate.new_content_bytes.len() > MAX_CANDIDATE_BYTES {
                return Err(PatchError::ContentTooLarge(rel.clone()));
            }

            // Undeclared deletion: empty payload for a path not declared.
            if candidate.new_content_bytes.is_empty() && !declared_paths.contains(rel) {
                return Err(PatchError::UndeclaredDeletion(rel.clone()));
            }
        }

        Ok(())
    }

    /// Writes candidate bytes into a temporary staging directory.
    pub async fn stage_candidate_artifacts(
        &self,
        candidates: &[CandidatePatch],
    ) -> Result<(PathBuf, Vec<PathBuf>), PatchError> {
        let staging_dir = self
            .output_root
            .join(".staging")
            .join(format!("patch-{}", uuid::Uuid::now_v7()));
        tokio::fs::create_dir_all(&staging_dir).await?;

        let mut staged_paths = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let staged_path = staging_dir.join(&candidate.relative_path);
            if let Some(parent) = staged_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&staged_path, &candidate.new_content_bytes).await?;
            staged_paths.push(staged_path);
        }

        Ok((staging_dir, staged_paths))
    }

    /// Lightweight C++ surface syntax check.
    ///
    /// `clang` is not required in this environment; we use a deterministic
    /// brace/paren/quote balance validator that catches obviously malformed
    /// output. This is documented as a limitation.
    pub fn parse_or_syntax_check(&self, candidates: &[CandidatePatch]) -> Result<(), PatchError> {
        for candidate in candidates {
            if let Err(msg) = syntax_check_cpp(&candidate.new_content_bytes) {
                return Err(PatchError::SyntaxCheckFailed(
                    candidate.relative_path.clone(),
                    msg,
                ));
            }
        }
        Ok(())
    }

    /// Builds a unified diff for each candidate against its prior content.
    pub fn construct_controlled_patch(
        &self,
        candidates: &[CandidatePatch],
    ) -> Result<Vec<String>, PatchError> {
        candidates
            .iter()
            .map(|c| {
                Ok(unified_diff(
                    &c.prior_content_bytes,
                    &c.new_content_bytes,
                    &c.relative_path,
                ))
            })
            .collect()
    }

    /// Applies the candidates to the generated project tree and issues the
    /// canonical `ImportGeneratedSourceCandidates` command.
    pub async fn apply_through_generated_project_manager(
        &self,
        candidates: &[CandidatePatch],
    ) -> Result<(), PatchError> {
        for candidate in candidates {
            let target = self.output_root.join(&candidate.relative_path);
            if let Some(parent) = target.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&target, &candidate.new_content_bytes).await?;
        }

        let candidate_paths: Vec<String> = candidates
            .iter()
            .map(|c| c.relative_path.to_string_lossy().into_owned())
            .collect();

        self.client
            .execute(ApplicationCommand::ImportGeneratedSourceCandidates(
                ImportGeneratedSourceCandidatesRequest {
                    project: self.project_id,
                    candidates: candidate_paths,
                },
            ))?;

        Ok(())
    }

    /// Runs the configured build provider over the generated project.
    pub async fn build(&self) -> Result<BuildOutcome, PatchError> {
        let source_files = discover_cpp_source_files(&self.output_root)?;
        if source_files.is_empty() {
            return Ok(BuildOutcome {
                success: false,
                diagnostics: Vec::new(),
            });
        }

        let manifest = GeneratorManifest {
            project_root: self.output_root.clone(),
            cmake_generator: "NMake Makefiles".into(),
            source_files: source_files.clone(),
            executable_target: "reconstruction".into(),
        };

        let configured = self
            .build_provider
            .configure_project(&manifest, &self.output_root)
            .await?;

        let units: Vec<CompileUnit> = source_files
            .iter()
            .map(|src| CompileUnit {
                source_path: src.clone(),
                object_path: PathBuf::from("build")
                    .join(src.file_stem().unwrap_or_default())
                    .with_extension("obj"),
            })
            .collect();

        let compiled = self.build_provider.compile_units(&units).await?;
        let linked = self.build_provider.link_target(&compiled.objects).await?;

        let success = configured.success && compiled.success && linked.success;
        let diagnostics = if success {
            Vec::new()
        } else {
            let logs = BuildLogs {
                stdout: format!(
                    "{}\n{}\n{}",
                    configured.stdout, compiled.stdout, linked.stdout
                ),
                stderr: format!(
                    "{}\n{}\n{}",
                    configured.stderr, compiled.stderr, linked.stderr
                ),
            };
            self.build_provider
                .collect_diagnostics(&logs)
                .await
                .unwrap_or_default()
        };

        Ok(BuildOutcome {
            success,
            diagnostics,
        })
    }

    /// Finalizes the patch on build success, otherwise restores prior source
    /// and discards staged artifacts.
    pub async fn accept_or_roll_back(
        &self,
        candidates: &[CandidatePatch],
        build_success: bool,
        staging_dir: &Path,
        subject_entity: EntityId,
        diagnostics: Vec<crate::build::types::BuildDiagnostic>,
    ) -> Result<PatchOutcome, PatchError> {
        let diff_texts = self.construct_controlled_patch(candidates)?;

        if build_success {
            for candidate in candidates {
                self.client.execute(ApplicationCommand::RegisterArtifact(
                    RegisterArtifactRequest {
                        project: self.project_id,
                        source_path: candidate.relative_path.clone(),
                        kind: "core.generated-candidate".to_string(),
                    },
                ))?;
            }

            self.client
                .execute(ApplicationCommand::RegisterGeneratedSourceMapping(
                    RegisterGeneratedSourceMappingRequest {
                        project: self.project_id,
                        work_item_id: subject_entity.to_string(),
                    },
                ))?;

            // Staging data is no longer needed after acceptance.
            let _ = tokio::fs::remove_dir_all(staging_dir).await;

            Ok(PatchOutcome {
                accepted: true,
                build_success: true,
                staging_dir: None,
                diff_texts,
                diagnostics,
            })
        } else {
            // Roll back: restore prior content for every patched file.
            for candidate in candidates {
                let target = self.output_root.join(&candidate.relative_path);
                tokio::fs::write(&target, &candidate.prior_content_bytes).await?;
            }

            // Signal work-item failure.
            let _ = self.client.execute(ApplicationCommand::FailWorkItem(
                autore_app::application_service::requests::FailWorkItemRequest {
                    project: self.project_id,
                    work_item_id: subject_entity.to_string(),
                    reason: "Patch build failed; rolled back".into(),
                },
            ));

            // Discard staged artifacts.
            tokio::fs::remove_dir_all(staging_dir).await?;

            Ok(PatchOutcome {
                accepted: false,
                build_success: false,
                staging_dir: None,
                diff_texts,
                diagnostics,
            })
        }
    }

    /// Runs the full pipeline.
    pub async fn apply(
        &self,
        candidates: Vec<CandidatePatch>,
        declared_paths: &HashSet<PathBuf>,
        subject_entity: EntityId,
    ) -> Result<PatchOutcome, PatchError> {
        self.validate_file_targets(&candidates, declared_paths, subject_entity)?;
        let (staging_dir, _staged_paths) = self.stage_candidate_artifacts(&candidates).await?;
        self.parse_or_syntax_check(&candidates)?;
        self.apply_through_generated_project_manager(&candidates)
            .await?;
        let build_outcome = self.build().await?;
        self.accept_or_roll_back(
            &candidates,
            build_outcome.success,
            &staging_dir,
            subject_entity,
            build_outcome.diagnostics,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_under_generated_tree(rel: &Path) -> bool {
    let s = rel.to_string_lossy();
    s.starts_with("src/generated/")
        || s.starts_with("include/recovered/")
        || s.starts_with("generated/openvb/")
}

fn allowed_source_prefixes(entity_id: &EntityId) -> Vec<PathBuf> {
    let dir = entity_source_directory(entity_id);
    vec![
        PathBuf::from("src/generated").join(&dir),
        PathBuf::from("include/recovered").join(&dir),
    ]
}

fn entity_source_directory(entity_id: &EntityId) -> PathBuf {
    let hex = entity_id.as_uuid().as_simple().to_string();
    PathBuf::from(&hex[0..2]).join(&hex[2..4]).join(&hex[4..6])
}

fn syntax_check_cpp(content: &[u8]) -> Result<(), String> {
    let text = String::from_utf8_lossy(content);
    let mut stack: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut in_char = false;
    let mut escape = false;

    for ch in text.chars() {
        if escape {
            escape = false;
            continue;
        }

        if in_string {
            if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if in_char {
            if ch == '\\' {
                escape = true;
            } else if ch == '\'' {
                in_char = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '\'' => in_char = true,
            '(' | '[' | '{' => stack.push(ch),
            ')' if stack.pop() != Some('(') => return Err("unmatched ')'".into()),
            ']' if stack.pop() != Some('[') => return Err("unmatched ']'".into()),
            '}' if stack.pop() != Some('{') => return Err("unmatched '}'".into()),
            ')' | ']' | '}' => {}
            _ => {}
        }
    }

    if in_string {
        return Err("unclosed string literal".into());
    }
    if in_char {
        return Err("unclosed character literal".into());
    }
    if !stack.is_empty() {
        return Err(format!("unclosed delimiters: {stack:?}"));
    }

    Ok(())
}

fn unified_diff(prior: &[u8], new: &[u8], path: &Path) -> String {
    let prior_text = String::from_utf8_lossy(prior);
    let new_text = String::from_utf8_lossy(new);
    let prior_lines: Vec<&str> = prior_text.lines().collect();
    let new_lines: Vec<&str> = new_text.lines().collect();

    if prior_lines == new_lines {
        return String::new();
    }

    let mut out = String::new();
    out.push_str(&format!("--- a/{path}\n", path = path.display()));
    out.push_str(&format!("+++ b/{path}\n", path = path.display()));

    let mut i = 0;
    let mut j = 0;
    while i < prior_lines.len() && j < new_lines.len() {
        if prior_lines[i] == new_lines[j] {
            out.push_str(&format!(" {}\n", prior_lines[i]));
            i += 1;
            j += 1;
        } else {
            out.push_str(&format!("-{line}\n", line = prior_lines[i]));
            out.push_str(&format!("+{line}\n", line = new_lines[j]));
            i += 1;
            j += 1;
        }
    }
    while i < prior_lines.len() {
        out.push_str(&format!("-{line}\n", line = prior_lines[i]));
        i += 1;
    }
    while j < new_lines.len() {
        out.push_str(&format!("+{line}\n", line = new_lines[j]));
        j += 1;
    }

    out
}

fn discover_cpp_source_files(root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let generated_dir = root.join("src/generated");
    if !generated_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_cpp_files(&generated_dir, &mut files)?;
    // Make paths relative to the project root.
    Ok(files
        .into_iter()
        .map(|p| p.strip_prefix(root).unwrap_or(&p).to_path_buf())
        .collect())
}

fn collect_cpp_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_cpp_files(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "cpp") {
            out.push(path);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use autore_schema::ids::{EntityId, ProjectId};

    use crate::build::{DockerMsvc2002BuildProvider, DockerMsvc2002Config};
    use crate::generation::patch::{CandidatePatch, PatchPipeline};
    use crate::tests_support::RecordingAutoReClient;

    fn mock_docker_path(script: &str) -> String {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/{script}"))
            .to_string_lossy()
            .into_owned()
    }

    fn success_provider() -> DockerMsvc2002BuildProvider {
        DockerMsvc2002BuildProvider::new(DockerMsvc2002Config {
            image_name: "msvc2002-build:test".into(),
            cmake_generator: "NMake Makefiles".into(),
            toolchain_path: std::path::PathBuf::from("/opt/msvc2002"),
            docker_binary: Some(mock_docker_path("mock-docker-success.sh")),
        })
    }

    fn failure_provider() -> DockerMsvc2002BuildProvider {
        DockerMsvc2002BuildProvider::new(DockerMsvc2002Config {
            image_name: "msvc2002-build:test".into(),
            cmake_generator: "NMake Makefiles".into(),
            toolchain_path: std::path::PathBuf::from("/opt/msvc2002"),
            docker_binary: Some(mock_docker_path("mock-docker-failure.sh")),
        })
    }

    fn entity_source_path(entity_id: EntityId) -> std::path::PathBuf {
        let hex = entity_id.as_uuid().as_simple().to_string();
        std::path::PathBuf::from("src/generated")
            .join(&hex[0..2])
            .join(&hex[2..4])
            .join(&hex[4..6])
            .join(&hex)
            .with_extension("cpp")
    }

    fn write_prior_file(
        root: &std::path::Path,
        entity_id: EntityId,
        content: &[u8],
    ) -> std::path::PathBuf {
        let rel = entity_source_path(entity_id);
        let path = root.join(&rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        rel
    }

    #[tokio::test]
    async fn apply_rejects_paths_outside_generated_openvb() {
        let tmp = tempfile::tempdir().unwrap();
        let client = RecordingAutoReClient::new();
        let entity = EntityId::new();
        let provider = success_provider();
        let pipeline = PatchPipeline::new(
            tmp.path().to_path_buf(),
            ProjectId::new(),
            &provider,
            &client,
        );

        let candidates = vec![CandidatePatch {
            relative_path: std::path::PathBuf::from("auto-re/Cargo.toml"),
            new_content_bytes: b"[package]".to_vec(),
            prior_content_bytes: Vec::new(),
            source_evidence_refs: vec![],
        }];

        let declared: HashSet<std::path::PathBuf> = HashSet::new();
        let result = pipeline.apply(candidates, &declared, entity).await;

        assert!(
            result.is_err(),
            "path outside generated tree must be rejected"
        );
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("auto-re")
                || err.to_string().contains("generated source tree"),
            "expected auto-re or generated-tree rejection, got: {err}"
        );
    }

    #[tokio::test]
    async fn apply_rejects_undeclared_deletion() {
        let tmp = tempfile::tempdir().unwrap();
        let client = RecordingAutoReClient::new();
        let entity = EntityId::new();
        let provider = success_provider();
        let pipeline = PatchPipeline::new(
            tmp.path().to_path_buf(),
            ProjectId::new(),
            &provider,
            &client,
        );

        let rel = write_prior_file(tmp.path(), entity, b"int f() { return 1; }\n");

        let candidates = vec![CandidatePatch {
            relative_path: rel,
            new_content_bytes: Vec::new(), // deletion request
            prior_content_bytes: b"int f() { return 1; }\n".to_vec(),
            source_evidence_refs: vec![],
        }];

        let declared: HashSet<std::path::PathBuf> = HashSet::new();
        let result = pipeline.apply(candidates, &declared, entity).await;

        assert!(result.is_err(), "undeclared deletion must be rejected");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("deletion of undeclared file"),
            "expected undeclared deletion error"
        );
    }

    #[tokio::test]
    async fn apply_rolls_back_on_build_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let client = RecordingAutoReClient::new();
        let entity = EntityId::new();
        let provider = failure_provider();
        let pipeline = PatchPipeline::new(
            tmp.path().to_path_buf(),
            ProjectId::new(),
            &provider,
            &client,
        );

        let prior = b"int f() { return 1; }\n";
        let rel = write_prior_file(tmp.path(), entity, prior);

        let candidates = vec![CandidatePatch {
            relative_path: rel.clone(),
            new_content_bytes: b"int f() { return 2; }\n".to_vec(),
            prior_content_bytes: prior.to_vec(),
            source_evidence_refs: vec!["evidence-1".into()],
        }];

        let mut declared: HashSet<std::path::PathBuf> = HashSet::new();
        declared.insert(rel.clone());

        let outcome = pipeline.apply(candidates, &declared, entity).await.unwrap();
        assert!(!outcome.accepted);
        assert!(!outcome.build_success);

        // Prior content must be restored.
        let restored = std::fs::read_to_string(tmp.path().join(&rel)).unwrap();
        assert_eq!(
            restored.as_bytes(),
            prior,
            "prior source must be restored after rollback"
        );

        // No artifact/mapping registration after the failed build.
        let cmds = client.commands();
        let import_count = cmds
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    autore_app::ApplicationCommand::ImportGeneratedSourceCandidates(_)
                )
            })
            .count();
        assert_eq!(
            import_count, 1,
            "ImportGeneratedSourceCandidates must be issued"
        );
        let artifact_count = cmds
            .iter()
            .filter(|c| matches!(c, autore_app::ApplicationCommand::RegisterArtifact(_)))
            .count();
        let mapping_count = cmds
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    autore_app::ApplicationCommand::RegisterGeneratedSourceMapping(_)
                )
            })
            .count();
        assert_eq!(artifact_count, 0, "no RegisterArtifact on failure");
        assert_eq!(
            mapping_count, 0,
            "no RegisterGeneratedSourceMapping on failure"
        );

        // Staging directory must be removed.
        assert!(
            !tmp.path().join(".staging").exists()
                || std::fs::read_dir(tmp.path().join(".staging"))
                    .unwrap()
                    .count()
                    == 0,
            "staging directory must be cleaned up"
        );
    }

    #[tokio::test]
    async fn atomic_accept_on_build_pass() {
        let tmp = tempfile::tempdir().unwrap();
        let client = RecordingAutoReClient::new();
        let entity = EntityId::new();
        let provider = success_provider();
        let pipeline = PatchPipeline::new(
            tmp.path().to_path_buf(),
            ProjectId::new(),
            &provider,
            &client,
        );

        let prior = b"int f() { return 1; }\n";
        let rel = write_prior_file(tmp.path(), entity, prior);

        let candidates = vec![CandidatePatch {
            relative_path: rel.clone(),
            new_content_bytes: b"int f() { return 2; }\n".to_vec(),
            prior_content_bytes: prior.to_vec(),
            source_evidence_refs: vec!["evidence-1".into()],
        }];

        let mut declared: HashSet<std::path::PathBuf> = HashSet::new();
        declared.insert(rel.clone());

        let outcome = pipeline.apply(candidates, &declared, entity).await.unwrap();
        assert!(outcome.accepted);
        assert!(outcome.build_success);

        // New content must remain in place.
        let updated = std::fs::read_to_string(tmp.path().join(&rel)).unwrap();
        assert_eq!(updated, "int f() { return 2; }\n");

        let cmds = client.commands();
        let import_count = cmds
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    autore_app::ApplicationCommand::ImportGeneratedSourceCandidates(_)
                )
            })
            .count();
        assert_eq!(import_count, 1);

        let artifact_count = cmds
            .iter()
            .filter(|c| matches!(c, autore_app::ApplicationCommand::RegisterArtifact(_)))
            .count();
        let mapping_count = cmds
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    autore_app::ApplicationCommand::RegisterGeneratedSourceMapping(_)
                )
            })
            .count();
        assert_eq!(artifact_count, 1, "RegisterArtifact per candidate");
        assert_eq!(mapping_count, 1, "RegisterGeneratedSourceMapping on accept");
    }
}
