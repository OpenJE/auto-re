# ./modules/automation/mcp-servers/tauri.nix

{ ... }: {
  opencode.mcp.git = {
    type = "local";
    command = [
      "npx"
      "-y"
      "@cyanheads/git-mcp-server@2.15.1"
    ];
  };
}

