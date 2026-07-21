use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../proto");

    let protos = &[
        proto_root.join("autore/provider/v1/provider.proto"),
        proto_root.join("autore/provider/v1/bootstrap.proto"),
        proto_root.join("autore/provider/v1/capability.proto"),
        proto_root.join("autore/provider/v1/event.proto"),
        proto_root.join("autore/provider/v1/execution.proto"),
        proto_root.join("autore/provider/v1/health.proto"),
        proto_root.join("autore/provider/v1/package.proto"),
    ];

    let includes = &[proto_root];

    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(protos, includes)?;

    for proto in protos {
        println!("cargo:rerun-if-changed={}", proto.display());
    }

    Ok(())
}
