# ./devenv/opencode/mcp/github.nix

{ ... }: {
  opencode.mcp.github = {
    type = "local";
    command = [
      "github-mcp-server"
      "stdio"
      "--read-only"
    ];
  };
}
