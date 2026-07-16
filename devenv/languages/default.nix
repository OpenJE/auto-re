# ./modules/languages/default.nix

{ ... }: {
  imports = [
    ./nix.nix
    ./rust.nix
  ];
}
