# ./modules/opencode/oh-my-openagent.nix

{ ... }:
let
  omoVersion  = "4.16.2";
  provider    = "localserver";
  leadModel   = "qwen36-27b-gguf";
  workerModel = "ornith-35b";
in {
  files.".opencode/oh-my-openagent.jsonc".text = builtins.toJSON {
    "$schema" = "https://raw.githubusercontent.com/code-yeongyu/oh-my-openagent/v${omoVersion}/assets/oh-my-opencode.schema.json";

    team_mode = {
      enabled              = true;
      max_parallel_members = 6;
      max_members          = 8;
      tmux_visualization   = false;
    };

    background_task = {
      defaultConcurrency = 6;
      staleTimeoutMs     = 300000;

      providerConcurrency = {
        "${provider}" = 6;
      };

      modelConcurrency = {
        "${provider}/${leadModel}" = 2;
        "${provider}/${workerModel}" = 4;
      };
    };

    agents = {
      sisyphus = {
        model = "${provider}/${leadModel}";
      };
      atlas = {
        model = "${provider}/${leadModel}";
      };
      sisyphus-junior = {
        model = "${provider}/${workerModel}";
      };
      multimodal-looker = {
        model = "${provider}/${leadModel}";
      };
      prometheus = {
        model = "${provider}/${leadModel}";
      };
      metis = {
        model = "${provider}/${leadModel}";
      };
      oracle = {
        model = "${provider}/${leadModel}";
      };
      momus = {
        model = "${provider}/${leadModel}";
      };
      librarian = {
        model = "${provider}/${workerModel}";
      };
      explore = {
        model = "${provider}/${workerModel}";
      };
      hephaestus = {
        model = "${provider}/${workerModel}";
      };
    };

    agents.hephaestus.allow_non_gpt_model = true;

    categories = {
      visual-engineering = {
        model = "${provider}/${leadModel}";
      };
      ultrabrain = {
        model = "${provider}/${leadModel}";
      };
      deep = {
        model = "${provider}/${workerModel}";
      };
      artistry = {
        model = "${provider}/${leadModel}";
      };
      quick = {
        model = "${provider}/${workerModel}";
      };
      unspecified-high = {
        model = "${provider}/${workerModel}";
      };
      unspecified-low = {
        model = "${provider}/${workerModel}";
      };
      writing = {
        model = "${provider}/${leadModel}";
      };
    };
  };
}
