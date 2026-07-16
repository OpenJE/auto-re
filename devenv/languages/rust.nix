# ./modules/languages/rust.nix

{ pkgs, ... }: {
  languages.rust = {
    enable = true;
    lsp.enable = true;
  };
}
