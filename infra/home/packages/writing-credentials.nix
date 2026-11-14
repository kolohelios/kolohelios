# Render the invocation boundary separately so routing can be tested without
# credentials, host mutation, or a real 1Password/NovelAI request.
{
  pkgs,
  command,
  nixosMarker ? "/etc/NIXOS",
  protectedOp ? "/run/wrappers/bin/op",
  fallbackOp ? "${pkgs._1password-cli}/bin/op",
}:
''
  set +x
  if [[ -n "''${NOVELAI_API_TOKEN:-}" ]]; then
    exec ${command}
  fi
  if [[ -e ${pkgs.lib.escapeShellArg nixosMarker} ]]; then
    op_command=${pkgs.lib.escapeShellArg protectedOp}
    if [[ ! -x "$op_command" ]]; then
      echo 'NixOS 1Password CLI integration needs programs._1password.enable = true in the host configuration and a system rebuild; no request was sent.' >&2
      exit 1
    fi
  else
    op_command=${pkgs.lib.escapeShellArg fallbackOp}
  fi
  reference="''${NOVELAI_API_TOKEN_REF:-}"
  if [[ -z "$reference" ]]; then
    reference_file="$HOME/.config/blogctl/novelai-token-reference"
    if [[ ! -f "$reference_file" || -L "$reference_file" ]]; then
      echo 'Configure a private 1Password token reference; see the writing guide.' >&2
      exit 1
    fi
    reference="$(< "$reference_file")"
  fi
  if [[ "$reference" != op://* || "$reference" == *$'\n'* || "$reference" == *$'\r'* ]]; then
    echo 'Invalid 1Password reference (value suppressed).' >&2
    exit 1
  fi
  export NOVELAI_API_TOKEN="$reference"
  exec "$op_command" run -- ${command}
''
