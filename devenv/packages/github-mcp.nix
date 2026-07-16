# ./modules/packages/mcp-nixos.nix

{ pkgs, ... }: {
  packages = with pkgs; [
    github-mcp-server
  ];
}
