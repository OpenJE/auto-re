# ./devenv.local.nix

{ ... }: {
  opencode.settings.provider = {
    opencode.options = {
      apiKey = "{file:/run/secrets/opencode-api-key-personal}";
    };

    opencode-go.options = {
      apiKey = "{file:/run/secrets/opencode-api-key-personal}";
    };
  };
}
