# ./modules/automation/mcp-servers/nixos.nix

{ ... }: {
  opencode.mcp.nixos = {
    type = "local";
    command = [ "mcp-nixos" ];
  };
}
