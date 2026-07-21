//! Generated gRPC protocol types for the `autore.provider.v1` package.
//!
//! This crate contains only proto codegen and shared type re-exports.
//! No business logic or runtime behavior.

/// Protocol version 1 types generated from `proto/autore/provider/v1/*.proto`.
pub mod v1 {
    tonic::include_proto!("autore.provider.v1");
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    /// Asserts that the `package autore.provider.v1;` declaration is present
    /// in the proto source files, ensuring the versioned package suffix.
    #[test]
    fn version_suffix_present() {
        let proto_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../proto/autore/provider/v1");

        let provider_proto = proto_dir.join("provider.proto");
        let content =
            std::fs::read_to_string(&provider_proto).expect("failed to read provider.proto");

        assert!(
            content.contains("package autore.provider.v1;"),
            "provider.proto must declare 'package autore.provider.v1;' — found content:\n{content}"
        );

        // Verify all proto files in the v1 directory declare the same package.
        for entry in std::fs::read_dir(&proto_dir).expect("failed to read proto dir") {
            let entry = entry.expect("failed to read dir entry");
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("proto") {
                let file_content =
                    std::fs::read_to_string(&path).expect("failed to read proto file");
                assert!(
                    file_content.contains("package autore.provider.v1;"),
                    "{} must declare 'package autore.provider.v1;'",
                    path.display()
                );
            }
        }
    }
}
