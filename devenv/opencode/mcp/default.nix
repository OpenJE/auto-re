# ./modules/automation/mcp-servers/default.nix

{ ... }: {
  imports = [
    ./git.nix
    ./jj.nix
    ./devenv.nix
    ./nixos.nix
    #./github.nix
    ./codegraph.nix
  ];
}
