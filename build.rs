fn main() {
    // idax handles its own build (C++ wrapper compilation, IDA SDK discovery,
    // and linkage) via its idax-sys dependency. No build script logic is needed
    // in the consumer crate.
    //
    // When the `ida` feature is disabled, idax is not compiled at all.
    // When enabled, idax-sys automatically:
    //   1. Locates the IDA installation ($IDADIR or standard paths)
    //   2. Compiles the C++ idax wrapper library
    //   3. Links against the IDA SDK
    //
    // If IDA is not found, idax-sys will print a cargo warning.
}
