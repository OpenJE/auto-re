# ./devenv/scripts/check-stage1.nix

{ ... }: {
  scripts.check-stage1 = {
    description = "Build, test, and lint the complete Stage 1 workspace";

    exec = /* bash */ ''
      set -euo pipefail

      : "''${PROTOC:?PROTOC is not configured}"

      cargo build --workspace
      cargo test --workspace
      cargo clippy --workspace --all-targets -- -D warnings

      cargo test \
        -p autore-tui \
        --test pty_integration \
        -- \
        --ignored \
        --nocapture
    '';
  };
}
