# ./devenv/scripts/check.nix

{ ... }: {
  scripts.check = {
    description = "Format, test, and lint the default auto-re workspace";

    exec = /* bash */ ''
      set -euo pipefail

      cargo fmt --all --check
      cargo test --workspace --exclude autore-stage1
      cargo clippy --workspace --all-targets -- -D warnings
    '';
  };
}
