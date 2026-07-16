# ./modules/packages/mcp-nixos.nix

{ pkgs, ... }: {
  packages = with pkgs; [
    mcp-nixos
  ];
}
