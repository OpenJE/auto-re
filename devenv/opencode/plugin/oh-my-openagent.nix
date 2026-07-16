# ./modules/opencode/oh-my-openagent.nix

{ ... }: {
  files.".opencode/oh-my-openagent.jsonc".text = builtins.toJSON {
    "$schema" = "https://raw.githubusercontent.com/code-yeongyu/oh-my-openagent/v4.15.1/assets/oh-my-opencode.schema.json";
    team_mode = {
      enabled               = true;
      max_parallel_members  = 4;
      max_members           = 8;
      tmux_visualization    = true;
    };
    agents = {
      sisyphus.model =          "opencode-go/kimi-k2.7-code";
      atlas.model =             "opencode-go/kimi-k2.7-code";
      sisyphus-junior.model =   "opencode-go/kimi-k2.7-code";
      multimodal-looker.model = "opencode-go/qwen3.7-plus";
      prometheus.model =        "opencode-go/glm-5.2";
      metis.model =             "opencode-go/qwen3.7-plus";
      oracle.model =            "opencode-go/qwen3.7-max";
      momus.model =             "opencode-go/qwen3.7-plus";
      librarian.model =         "opencode-go/deepseek-v4-flash";
      explore.model =           "opencode-go/deepseek-v4-flash";
      hephaestus.model =        "opencode-go/kimi-k2.7-code";
    };
    categories = {
      visual-engineering.model = "opencode-go/qwen3.7-plus";
      ultrabrain.model =         "opencode-go/qwen3.7-max";
      deep.model =               "opencode-go/kimi-k2.7-code";
      artistry.model =           "opencode-go/qwen3.7-max";
      quick.model =              "opencode-go/deepseek-v4-flash";
      unspecified-high.model =   "opencode-go/qwen3.7-max";
      unspecified-low.model =    "opencode-go/deepseek-v4-flash";
      writing.model =            "opencode-go/qwen3.7-plus";
    };
  };
}
