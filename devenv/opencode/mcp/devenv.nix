# ./modules/automation/mcp-servers/devenv.nix

{ config, ... }: {
  opencode.mcp.devenv = {
    type = "local";
    command = [ "devenv" "mcp" ];
    environment = {
      DEVENV_ROOT = config.devenv.root;
    };
  };
}
