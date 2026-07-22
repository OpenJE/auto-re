//! MSVC diagnostic parser: extracts typed diagnostics from compiler stderr.
//!
//! MSVC format: `<file>(<line>) : <severity> <code>: <message>`
//! Example: `main.cpp(42) : error C2065: 'foo' : undeclared identifier`

use std::path::PathBuf;

use super::types::{BuildDiagnostic, DiagnosticSeverity, SuggestedWorkKind};

/// Parse MSVC-style diagnostics from stderr output.
pub fn parse_msvc_diagnostics(stderr: &str) -> Vec<BuildDiagnostic> {
    stderr.lines().filter_map(parse_msvc_line).collect()
}

/// Parse a single MSVC diagnostic line.
fn parse_msvc_line(line: &str) -> Option<BuildDiagnostic> {
    let paren_open = line.find('(')?;
    let paren_close = line[paren_open..].find(')')? + paren_open;
    let file_path = &line[..paren_open];
    let line_num: u32 = line[paren_open + 1..paren_close].parse().ok()?;
    let rest = line[paren_close + 1..]
        .trim_start_matches(") : ")
        .trim_start_matches("): ");
    let (severity, code, message) = parse_severity_code(rest)?;
    let (candidate_cause, suggested_work_kind) = classify_msvc_code(&code, &message);
    Some(BuildDiagnostic {
        diagnostic_code: code,
        severity,
        file_path: PathBuf::from(file_path.trim()),
        line: line_num,
        column: 0,
        message,
        candidate_cause,
        suggested_work_kind,
    })
}

/// Extract severity, error code, and message from the tail of a diagnostic line.
fn parse_severity_code(rest: &str) -> Option<(DiagnosticSeverity, String, String)> {
    if let Some(idx) = rest.find(" error ") {
        let tail = &rest[idx + 7..];
        let end = tail.find(':').unwrap_or(tail.len());
        Some((
            DiagnosticSeverity::Error,
            tail[..end].trim().into(),
            tail[end..].trim_start_matches(':').trim().into(),
        ))
    } else if let Some(idx) = rest.find(" warning ") {
        let tail = &rest[idx + 9..];
        let end = tail.find(':').unwrap_or(tail.len());
        Some((
            DiagnosticSeverity::Warning,
            tail[..end].trim().into(),
            tail[end..].trim_start_matches(':').trim().into(),
        ))
    } else {
        None
    }
}

/// Classify an MSVC error code into a candidate cause and work kind.
fn classify_msvc_code(code: &str, message: &str) -> (String, SuggestedWorkKind) {
    match code {
        "C2065" => (
            format!("undeclared identifier: {message}"),
            SuggestedWorkKind::UndeclaredIdentifier,
        ),
        "C2061" => (
            format!("syntax error near identifier: {message}"),
            SuggestedWorkKind::SyntaxError,
        ),
        "C2079" => (
            format!("incomplete type usage: {message}"),
            SuggestedWorkKind::MissingDeclaration,
        ),
        "C2440" => (
            format!("type conversion mismatch: {message}"),
            SuggestedWorkKind::TypeMismatch,
        ),
        "C2039" => (
            format!("member not found: {message}"),
            SuggestedWorkKind::MissingDeclaration,
        ),
        "C2027" => (
            format!("use of undefined type: {message}"),
            SuggestedWorkKind::MissingDeclaration,
        ),
        c if c.starts_with("LNK") => (
            format!("linker error: {message}"),
            SuggestedWorkKind::LinkerUnresolved,
        ),
        _ => (
            format!("compiler diagnostic {code}: {message}"),
            SuggestedWorkKind::Unknown,
        ),
    }
}
