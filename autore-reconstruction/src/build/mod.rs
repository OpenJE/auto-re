//! Build pipeline abstraction: compile, link, test, and diagnose.
//!
//! This module defines [`BuildProviderTrait`] — the async contract every
//! build backend implements — plus a first concrete provider
//! ([`DockerMsvc2002BuildProvider`]) that wraps `cmkr` + `cmake` inside
//! a Docker container hosting the MSVC 2002 compiler.
//!
//! The trait is toolchain-agnostic: Docker image names, CMake generators,
//! and MSVC toolchain paths live in the provider's configuration bundle,
//! not in the trait. A future `CmakeNinjaBuildProvider` or
//! `WasmEmscriptenBuildProvider` can implement the same trait with
//! different toolchains.
//!
//! Every Docker command is validated against an allowlist (project-root
//! containment, configured image name, no shell metacharacters) before
//! execution.

pub mod classification;
pub mod diagnostics;
pub mod docker_msvc2002;
pub mod trait_def;
pub mod types;

#[cfg(test)]
mod tests;

pub use classification::{BuildFailureKind, RepairStrategy, classify, select_repair_strategy};
pub use diagnostics::parse_msvc_diagnostics;
pub use docker_msvc2002::{DockerMsvc2002BuildProvider, DockerMsvc2002Config};
pub use trait_def::{BuildProviderError, BuildProviderTrait, BuildResult};
pub use types::{
    BuildConfigured, BuildDiagnostic, BuildLogs, CompileResult, CompileUnit, DiagnosticSeverity,
    GeneratorManifest, LinkResult, RunTestResult, SuggestedWorkKind,
};
