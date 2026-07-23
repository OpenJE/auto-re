# ./modules/packages/default.nix

{ pkgs, ... }:
let
  cargo-mcp = pkgs.callPackage ./cargo-mcp.nix {};
in {
  imports = [
    ./mcp-nixos.nix
    ./codegraph.nix
    ./opencode.nix
    #./github-mcp.nix
  ];

  packages = [
    cargo-mcp
  ] ++ (with pkgs; [
    protobuf
    buf
    cmake
    ninja
    pkg-config
    clang
    llvm
    docker
    cargo-nextest
    jq
    sqlite
  ]);
}
