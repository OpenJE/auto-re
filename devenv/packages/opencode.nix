# ./modules/packages/mcp-nixos.nix

{ inputs, pkgs, ... }: {
  packages = [
    inputs.opencode.legacyPackages.${pkgs.stdenv.hostPlatform.system}.opencode
  ];
}
