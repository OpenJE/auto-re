{ lib, fetchFromGitHub, rustPlatform }:
  rustPlatform.buildRustPackage rec {
    pname = "cargo-mcp";
    version = "0.2.0";

    src = fetchFromGitHub {
      owner = "MikeGrier";
      repo = "cargo-mcp-rs";
      rev = "v${version}";
      hash = "sha256-+DjNC1KZmGnBE4wMCDyhGCSfR+Z3L3Ckgv1LUwcAoYQ=";
    };

    cargoHash = "sha256-cEV4d6TIjw5uSG8SbGvZKGIQXP1JcF+pPZIv3f9lqO0=";
    buildAndTestSubdir = "crates/cargo-mcp";
    doCheck = false;

    meta = {
      description = "MCP server for Cargo/Rust projects";
      homepage = "https://github.com/MikeGrier/cargo-mcp-rs";
      license = lib.licenses.mit;
      mainProgram = "cargo-mcp";
    };
  }
