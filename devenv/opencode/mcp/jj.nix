# ./modules/automation/mcp-servers/tauri.nix

{ ... }: {
  opencode.mcp.jj = {
    type = "local";
    command = [
      "npx"
      "-y"
      "jj-mcp@1.0.5"
    ];
  };
}
