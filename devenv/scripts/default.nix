# ./devenv/scripts/default.nix

{ ... }: {
  imports = [
    ./check.nix
    ./check-stage1.nix
  ];
}
