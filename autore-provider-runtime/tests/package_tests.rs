//! Integration tests for the package discovery and validation module.

use std::path::Path;

use autore_provider_runtime::package::{
    PackageValidationError, compute_content_hash, validate_package,
};
use tempfile::TempDir;

/// Creates a minimal valid package fixture in the given directory.
///
/// Returns the hex-encoded content hash for use in the manifest.
fn create_fixture_package(dir: &Path) -> String {
    // Create the entrypoint binary (just a script).
    let bin_dir = dir.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    std::fs::write(bin_dir.join("fixture-echo"), "#!/bin/sh\necho hello\n").unwrap();

    // Make it executable on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            bin_dir.join("fixture-echo"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }

    // Compute the content hash before writing manifest.
    let hash = compute_content_hash(dir).unwrap();
    let hex_hash = hash.to_hex().to_string();

    // Write manifest.
    let manifest = format!(
        r#"schema_version = 1
package_id = "fixture.echo"
version = "0.1.0"
content_hash = "{hex_hash}"
entrypoint = "bin/fixture-echo"
protocol_range = [1, 1]
configuration_schema = '{{"type": "object"}}'

[[capabilities]]
capability_id = "fixture.echo"
version = "1.0.0"
name = "Echo"
request_schema = '{{"type": "object"}}'
response_schema = '{{"type": "object"}}'

[max_concurrency]
"fixture.echo" = 10
"#
    );

    std::fs::write(dir.join("manifest.toml"), manifest).unwrap();
    hex_hash
}

#[test]
fn valid_manifest_loads() {
    let tmp = TempDir::new().unwrap();
    let pkg_dir = tmp.path().join("fixture-echo");
    std::fs::create_dir_all(&pkg_dir).unwrap();

    create_fixture_package(&pkg_dir);

    let intent = validate_package(&pkg_dir).expect("valid package should load");
    assert_eq!(intent.manifest.package_id, "fixture.echo");
    assert_eq!(intent.manifest.version, semver::Version::new(0, 1, 0));
    assert_eq!(intent.manifest.schema_version, 1);
    assert_eq!(intent.manifest.capabilities.len(), 1);
    assert_eq!(
        intent.manifest.capabilities[0].capability_id,
        "fixture.echo"
    );
    assert_eq!(
        *intent.manifest.max_concurrency.get("fixture.echo").unwrap(),
        10
    );
}

#[test]
fn missing_entrypoint_fails() {
    let tmp = TempDir::new().unwrap();
    let pkg_dir = tmp.path().join("bad-entry");
    std::fs::create_dir_all(&pkg_dir).unwrap();

    // Write manifest pointing to a non-existent entrypoint.
    // We need a valid content hash, so create a dummy file for hashing.
    let data_dir = pkg_dir.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::write(data_dir.join("dummy.txt"), "dummy").unwrap();

    let hash = compute_content_hash(&pkg_dir).unwrap();
    let hex_hash = hash.to_hex().to_string();

    let manifest = format!(
        r#"schema_version = 1
package_id = "fixture.echo"
version = "0.1.0"
content_hash = "{hex_hash}"
entrypoint = "bin/nonexistent"
protocol_range = [1, 1]
configuration_schema = '{{"type": "object"}}'

[[capabilities]]
capability_id = "fixture.echo"
version = "1.0.0"
name = "Echo"
request_schema = '{{"type": "object"}}'
response_schema = '{{"type": "object"}}'

[max_concurrency]
"fixture.echo" = 10
"#
    );

    std::fs::write(pkg_dir.join("manifest.toml"), manifest).unwrap();

    let err = validate_package(&pkg_dir).unwrap_err();
    assert!(
        matches!(err, PackageValidationError::EntrypointMissing(_)),
        "expected EntrypointMissing, got: {err:?}"
    );
}

#[test]
fn symlink_escape_rejected() {
    let tmp = TempDir::new().unwrap();
    let pkg_dir = tmp.path().join("symlink-pkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();

    // Create a symlink pointing to /etc/passwd (or a file outside the package).
    let target = if Path::new("/etc/passwd").exists() {
        "/etc/passwd".to_string()
    } else {
        // Fallback: create a file outside the package dir.
        let outside = tmp.path().join("outside.txt");
        std::fs::write(&outside, "outside").unwrap();
        outside.to_string_lossy().to_string()
    };

    let bin_dir = pkg_dir.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();

    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, bin_dir.join("fixture-echo")).unwrap();

    // Also create a data file for hashing.
    std::fs::write(pkg_dir.join("data.txt"), "data").unwrap();

    // The hash will fail because of the symlink in the walk, but we need to get
    // past hash computation to test entrypoint validation. So we use a manifest
    // with a dummy hash and expect the symlink walk to fail first.
    let manifest = r#"schema_version = 1
package_id = "fixture.echo"
version = "0.1.0"
content_hash = "0000000000000000000000000000000000000000000000000000000000000000"
entrypoint = "bin/fixture-echo"
protocol_range = [1, 1]
configuration_schema = '{"type": "object"}'

[[capabilities]]
capability_id = "fixture.echo"
version = "1.0.0"
name = "Echo"
request_schema = '{"type": "object"}'
response_schema = '{"type": "object"}'

[max_concurrency]
"fixture.echo" = 10
"#;

    std::fs::write(pkg_dir.join("manifest.toml"), manifest).unwrap();

    let err = validate_package(&pkg_dir).unwrap_err();
    let is_symlink_error = matches!(
        err,
        PackageValidationError::SymlinkEscape(_)
            | PackageValidationError::EntrypointNotContained(_)
    );
    assert!(
        is_symlink_error,
        "expected SymlinkEscape or EntrypointNotContained, got: {err:?}"
    );
}

#[test]
fn protocol_range_outside_v1_rejected() {
    let tmp = TempDir::new().unwrap();
    let pkg_dir = tmp.path().join("proto-range");
    std::fs::create_dir_all(&pkg_dir).unwrap();

    // Create valid files.
    let bin_dir = pkg_dir.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    std::fs::write(bin_dir.join("fixture-echo"), "#!/bin/sh\n").unwrap();

    let hash = compute_content_hash(&pkg_dir).unwrap();
    let hex_hash = hash.to_hex().to_string();

    // Protocol range [5, 10] does not include version 1.
    let manifest = format!(
        r#"schema_version = 1
package_id = "fixture.echo"
version = "0.1.0"
content_hash = "{hex_hash}"
entrypoint = "bin/fixture-echo"
protocol_range = [5, 10]
configuration_schema = '{{"type": "object"}}'

[[capabilities]]
capability_id = "fixture.echo"
version = "1.0.0"
name = "Echo"
request_schema = '{{"type": "object"}}'
response_schema = '{{"type": "object"}}'

[max_concurrency]
"fixture.echo" = 10
"#
    );

    std::fs::write(pkg_dir.join("manifest.toml"), manifest).unwrap();

    let err = validate_package(&pkg_dir).unwrap_err();
    assert!(
        matches!(
            err,
            PackageValidationError::ProtocolRange { min: 5, max: 10 }
        ),
        "expected ProtocolRange {{ min: 5, max: 10 }}, got: {err:?}"
    );
}

#[test]
fn content_hash_mismatch_fails() {
    let tmp = TempDir::new().unwrap();
    let pkg_dir = tmp.path().join("hash-mismatch");
    std::fs::create_dir_all(&pkg_dir).unwrap();

    // Create a valid package first.
    let bin_dir = pkg_dir.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    std::fs::write(bin_dir.join("fixture-echo"), "#!/bin/sh\necho hello\n").unwrap();

    let hash = compute_content_hash(&pkg_dir).unwrap();
    let hex_hash = hash.to_hex().to_string();

    let manifest = format!(
        r#"schema_version = 1
package_id = "fixture.echo"
version = "0.1.0"
content_hash = "{hex_hash}"
entrypoint = "bin/fixture-echo"
protocol_range = [1, 1]
configuration_schema = '{{"type": "object"}}'

[[capabilities]]
capability_id = "fixture.echo"
version = "1.0.0"
name = "Echo"
request_schema = '{{"type": "object"}}'
response_schema = '{{"type": "object"}}'

[max_concurrency]
"fixture.echo" = 10
"#
    );

    std::fs::write(pkg_dir.join("manifest.toml"), manifest).unwrap();

    // Now flip a byte in the data file to cause a hash mismatch.
    std::fs::write(bin_dir.join("fixture-echo"), "#!/bin/sh\necho TAMPERED\n").unwrap();

    let err = validate_package(&pkg_dir).unwrap_err();
    assert!(
        matches!(err, PackageValidationError::ContentHashMismatch { .. }),
        "expected ContentHashMismatch, got: {err:?}"
    );
}

#[test]
fn namespaced_id_rejects_uppercase() {
    let tmp = TempDir::new().unwrap();
    let pkg_dir = tmp.path().join("uppercase-id");
    std::fs::create_dir_all(&pkg_dir).unwrap();

    let bin_dir = pkg_dir.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    std::fs::write(bin_dir.join("fixture-echo"), "#!/bin/sh\n").unwrap();

    let hash = compute_content_hash(&pkg_dir).unwrap();
    let hex_hash = hash.to_hex().to_string();

    // Uppercase in package_id should be rejected.
    let manifest = format!(
        r#"schema_version = 1
package_id = "Fixture.Echo"
version = "0.1.0"
content_hash = "{hex_hash}"
entrypoint = "bin/fixture-echo"
protocol_range = [1, 1]
configuration_schema = '{{"type": "object"}}'

[[capabilities]]
capability_id = "fixture.echo"
version = "1.0.0"
name = "Echo"
request_schema = '{{"type": "object"}}'
response_schema = '{{"type": "object"}}'

[max_concurrency]
"fixture.echo" = 10
"#
    );

    std::fs::write(pkg_dir.join("manifest.toml"), manifest).unwrap();

    let err = validate_package(&pkg_dir).unwrap_err();
    assert!(
        matches!(err, PackageValidationError::PackageId(ref id) if id == "Fixture.Echo"),
        "expected PackageId(\"Fixture.Echo\"), got: {err:?}"
    );
}
