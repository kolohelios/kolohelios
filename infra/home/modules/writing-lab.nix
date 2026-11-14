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
  authenticate = command: ''
    set +x
    if [[ -n "''${NOVELAI_API_TOKEN:-}" ]]; then
      exec ${command}
    fi
    reference="''${NOVELAI_API_TOKEN_REF:-}"
    if [[ -z "$reference" ]]; then
      reference_file="$HOME/.config/blogctl/novelai-token-reference"
      if [[ ! -f "$reference_file" || -L "$reference_file" ]]; then
        echo 'Configure a private 1Password token reference; see the writing runbook.' >&2
        exit 1
      fi
      reference="$(< "$reference_file")"
    fi
    if [[ "$reference" != op://* || "$reference" == *$'\n'* || "$reference" == *$'\r'* ]]; then
      echo 'Invalid 1Password reference (value suppressed).' >&2
      exit 1
    fi
    export NOVELAI_API_TOKEN="$reference"
    exec ${pkgs._1password-cli}/bin/op run -- ${command}
  '';
  models = pkgs.writeShellApplication {
    name = "blogctl-writing-models";
    text = authenticate "${cfg.package}/bin/blogctl novelai models --json";
  };
  provider = pkgs.writeShellApplication {
    name = "blogctl-novelai";
    text = ''
      set +x
      for arg in "$@"; do
        if [[ "$arg" == --help || "$arg" == -h ]]; then
          exec ${cfg.package}/bin/blogctl novelai "$@"
        fi
      done
      needs_token=false
      case "''${1:-}" in
        models) needs_token=true ;;
        generate)
          for arg in "$@"; do
            if [[ "$arg" == --run ]]; then needs_token=true; fi
          done
          ;;
      esac
      if [[ "$needs_token" == false ]]; then
        exec ${cfg.package}/bin/blogctl novelai "$@"
      fi
      ${authenticate "${cfg.package}/bin/blogctl novelai \"$@\""}
    '';
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
