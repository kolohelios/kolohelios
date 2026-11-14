{
  config,
  pkgs,
  lib,
  blogctl,
  ...
}:
let
  cfg = config.programs.writing-lab;
  root = "${config.home.homeDirectory}/.local/share/blogctl-writing";
  authenticate = command: import ../packages/writing-credentials.nix { inherit pkgs command; };
  models = pkgs.writeShellApplication {
    name = "blogctl-writing-models";
    text = authenticate "${cfg.package}/bin/blogctl novelai models --json";
  };
  provider = pkgs.writeShellApplication {
    name = "blogctl-novelai";
    text = import ../packages/writing-provider.nix {
      engine = "${cfg.package}/bin/blogctl";
      inherit authenticate;
    };
  };
in
{
  options.programs.writing-lab = {
    enable = lib.mkEnableOption "private blogctl writing and read-only agent integration";
    package = lib.mkOption {
      type = lib.types.package;
      default = blogctl.packages.${pkgs.stdenv.hostPlatform.system}.default;
      description = "Published blogctl with the writing MCP, review, and migration commands.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [
      cfg.package
      provider
      pkgs._1password-cli
      pkgs.codex
    ];
    home.activation.writingLab = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
      if [[ -L ${lib.escapeShellArg root} ]]; then
        echo 'The private writing data directory must not be a symlink.' >&2
        exit 1
      fi
      run mkdir -p ${lib.escapeShellArg root}
      run chmod 700 ${lib.escapeShellArg root}
      run ${cfg.package}/bin/blogctl mcp install \
        --home ${lib.escapeShellArg config.home.homeDirectory} \
        --command ${cfg.package}/bin/blogctl \
        --workdir ${lib.escapeShellArg root} \
        --models-command ${models}/bin/blogctl-writing-models
    '';
  };
}
