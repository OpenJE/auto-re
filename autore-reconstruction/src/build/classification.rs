//! Build-failure classification taxonomy and repair-strategy routing.
//!
//! This module provides deterministic classification of MSVC compiler/linker
//! diagnostics into [`BuildFailureKind`] variants and maps each kind to a
//! structured [`RepairStrategy`] — without issuing commands. Callers decide
//! when and where to act on the strategy.
//!
//! Spec §12.3: "Use deterministic repairs where possible. Only send bounded
//! relevant diagnostics to the LLM."

use autore_schema::domain::records::WorkItemKind;

use super::types::BuildDiagnostic;

/// The 13-variant taxonomy of build failure kinds per spec §12.3.
///
/// Each variant maps to a deterministic repair strategy via
/// [`select_repair_strategy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildFailureKind {
    /// Syntax error in generated source.
    Syntax,
    /// Undeclared identifier or missing forward declaration.
    MissingDeclaration,
    /// Unknown type referenced but not yet generated.
    UnknownType,
    /// Forward-declared type used incompletely (C2079, C2027).
    IncompleteType,
    /// Struct layout mismatch — field offsets or sizes don't match the binary.
    LayoutMismatch,
    /// Calling convention mismatch between caller and callee.
    CallingConventionMismatch,
    /// Generic linkage error (not missing-symbol or duplicate-symbol).
    Linkage,
    /// Unresolved external symbol at link time (LNK2019).
    MissingSymbol,
    /// Symbol defined multiple times at link time (LNK2005).
    DuplicateSymbol,
    /// ABI mismatch — type conversion or calling-protocol error.
    Abi,
    /// Platform API not available on target (e.g. Win32 API in POSIX build).
    UnsupportedPlatformApi,
    /// Generated code has a fundamental defect requiring LLM analysis.
    GeneratedCodeDefect,
    /// Build environment issue (missing cmake, docker daemon down, etc.).
    BuildEnvironmentDefect,
}

impl std::fmt::Display for BuildFailureKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Syntax => "syntax",
            Self::MissingDeclaration => "missing-declaration",
            Self::UnknownType => "unknown-type",
            Self::IncompleteType => "incomplete-type",
            Self::LayoutMismatch => "layout-mismatch",
            Self::CallingConventionMismatch => "calling-convention-mismatch",
            Self::Linkage => "linkage",
            Self::MissingSymbol => "missing-symbol",
            Self::DuplicateSymbol => "duplicate-symbol",
            Self::Abi => "abi",
            Self::UnsupportedPlatformApi => "unsupported-platform-api",
            Self::GeneratedCodeDefect => "generated-code-defect",
            Self::BuildEnvironmentDefect => "build-environment-defect",
        };
        f.write_str(s)
    }
}

/// Structured repair action produced by the classifier.
///
/// Variants name the next step without issuing commands. Callers are
/// responsible for translating a `RepairStrategy` into the appropriate
/// `ApplicationCommand` at the right time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairStrategy {
    /// Create one or more work items of the given kind.
    CreateWorkItems {
        /// The kind of work item to create.
        kind: WorkItemKind,
        /// Human-readable reason for creating the work item.
        reason: String,
    },
    /// Block the current work item — operator intervention required.
    BlockWorkItem {
        /// Human-readable reason for blocking.
        reason: String,
    },
    /// Route the diagnostic to an LLM for analysis.
    RequestLlmAnalysis {
        /// The LLM capability to invoke (e.g. `"llm.analysis.failure"`).
        capability: String,
    },
    /// Request a layout investigation — needs dynamic analysis (Wave 8).
    RequestLayoutInvestigation,
    /// No actionable repair — diagnostic is informational only.
    NoAction,
}

/// Classify a single [`BuildDiagnostic`] into a [`BuildFailureKind`].
///
/// The classification is deterministic based on `diagnostic_code`,
/// `candidate_cause`, and `message`. No I/O or side effects.
///
/// # Rules (spec §12.3)
///
/// | MSVC Code | `BuildFailureKind` |
/// |-----------|-------------------|
/// | C2065 | `MissingDeclaration` (or `UnsupportedPlatformApi` with stdlib context) |
/// | C2061 | `UnknownType` |
/// | C2079 | `IncompleteType` |
/// | C2027 | `IncompleteType` |
/// | C2440 | `Abi` (or `LayoutMismatch` with layout/size context) |
/// | C2385 | `CallingConventionMismatch` |
/// | C3861 | `MissingDeclaration` |
/// | C1010 | `GeneratedCodeDefect` |
/// | LNK2019 | `MissingSymbol` |
/// | LNK2005 | `DuplicateSymbol` |
/// | LNK* | `Linkage` |
/// | ENV* | `BuildEnvironmentDefect` |
/// | (other) | `Syntax` |
pub fn classify(diagnostic: &BuildDiagnostic) -> BuildFailureKind {
    if is_environment_error(diagnostic) {
        return BuildFailureKind::BuildEnvironmentDefect;
    }

    match diagnostic.diagnostic_code.as_str() {
        "C1010" => BuildFailureKind::GeneratedCodeDefect,
        "C2065" if has_stdlib_context(diagnostic) => BuildFailureKind::UnsupportedPlatformApi,
        "C2065" | "C3861" => BuildFailureKind::MissingDeclaration,
        "C2061" => BuildFailureKind::UnknownType,
        "C2079" | "C2027" => BuildFailureKind::IncompleteType,
        "C2440" if has_layout_context(diagnostic) => BuildFailureKind::LayoutMismatch,
        "C2440" => BuildFailureKind::Abi,
        "C2385" => BuildFailureKind::CallingConventionMismatch,
        "LNK2019" => BuildFailureKind::MissingSymbol,
        "LNK2005" => BuildFailureKind::DuplicateSymbol,
        code if code.starts_with("LNK") => BuildFailureKind::Linkage,
        _ => BuildFailureKind::Syntax,
    }
}

/// Select a [`RepairStrategy`] for a classified build failure.
///
/// Pure function: no side effects, no command issuance.
///
/// # Routing (spec §12.3)
///
/// - `GeneratedCodeDefect` → `RequestLlmAnalysis` (LLM-bound)
/// - `BuildEnvironmentDefect` → `BlockWorkItem` (operator-fixable)
/// - `LayoutMismatch` → `RequestLayoutInvestigation` (needs dynamic analysis)
/// - All other kinds → `CreateWorkItems` with appropriate `WorkItemKind`
pub fn select_repair_strategy(
    failure_kind: BuildFailureKind,
    diagnostic: &BuildDiagnostic,
) -> RepairStrategy {
    match failure_kind {
        BuildFailureKind::GeneratedCodeDefect => RepairStrategy::RequestLlmAnalysis {
            capability: "llm.analysis.failure".to_string(),
        },
        BuildFailureKind::BuildEnvironmentDefect => RepairStrategy::BlockWorkItem {
            reason: format!("build_environment_defect: {}", diagnostic.message),
        },
        BuildFailureKind::LayoutMismatch => RepairStrategy::RequestLayoutInvestigation,
        BuildFailureKind::MissingDeclaration => RepairStrategy::CreateWorkItems {
            kind: WorkItemKind::Generation,
            reason: format!("missing_declaration: {}", diagnostic.message),
        },
        BuildFailureKind::UnknownType => RepairStrategy::CreateWorkItems {
            kind: WorkItemKind::Generation,
            reason: format!("unknown_type: {}", diagnostic.message),
        },
        BuildFailureKind::IncompleteType => RepairStrategy::CreateWorkItems {
            kind: WorkItemKind::Generation,
            reason: format!("incomplete_type: {}", diagnostic.message),
        },
        BuildFailureKind::CallingConventionMismatch => RepairStrategy::CreateWorkItems {
            kind: WorkItemKind::Generation,
            reason: format!("calling_convention_mismatch: {}", diagnostic.message),
        },
        BuildFailureKind::MissingSymbol => RepairStrategy::CreateWorkItems {
            kind: WorkItemKind::Generation,
            reason: format!("missing_symbol: {}", diagnostic.message),
        },
        BuildFailureKind::DuplicateSymbol => RepairStrategy::CreateWorkItems {
            kind: WorkItemKind::ConflictResolution,
            reason: format!("duplicate_symbol: {}", diagnostic.message),
        },
        BuildFailureKind::Linkage => RepairStrategy::CreateWorkItems {
            kind: WorkItemKind::LinkFailure,
            reason: format!("linkage: {}", diagnostic.message),
        },
        BuildFailureKind::Abi => RepairStrategy::CreateWorkItems {
            kind: WorkItemKind::Generation,
            reason: format!("abi: {}", diagnostic.message),
        },
        BuildFailureKind::UnsupportedPlatformApi => RepairStrategy::CreateWorkItems {
            kind: WorkItemKind::Generation,
            reason: format!("unsupported_platform_api: {}", diagnostic.message),
        },
        BuildFailureKind::Syntax => RepairStrategy::CreateWorkItems {
            kind: WorkItemKind::BuildFailure,
            reason: format!("syntax_error: {}", diagnostic.message),
        },
    }
}

// ─── Private helpers ────────────────────────────────────────────────────

/// Detects environment-level errors by diagnostic code prefix or message text.
fn is_environment_error(diagnostic: &BuildDiagnostic) -> bool {
    if diagnostic.diagnostic_code.starts_with("ENV") {
        return true;
    }
    let msg_lower = diagnostic.message.to_ascii_lowercase();
    let cause_lower = diagnostic.candidate_cause.to_ascii_lowercase();
    let combined = format!("{msg_lower} {cause_lower}");
    combined.contains("cmake not found")
        || combined.contains("docker daemon")
        || combined.contains("command not found")
        || combined.contains("no such file or directory")
}

/// Detects stdlib context in a C2065 diagnostic — indicating the identifier
/// belongs to a platform API that isn't available in the build environment.
fn has_stdlib_context(diagnostic: &BuildDiagnostic) -> bool {
    let text = format!("{} {}", diagnostic.message, diagnostic.candidate_cause);
    text.contains("std::")
        || text.contains("windows.h")
        || text.contains("winsock")
        || text.contains("CreateFile")
        || text.contains("GetModuleHandle")
        || text.contains("WinMain")
}

/// Detects layout/size context in a C2440 diagnostic — indicating a
/// structural layout mismatch rather than a simple ABI conversion error.
fn has_layout_context(diagnostic: &BuildDiagnostic) -> bool {
    let text = format!("{} {}", diagnostic.message, diagnostic.candidate_cause);
    let lower = text.to_ascii_lowercase();
    lower.contains("layout") || lower.contains("size") || lower.contains("offset")
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::build::types::{DiagnosticSeverity, SuggestedWorkKind};

    /// Create a diagnostic fixture with the given code and message.
    fn diag(code: &str, message: &str) -> BuildDiagnostic {
        BuildDiagnostic {
            diagnostic_code: code.to_string(),
            severity: DiagnosticSeverity::Error,
            file_path: PathBuf::from("test.cpp"),
            line: 1,
            column: 0,
            message: message.to_string(),
            candidate_cause: String::new(),
            suggested_work_kind: SuggestedWorkKind::Unknown,
        }
    }

    /// Create a diagnostic fixture with candidate_cause populated.
    fn diag_with_cause(code: &str, message: &str, cause: &str) -> BuildDiagnostic {
        BuildDiagnostic {
            diagnostic_code: code.to_string(),
            severity: DiagnosticSeverity::Error,
            file_path: PathBuf::from("test.cpp"),
            line: 1,
            column: 0,
            message: message.to_string(),
            candidate_cause: cause.to_string(),
            suggested_work_kind: SuggestedWorkKind::Unknown,
        }
    }

    // ─── One test per BuildFailureKind variant ──────────────────────

    #[test]
    fn classify_c2065_to_missing_declaration() {
        let d = diag("C2065", "'myvar' : undeclared identifier");
        assert_eq!(classify(&d), BuildFailureKind::MissingDeclaration);
    }

    #[test]
    fn classify_c2061_to_unknown_type() {
        let d = diag("C2061", "syntax error : identifier 'MyStruct'");
        assert_eq!(classify(&d), BuildFailureKind::UnknownType);
    }

    #[test]
    fn classify_c2079_to_incomplete_type() {
        let d = diag("C2079", "'x' uses undefined struct 'Foo'");
        assert_eq!(classify(&d), BuildFailureKind::IncompleteType);
    }

    #[test]
    fn classify_c2027_to_incomplete_type() {
        let d = diag("C2027", "use of undefined type 'Bar'");
        assert_eq!(classify(&d), BuildFailureKind::IncompleteType);
    }

    #[test]
    fn classify_c2440_to_abi() {
        let d = diag("C2440", "'=' : cannot convert from 'int' to 'MyEnum'");
        assert_eq!(classify(&d), BuildFailureKind::Abi);
    }

    #[test]
    fn classify_c2440_with_layout_context_to_layout_mismatch() {
        let d = diag_with_cause(
            "C2440",
            "cannot convert: layout size mismatch",
            "type conversion with layout context",
        );
        assert_eq!(classify(&d), BuildFailureKind::LayoutMismatch);
    }

    #[test]
    fn classify_c2385_to_calling_convention_mismatch() {
        let d = diag("C2385", "ambiguous call to 'func'");
        assert_eq!(classify(&d), BuildFailureKind::CallingConventionMismatch);
    }

    #[test]
    fn classify_c3861_to_missing_declaration() {
        let d = diag("C3861", "'initialize': identifier not found");
        assert_eq!(classify(&d), BuildFailureKind::MissingDeclaration);
    }

    #[test]
    fn classify_c1010_to_generated_code_defect() {
        let d = diag(
            "C1010",
            "unexpected end of file while looking for precompiled header",
        );
        assert_eq!(classify(&d), BuildFailureKind::GeneratedCodeDefect);
    }

    #[test]
    fn classify_lnk2019_to_missing_symbol() {
        let d = diag(
            "LNK2019",
            "unresolved external symbol _main referenced in function ___tmainCRTStartup",
        );
        assert_eq!(classify(&d), BuildFailureKind::MissingSymbol);
    }

    #[test]
    fn classify_lnk2005_to_duplicate_symbol() {
        let d = diag("LNK2005", "\"int g_count\" already defined in main.obj");
        assert_eq!(classify(&d), BuildFailureKind::DuplicateSymbol);
    }

    #[test]
    fn classify_generic_lnk_to_linkage() {
        let d = diag("LNK1120", "1 unresolved externals");
        assert_eq!(classify(&d), BuildFailureKind::Linkage);
    }

    #[test]
    fn classify_unknown_code_to_syntax() {
        let d = diag("C9999", "some unknown compiler error");
        assert_eq!(classify(&d), BuildFailureKind::Syntax);
    }

    #[test]
    fn classify_c2065_with_stdlib_context_to_unsupported_platform_api() {
        let d = diag_with_cause(
            "C2065",
            "'CreateFileA' : undeclared identifier",
            "references std:: windows API CreateFile",
        );
        assert_eq!(classify(&d), BuildFailureKind::UnsupportedPlatformApi);
    }

    #[test]
    fn classify_env_prefix_to_build_environment_defect() {
        let d = diag("ENV_CMAKE", "cmake not found in PATH");
        assert_eq!(classify(&d), BuildFailureKind::BuildEnvironmentDefect);
    }

    // ─── Routing tests ──────────────────────────────────────────────

    #[test]
    fn classify_routes_generated_code_defect_to_llm_analysis_failure() {
        let d = diag(
            "C1010",
            "unexpected end of file while looking for precompiled header",
        );
        let kind = classify(&d);
        assert_eq!(kind, BuildFailureKind::GeneratedCodeDefect);
        let strategy = select_repair_strategy(kind, &d);
        assert_eq!(
            strategy,
            RepairStrategy::RequestLlmAnalysis {
                capability: "llm.analysis.failure".to_string(),
            }
        );
    }

    #[test]
    fn classify_routes_environment_defect_to_block_work_item() {
        let d = diag("ENV_DOCKER", "docker daemon is not running");
        let kind = classify(&d);
        assert_eq!(kind, BuildFailureKind::BuildEnvironmentDefect);
        let strategy = select_repair_strategy(kind, &d);
        assert!(
            matches!(strategy, RepairStrategy::BlockWorkItem { .. }),
            "expected BlockWorkItem, got: {strategy:?}"
        );
        // Verify it does NOT route to LLM
        assert!(
            !matches!(strategy, RepairStrategy::RequestLlmAnalysis { .. }),
            "environment defect must not route to LLM"
        );
    }

    #[test]
    fn classify_routes_layout_mismatch_to_investigation_kind() {
        let d = diag_with_cause(
            "C2440",
            "cannot convert: struct size mismatch",
            "layout offset does not match expected",
        );
        let kind = classify(&d);
        assert_eq!(kind, BuildFailureKind::LayoutMismatch);
        let strategy = select_repair_strategy(kind, &d);
        assert_eq!(strategy, RepairStrategy::RequestLayoutInvestigation);
    }

    // ─── Repair strategy routing for remaining kinds ────────────────

    #[test]
    fn repair_missing_declaration_creates_generation_work_item() {
        let d = diag("C2065", "'foo' : undeclared identifier");
        let kind = classify(&d);
        let strategy = select_repair_strategy(kind, &d);
        match strategy {
            RepairStrategy::CreateWorkItems { kind, reason } => {
                assert_eq!(kind, WorkItemKind::Generation);
                assert!(reason.contains("missing_declaration"));
            }
            other => panic!("expected CreateWorkItems, got: {other:?}"),
        }
    }

    #[test]
    fn repair_duplicate_symbol_creates_conflict_resolution() {
        let d = diag("LNK2005", "\"g_count\" already defined in main.obj");
        let kind = classify(&d);
        let strategy = select_repair_strategy(kind, &d);
        match strategy {
            RepairStrategy::CreateWorkItems { kind, reason } => {
                assert_eq!(kind, WorkItemKind::ConflictResolution);
                assert!(reason.contains("duplicate_symbol"));
            }
            other => panic!("expected CreateWorkItems, got: {other:?}"),
        }
    }

    #[test]
    fn repair_linkage_creates_link_failure_work_item() {
        let d = diag("LNK1120", "1 unresolved externals");
        let kind = classify(&d);
        let strategy = select_repair_strategy(kind, &d);
        match strategy {
            RepairStrategy::CreateWorkItems { kind, reason } => {
                assert_eq!(kind, WorkItemKind::LinkFailure);
                assert!(reason.contains("linkage"));
            }
            other => panic!("expected CreateWorkItems, got: {other:?}"),
        }
    }

    #[test]
    fn repair_syntax_creates_build_failure_work_item() {
        let d = diag("C9999", "unexpected token");
        let kind = classify(&d);
        let strategy = select_repair_strategy(kind, &d);
        match strategy {
            RepairStrategy::CreateWorkItems { kind, reason } => {
                assert_eq!(kind, WorkItemKind::BuildFailure);
                assert!(reason.contains("syntax_error"));
            }
            other => panic!("expected CreateWorkItems, got: {other:?}"),
        }
    }

    // ─── Display coverage ───────────────────────────────────────────

    #[test]
    fn build_failure_kind_display_covers_all_variants() {
        let kinds = [
            BuildFailureKind::Syntax,
            BuildFailureKind::MissingDeclaration,
            BuildFailureKind::UnknownType,
            BuildFailureKind::IncompleteType,
            BuildFailureKind::LayoutMismatch,
            BuildFailureKind::CallingConventionMismatch,
            BuildFailureKind::Linkage,
            BuildFailureKind::MissingSymbol,
            BuildFailureKind::DuplicateSymbol,
            BuildFailureKind::Abi,
            BuildFailureKind::UnsupportedPlatformApi,
            BuildFailureKind::GeneratedCodeDefect,
            BuildFailureKind::BuildEnvironmentDefect,
        ];
        for kind in &kinds {
            let s = kind.to_string();
            assert!(
                !s.is_empty(),
                "Display must produce non-empty string for {kind:?}"
            );
        }
        // Exactly 13 variants
        assert_eq!(kinds.len(), 13);
    }
}
