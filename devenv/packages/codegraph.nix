# ./modules/packages/mcp-nixos.nix

{ inputs, pkgs, ... }: {
  packages = [
    inputs.codegraph.legacyPackages.${pkgs.stdenv.hostPlatform.system}.codegraph
  ];
}
