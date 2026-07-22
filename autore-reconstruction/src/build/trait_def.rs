//! Async trait that every build provider must implement.
//!
//! The trait is generic over build toolchains — it knows nothing about
//! Docker, MSVC, CMake generators, or any specific compiler. All such
//! configuration lives in the concrete provider's configuration bundle.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use super::types::{
    BuildConfigured, BuildDiagnostic, BuildLogs, CompileResult, CompileUnit, GeneratorManifest,
    LinkResult, RunTestResult,
};

/// Result type for build provider operations.
pub type BuildResult<T> = Result<T, BuildProviderError>;

/// Errors from build provider operations.
#[derive(Debug, thiserror::Error)]
pub enum BuildProviderError {
    /// A subprocess exited with a non-zero code.
    #[error("command failed (exit {exit_code}): {command}")]
    CommandFailed {
        command: String,
        exit_code: i32,
        stderr: String,
    },

    /// A command argument failed allowlist validation.
    #[error("command validation rejected: {reason}")]
    ValidationRejected { reason: String },

    /// The Docker daemon is unreachable.
    #[error("docker unreachable: {0}")]
    DockerUnreachable(String),

    /// An I/O error from the filesystem or subprocess.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Catch-all for unexpected failures.
    #[error("build provider error: {0}")]
    Other(String),
}

/// The async contract every build provider implements.
///
/// Implementations receive their toolchain configuration (image name,
/// generator, toolchain path) through their constructor — the trait
/// itself is configuration-agnostic.
#[async_trait]
pub trait BuildProviderTrait: Send + Sync {
    /// Run the project generation step (e.g. `cmkr gen`).
    async fn configure_project(
        &self,
        generator_manifest: &GeneratorManifest,
        project_root: &Path,
    ) -> BuildResult<BuildConfigured>;

    /// Compile a set of source units into object files.
    async fn compile_units(&self, units: &[CompileUnit]) -> BuildResult<CompileResult>;

    /// Link compiled object files into the final executable target.
    async fn link_target(&self, target_artifacts: &[PathBuf]) -> BuildResult<LinkResult>;

    /// Run a named test target and return its exit code + output.
    async fn run_test(&self, test_target: &str) -> BuildResult<RunTestResult>;

    /// Parse raw build logs into typed diagnostics.
    async fn collect_diagnostics(
        &self,
        build_logs: &BuildLogs,
    ) -> BuildResult<Vec<BuildDiagnostic>>;
}
