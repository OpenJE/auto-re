//! Typed records for the build pipeline: manifests, compile units, results,
//! logs, and parsed diagnostics.
//!
//! Every struct here is transport-only — no I/O, no side effects. The
//! [`super::BuildProviderTrait`] implementations consume and produce these.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Describes the generated project tree that a build provider must configure.
#[derive(Debug, Clone)]
pub struct GeneratorManifest {
    /// Absolute path to the generated `cmkr.toml` / `CMakeLists.txt` root.
    pub project_root: PathBuf,
    /// CMake generator name (e.g. `"NMake Makefiles"`, `"Visual Studio 6"`).
    pub cmake_generator: String,
    /// Relative paths (from `project_root`) of source files to compile.
    pub source_files: Vec<PathBuf>,
    /// Executable target name declared in the CMake project.
    pub executable_target: String,
}

/// A single compilation unit: one source file → one object file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompileUnit {
    /// Path to the source file (relative to project root).
    pub source_path: PathBuf,
    /// Path where the object file should be written (relative to build dir).
    pub object_path: PathBuf,
}

/// Result of [`super::BuildProviderTrait::configure_project`].
#[derive(Debug, Clone)]
pub struct BuildConfigured {
    /// Absolute path to the build directory inside the container.
    pub build_dir: PathBuf,
    /// Whether `cmkr gen` succeeded.
    pub success: bool,
    /// Raw stdout from the configure step.
    pub stdout: String,
    /// Raw stderr from the configure step.
    pub stderr: String,
}

/// Result of [`super::BuildProviderTrait::compile_units`].
#[derive(Debug, Clone)]
pub struct CompileResult {
    /// Object files successfully produced.
    pub objects: Vec<PathBuf>,
    /// Whether all units compiled without error.
    pub success: bool,
    /// Raw stdout.
    pub stdout: String,
    /// Raw stderr.
    pub stderr: String,
}

/// Result of [`super::BuildProviderTrait::link_target`].
#[derive(Debug, Clone)]
pub struct LinkResult {
    /// Path to the linked executable.
    pub executable: PathBuf,
    /// Whether linking succeeded.
    pub success: bool,
    /// Raw stdout.
    pub stdout: String,
    /// Raw stderr.
    pub stderr: String,
}

/// Result of [`super::BuildProviderTrait::run_test`].
#[derive(Debug, Clone)]
pub struct RunTestResult {
    /// Exit code of the test executable (0 = pass).
    pub exit_code: i32,
    /// Raw stdout from the test run.
    pub stdout: String,
    /// Raw stderr from the test run.
    pub stderr: String,
}

/// Captured build logs for diagnostic parsing.
#[derive(Debug, Clone)]
pub struct BuildLogs {
    /// Combined stdout from all build steps.
    pub stdout: String,
    /// Combined stderr from all build steps.
    pub stderr: String,
}

/// Severity of a parsed compiler diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

/// The kind of repair work a diagnostic suggests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SuggestedWorkKind {
    MissingDeclaration,
    TypeMismatch,
    UndeclaredIdentifier,
    SyntaxError,
    LinkerUnresolved,
    Unknown,
}

/// A single parsed compiler/linker diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuildDiagnostic {
    /// MSVC error code (e.g. `"C2065"`, `"LNK2019"`).
    pub diagnostic_code: String,
    /// Severity classification.
    pub severity: DiagnosticSeverity,
    /// Source file path from the diagnostic.
    pub file_path: PathBuf,
    /// Line number (1-based).
    pub line: u32,
    /// Column number (1-based, 0 if unknown).
    pub column: u32,
    /// Human-readable message text.
    pub message: String,
    /// Probable root cause inferred from the code.
    pub candidate_cause: String,
    /// What kind of repair work this diagnostic suggests.
    pub suggested_work_kind: SuggestedWorkKind,
}
