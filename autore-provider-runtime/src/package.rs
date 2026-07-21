//! Local provider package discovery, manifest validation, and content-hash verification.
// allow: SIZE_OK — single responsibility (package validation pipeline); splitting would create
// artificial fragmentation across tightly coupled error/manifest/hash/discovery types.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use autore_provider_protocol::v1::CapabilityDescriptor;
use regex::Regex;

static PACKAGE_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9-]+\.[a-z0-9-]+$").unwrap());

/// Errors that can occur during package validation.
#[derive(Debug, thiserror::Error)]
pub enum PackageValidationError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Toml(String),
    #[error("schema_version must be 1, got {0}")]
    SchemaVersion(u32),
    #[error("invalid package_id '{0}': must match ^[a-z0-9-]+\\.[a-z0-9-]+$")]
    PackageId(String),
    #[error("entrypoint does not exist: {0}")]
    EntrypointMissing(PathBuf),
    #[error("entrypoint resolves outside package root: {0}")]
    EntrypointNotContained(PathBuf),
    #[error("symlink escape detected: {0}")]
    SymlinkEscape(PathBuf),
    #[error("content hash mismatch: expected {expected}, computed {computed}")]
    ContentHashMismatch { expected: String, computed: String },
    #[error("protocol_range [{min}, {max}] does not include coordinator version 1")]
    ProtocolRange { min: u32, max: u32 },
    #[error("capabilities list must not be empty")]
    CapabilityEmpty,
    #[error("invalid capability descriptor: {0}")]
    CapabilityInvalid(String),
    #[error("configuration_schema is not valid JSON Schema: {0}")]
    ConfigurationSchemaInvalid(String),
    #[error("failed to parse version '{0}' as semver")]
    VersionParse(String),
}

/// Raw TOML-deserialized manifest (intermediate representation).
#[derive(serde::Deserialize)]
struct RawManifest {
    schema_version: u32,
    package_id: String,
    version: String,
    content_hash: String,
    entrypoint: PathBuf,
    protocol_range: (u32, u32),
    capabilities: Vec<RawCapability>,
    #[serde(default)]
    max_concurrency: HashMap<String, usize>,
    configuration_schema: String,
}

/// Raw TOML-deserialized capability (intermediate representation).
#[derive(serde::Deserialize)]
struct RawCapability {
    capability_id: String,
    version: String,
    name: String,
    request_schema: String,
    response_schema: String,
}

/// Validated provider package manifest.
#[derive(Debug)]
pub struct PackageManifest {
    pub schema_version: u32,
    pub package_id: String,
    pub version: semver::Version,
    pub content_hash: Vec<u8>,
    pub entrypoint: PathBuf,
    pub protocol_range: (u32, u32),
    pub capabilities: Vec<CapabilityDescriptor>,
    pub max_concurrency: HashMap<String, usize>,
    pub configuration_schema: Vec<u8>,
}

/// Result of successful package discovery and validation.
#[derive(Debug)]
pub struct PackageInstallationIntent {
    pub manifest: PackageManifest,
    pub package_root: PathBuf,
    pub executable_path: PathBuf,
}

/// Discovers provider packages from configured local roots.
pub struct ProviderPackageDiscovery {
    provider_roots: Vec<PathBuf>,
}

#[derive(serde::Deserialize)]
struct ProviderRootsConfig {
    roots: Vec<PathBuf>,
}

impl ProviderPackageDiscovery {
    /// Creates a discovery instance from a project directory.
    ///
    /// Reads `project.auto-re/provider_roots.toml` if present; otherwise defaults
    /// to `<project_dir>/providers/`.
    pub fn from_project_dir(project_dir: &Path) -> Result<Self, PackageValidationError> {
        let config_path = project_dir.join("project.auto-re/provider_roots.toml");
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: ProviderRootsConfig = toml::from_str(&content)
                .map_err(|e| PackageValidationError::Toml(e.to_string()))?;
            Ok(Self {
                provider_roots: config.roots,
            })
        } else {
            let default_root = project_dir.join("providers");
            Ok(Self {
                provider_roots: vec![default_root],
            })
        }
    }

    /// Creates a discovery instance with explicit provider roots.
    pub fn new(provider_roots: Vec<PathBuf>) -> Self {
        Self { provider_roots }
    }

    /// Scans all provider roots and returns validated installation intents.
    ///
    /// Each subdirectory within a root that contains a `manifest.toml` is treated
    /// as a candidate package.
    pub fn discover_packages(
        &self,
    ) -> Result<Vec<PackageInstallationIntent>, PackageValidationError> {
        let mut results = Vec::new();

        for root in &self.provider_roots {
            if !root.exists() {
                continue;
            }

            let entries = std::fs::read_dir(root)?;
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let manifest_path = path.join("manifest.toml");
                if !manifest_path.exists() {
                    continue;
                }

                let intent = validate_package(&path)?;
                results.push(intent);
            }
        }

        Ok(results)
    }
}

/// Validates a single package directory and returns an installation intent.
pub fn validate_package(
    package_root: &Path,
) -> Result<PackageInstallationIntent, PackageValidationError> {
    let manifest_path = package_root.join("manifest.toml");
    let content = std::fs::read_to_string(&manifest_path)?;

    let raw: RawManifest =
        toml::from_str(&content).map_err(|e| PackageValidationError::Toml(e.to_string()))?;

    // Validate schema_version.
    if raw.schema_version != 1 {
        return Err(PackageValidationError::SchemaVersion(raw.schema_version));
    }

    // Validate package_id format.
    if !PACKAGE_ID_RE.is_match(&raw.package_id) {
        return Err(PackageValidationError::PackageId(raw.package_id));
    }

    // Parse semver version.
    let version = semver::Version::parse(&raw.version)
        .map_err(|_| PackageValidationError::VersionParse(raw.version.clone()))?;

    // Decode content hash from hex.
    let content_hash = hex_decode(&raw.content_hash)
        .map_err(|e| PackageValidationError::Toml(format!("invalid content_hash hex: {e}")))?;

    // Validate entrypoint exists.
    let entrypoint_abs = package_root.join(&raw.entrypoint);
    if !entrypoint_abs.exists() {
        return Err(PackageValidationError::EntrypointMissing(
            raw.entrypoint.clone(),
        ));
    }

    // Validate entrypoint containment (no symlink escape).
    let canonical_root = std::fs::canonicalize(package_root)?;
    let canonical_entrypoint = std::fs::canonicalize(&entrypoint_abs)?;
    if !canonical_entrypoint.starts_with(&canonical_root) {
        let meta = std::fs::symlink_metadata(&entrypoint_abs)?;
        return if meta.file_type().is_symlink() {
            Err(PackageValidationError::SymlinkEscape(raw.entrypoint))
        } else {
            Err(PackageValidationError::EntrypointNotContained(
                raw.entrypoint,
            ))
        };
    }

    // Compute and verify content hash.
    let computed_hash = compute_content_hash(package_root)?;
    let computed_bytes = computed_hash.as_bytes();
    if computed_bytes.as_slice() != content_hash.as_slice() {
        return Err(PackageValidationError::ContentHashMismatch {
            expected: raw.content_hash,
            computed: hex_encode(computed_bytes),
        });
    }

    // Validate protocol range includes version 1.
    let (proto_min, proto_max) = raw.protocol_range;
    if proto_min > 1 || proto_max < 1 {
        return Err(PackageValidationError::ProtocolRange {
            min: proto_min,
            max: proto_max,
        });
    }

    // Validate capabilities list is non-empty.
    if raw.capabilities.is_empty() {
        return Err(PackageValidationError::CapabilityEmpty);
    }

    // Validate each capability descriptor.
    let mut capabilities = Vec::with_capacity(raw.capabilities.len());
    for cap in &raw.capabilities {
        if cap.capability_id.is_empty() {
            return Err(PackageValidationError::CapabilityInvalid(
                "capability_id must not be empty".to_string(),
            ));
        }
        let cap_ver = semver::Version::parse(&cap.version).map_err(|_| {
            PackageValidationError::CapabilityInvalid(format!(
                "capability '{}' version '{}' is not valid semver",
                cap.capability_id, cap.version
            ))
        })?;
        if cap_ver.major < 1 {
            return Err(PackageValidationError::CapabilityInvalid(format!(
                "capability '{}' version must have major >= 1, got {cap_ver}",
                cap.capability_id
            )));
        }
        let req_bytes = cap.request_schema.as_bytes().to_vec();
        let res_bytes = cap.response_schema.as_bytes().to_vec();
        let ctx_req = format!("capability '{}'.request_schema", cap.capability_id);
        let ctx_res = format!("capability '{}'.response_schema", cap.capability_id);
        validate_json_schema(&req_bytes, &ctx_req)?;
        validate_json_schema(&res_bytes, &ctx_res)?;
        capabilities.push(CapabilityDescriptor {
            capability_id: cap.capability_id.clone(),
            version: cap.version.clone(),
            name: cap.name.clone(),
            request_schema: req_bytes,
            response_schema: res_bytes,
        });
    }

    // Validate configuration_schema is valid JSON Schema.
    let config_schema_bytes = raw.configuration_schema.as_bytes().to_vec();
    validate_json_schema(&config_schema_bytes, "configuration_schema")
        .map_err(|e| PackageValidationError::ConfigurationSchemaInvalid(e.to_string()))?;

    let manifest = PackageManifest {
        schema_version: raw.schema_version,
        package_id: raw.package_id,
        version,
        content_hash,
        entrypoint: raw.entrypoint,
        protocol_range: raw.protocol_range,
        capabilities,
        max_concurrency: raw.max_concurrency,
        configuration_schema: config_schema_bytes,
    };

    Ok(PackageInstallationIntent {
        manifest,
        package_root: canonical_root,
        executable_path: canonical_entrypoint,
    })
}

/// Validates that a byte slice parses as a valid JSON Schema.
fn validate_json_schema(bytes: &[u8], context: &str) -> Result<(), PackageValidationError> {
    let schema_value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| {
        PackageValidationError::CapabilityInvalid(format!("{context}: invalid JSON: {e}"))
    })?;

    jsonschema::validator_for(&schema_value).map_err(|e| {
        PackageValidationError::CapabilityInvalid(format!(
            "{context}: schema compilation failed: {e}"
        ))
    })?;

    Ok(())
}

/// Computes the BLAKE3 content hash of a package directory.
///
/// Walks all files recursively, excluding `manifest.toml` and symlinks.
/// For each file, computes its BLAKE3 hash. Files are sorted by relative path,
/// then a final BLAKE3 hash is computed over the concatenated (path, hash) pairs.
pub fn compute_content_hash(package_root: &Path) -> Result<blake3::Hash, PackageValidationError> {
    let mut entries: Vec<(String, [u8; 32])> = Vec::new();
    collect_files(package_root, package_root, &mut entries)?;

    // Sort by relative path string.
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    // Feed sorted (path, hash) pairs into final hasher.
    let mut final_hasher = blake3::Hasher::new();
    for (path, hash) in &entries {
        final_hasher.update(path.as_bytes());
        final_hasher.update(hash);
    }

    Ok(final_hasher.finalize())
}

/// Recursively collects regular files, rejecting symlinks.
fn collect_files(
    dir: &Path,
    root: &Path,
    entries: &mut Vec<(String, [u8; 32])>,
) -> Result<(), PackageValidationError> {
    let read_dir = std::fs::read_dir(dir)?;

    for entry in read_dir {
        let entry = entry?;
        let path = entry.path();

        // Check for symlinks — reject rather than traverse.
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            return Err(PackageValidationError::SymlinkEscape(path));
        }

        if meta.is_dir() {
            collect_files(&path, root, entries)?;
        } else if meta.is_file() {
            // Skip manifest.toml.
            if entry.file_name() == "manifest.toml" {
                continue;
            }

            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");

            let data = std::fs::read(&path)?;
            let hash = *blake3::hash(&data).as_bytes();
            entries.push((relative, hash));
        }
    }

    Ok(())
}

/// Decodes a hex string to bytes.
fn hex_decode(hex: &str) -> Result<Vec<u8>, String> {
    if !hex.len().is_multiple_of(2) {
        return Err("odd-length hex string".to_string());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|e| format!("invalid hex at position {i}: {e}"))
        })
        .collect()
}

/// Encodes bytes as a lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let data = b"hello world";
        let hex = hex_encode(data);
        let decoded = hex_decode(&hex).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn package_id_regex_accepts_valid() {
        let re = Regex::new(r"^[a-z0-9-]+\.[a-z0-9-]+$").unwrap();
        assert!(re.is_match("fixture.echo"));
        assert!(re.is_match("ida.binary-open"));
        assert!(re.is_match("my-cool.provider-1"));
    }

    #[test]
    fn package_id_regex_rejects_invalid() {
        let re = Regex::new(r"^[a-z0-9-]+\.[a-z0-9-]+$").unwrap();
        assert!(!re.is_match("Fixture.Echo")); // uppercase
        assert!(!re.is_match("no-namespace"));
        assert!(!re.is_match("too.many.dots"));
        assert!(!re.is_match(""));
    }
}
