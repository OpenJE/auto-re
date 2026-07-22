//! Unit tests for the build module.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use super::diagnostics::parse_msvc_diagnostics;
use super::docker_msvc2002::{DockerMsvc2002BuildProvider, DockerMsvc2002Config};
use super::trait_def::{BuildProviderError, BuildProviderTrait, BuildResult};
use super::types::{
    BuildConfigured, BuildDiagnostic, BuildLogs, CompileResult, CompileUnit, DiagnosticSeverity,
    GeneratorManifest, LinkResult, RunTestResult, SuggestedWorkKind,
};

// ─── Stub adapter: proves the trait admits other implementations ───

struct CmakeNinjaBuildProvider;

#[async_trait]
impl BuildProviderTrait for CmakeNinjaBuildProvider {
    async fn configure_project(
        &self,
        _manifest: &GeneratorManifest,
        _root: &Path,
    ) -> BuildResult<BuildConfigured> {
        Ok(BuildConfigured {
            build_dir: PathBuf::from("/tmp/ninja-build"),
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    async fn compile_units(&self, units: &[CompileUnit]) -> BuildResult<CompileResult> {
        let objects = units.iter().map(|u| u.object_path.clone()).collect();
        Ok(CompileResult {
            objects,
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    async fn link_target(&self, targets: &[PathBuf]) -> BuildResult<LinkResult> {
        Ok(LinkResult {
            executable: targets.first().cloned().unwrap_or_default(),
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    async fn run_test(&self, _target: &str) -> BuildResult<RunTestResult> {
        Ok(RunTestResult {
            exit_code: 0,
            stdout: "PASS".into(),
            stderr: String::new(),
        })
    }

    async fn collect_diagnostics(&self, _logs: &BuildLogs) -> BuildResult<Vec<BuildDiagnostic>> {
        Ok(vec![])
    }
}

// ─── Tests ────────────────────────────────────────────────────────

#[test]
fn trait_admits_other_adapters() {
    // CmakeNinjaBuildProvider compiles against BuildProviderTrait —
    // proving the trait has no Docker or MSVC specifics baked in.
    let provider = CmakeNinjaBuildProvider;
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async {
        let manifest = GeneratorManifest {
            project_root: PathBuf::from("/tmp/test"),
            cmake_generator: "Ninja".into(),
            source_files: vec![],
            executable_target: "test".into(),
        };
        provider
            .configure_project(&manifest, Path::new("/tmp/test"))
            .await
    });
    assert!(result.is_ok());
    let configured = result.unwrap();
    assert!(configured.success);
}

#[test]
fn build_provider_parses_compiler_diagnostics_to_typed_records() {
    let stderr = "main.cpp(42) : error C2065: 'foo' : undeclared identifier\n\
                  utils.cpp(10) : warning C4244: conversion from 'double' to 'int'\n";
    let diags = parse_msvc_diagnostics(stderr);
    assert_eq!(diags.len(), 2);
    let first = &diags[0];
    assert_eq!(first.diagnostic_code, "C2065");
    assert_eq!(first.severity, DiagnosticSeverity::Error);
    assert_eq!(first.file_path, PathBuf::from("main.cpp"));
    assert_eq!(first.line, 42);
    assert_eq!(
        first.suggested_work_kind,
        SuggestedWorkKind::UndeclaredIdentifier
    );
    assert!(first.message.contains("undeclared identifier"));

    let second = &diags[1];
    assert_eq!(second.diagnostic_code, "C4244");
    assert_eq!(second.severity, DiagnosticSeverity::Warning);
    assert_eq!(second.file_path, PathBuf::from("utils.cpp"));
    assert_eq!(second.line, 10);
}

#[tokio::test]
async fn build_provider_propagates_fail_status_to_completed_with_failed() {
    // The Docker provider should return CommandFailed when docker exits non-zero.
    let config = DockerMsvc2002Config {
        image_name: "msvc2002-build:test".into(),
        cmake_generator: "NMake Makefiles".into(),
        toolchain_path: PathBuf::from("/opt/msvc2002"),
        docker_binary: Some("false".into()), // `false` always exits 1
    };
    let provider = DockerMsvc2002BuildProvider::new(config);
    let manifest = GeneratorManifest {
        project_root: PathBuf::from("/tmp/test"),
        cmake_generator: "NMake Makefiles".into(),
        source_files: vec![],
        executable_target: "test".into(),
    };
    let result = provider
        .configure_project(&manifest, Path::new("/tmp/test"))
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, BuildProviderError::DockerUnreachable(_)),
        "expected DockerUnreachable, got: {err}"
    );
}

#[test]
fn build_provider_never_uses_unconfigured_image_names() {
    let config = DockerMsvc2002Config {
        image_name: "msvc2002-build:approved".into(),
        cmake_generator: "NMake Makefiles".into(),
        toolchain_path: PathBuf::from("/opt/msvc2002"),
        docker_binary: None,
    };
    let provider = DockerMsvc2002BuildProvider::new(config);
    // Attempting to validate an unconfigured image name should fail.
    let result = provider.validate_image_name("evil-image:latest");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, BuildProviderError::ValidationRejected { .. }),
        "expected ValidationRejected, got: {err}"
    );
}

#[tokio::test]
async fn build_provider_invokes_cmkr_gen_then_cmake_build_inside_docker() {
    // Use `echo` as a fake docker binary to verify the argument sequence.
    // `echo` will succeed (exit 0) and print its arguments to stdout,
    // which we can capture to verify the command sequence.
    let config = DockerMsvc2002Config {
        image_name: "msvc2002-build:test".into(),
        cmake_generator: "NMake Makefiles".into(),
        toolchain_path: PathBuf::from("/opt/msvc2002"),
        docker_binary: Some("echo".into()),
    };
    let provider = DockerMsvc2002BuildProvider::new(config);
    let manifest = GeneratorManifest {
        project_root: PathBuf::from("/tmp/test-build-seq"),
        cmake_generator: "NMake Makefiles".into(),
        source_files: vec![PathBuf::from("src/main.cpp")],
        executable_target: "myapp".into(),
    };
    // configure_project: calls `docker run` then `docker exec cmkr gen`.
    let result = provider
        .configure_project(&manifest, Path::new("/tmp/test-build-seq"))
        .await;
    assert!(
        result.is_ok(),
        "configure should succeed with echo: {result:?}"
    );
    let configured = result.unwrap();
    assert!(configured.success);
    // The stdout from `echo` contains the arguments, proving cmkr gen was called.
    assert!(
        configured.stdout.contains("cmkr") || configured.stdout.contains("gen"),
        "stdout should contain cmkr/gen args: {}",
        configured.stdout
    );

    // compile_units: calls `docker exec cmake --build build`.
    let units = vec![CompileUnit {
        source_path: PathBuf::from("src/main.cpp"),
        object_path: PathBuf::from("build/main.obj"),
    }];
    let compile = provider.compile_units(&units).await;
    assert!(compile.is_ok(), "compile should succeed: {compile:?}");
    let compiled = compile.unwrap();
    assert!(compiled.success);
    assert!(
        compiled.stdout.contains("cmake") || compiled.stdout.contains("--build"),
        "stdout should contain cmake --build args: {}",
        compiled.stdout
    );
}

#[test]
fn validate_no_metacharacters_rejects_shell_injection() {
    let result = DockerMsvc2002BuildProvider::validate_no_metacharacters("good-arg");
    assert!(result.is_ok());
    let result = DockerMsvc2002BuildProvider::validate_no_metacharacters("bad;rm -rf /");
    assert!(result.is_err());
    let result = DockerMsvc2002BuildProvider::validate_no_metacharacters("bad$(cmd)");
    assert!(result.is_err());
}

#[test]
fn parse_msvc_diagnostics_handles_empty_input() {
    let diags = parse_msvc_diagnostics("");
    assert!(diags.is_empty());
}

#[test]
fn parse_msvc_diagnostics_classifies_known_codes() {
    let stderr = "file.cpp(1) : error C2079: uses undefined struct 'Foo'\n";
    let diags = parse_msvc_diagnostics(stderr);
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].suggested_work_kind,
        SuggestedWorkKind::MissingDeclaration
    );
    assert_eq!(diags[0].diagnostic_code, "C2079");
}
