# ./modules/opencode/default.nix

{ ... }: {
  imports = [
    ./mcp
    ./plugin
  ];

  opencode = {
    enable = true;

    settings = {
      compaction = {
        auto = true;
        prune = true;
      };

      plugin = [
        "oh-my-openagent@4.18.2"
      ];

      permission = {
        external_directory = "ask";
      };
    };

    rules = ''
    '';
  };
}
