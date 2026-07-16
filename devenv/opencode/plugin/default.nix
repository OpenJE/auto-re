# ./modules/opencode/default.nix

{ ... }: {
  imports = [
    ./oh-my-openagent.nix
    #./oh-my-openagent-local.nix
  ];
}
