# Select the credential boundary only for commands that send API requests.
{
  engine,
  authenticate,
}:
''
  set +x
  for arg in "$@"; do
    if [[ "$arg" == --help || "$arg" == -h ]]; then
      exec ${engine} novelai "$@"
    fi
  done
  needs_token=false
  case "''${1:-}" in
    models) needs_token=true ;;
    generate)
      needs_token=true
      for arg in "$@"; do
        if [[ "$arg" == --dry-run ]]; then needs_token=false; fi
      done
      ;;
  esac
  if [[ "$needs_token" == false ]]; then
    exec ${engine} novelai "$@"
  fi
  ${authenticate "${engine} novelai \"$@\""}
''
