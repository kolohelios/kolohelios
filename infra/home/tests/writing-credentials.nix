{ pkgs }:
let
  target = pkgs.writeShellScript "writing-target" ''
    printf '%s\n' target >> "$CALL_LOG"
  '';
  fakeOp =
    name:
    pkgs.writeShellScript "fake-${name}-op" ''
      printf '%s\n' ${name} >> "$CALL_LOG"
      [[ "$1" == run && "$2" == -- ]]
      shift 2
      exec "$@"
    '';
  marker = pkgs.writeText "nixos-marker" "";
  protected = fakeOp "protected";
  fallback = fakeOp "fallback";
  boundary =
    options:
    pkgs.writeShellApplication {
      name = "writing-credentials-test";
      text = import ../packages/writing-credentials.nix (
        {
          inherit pkgs;
          command = toString target;
          fallbackOp = toString fallback;
        }
        // options
      );
    };
  nixos = boundary {
    nixosMarker = toString marker;
    protectedOp = toString protected;
  };
  missing = boundary {
    nixosMarker = toString marker;
    protectedOp = "/no-system-op";
  };
  other = boundary {
    nixosMarker = "/no-nixos-marker";
    protectedOp = toString protected;
  };
in
pkgs.runCommand "writing-credentials-routing-test" { nativeBuildInputs = [ pkgs.gnugrep ]; } ''
  export HOME="$TMPDIR/home"
  export CALL_LOG="$TMPDIR/calls"
  export NOVELAI_API_TOKEN_REF='op://Synthetic/NovelAI/password'
  unset NOVELAI_API_TOKEN
  mkdir -p "$HOME/.config/blogctl"

  ${nixos}/bin/writing-credentials-test
  printf 'protected\ntarget\n' > expected
  cmp expected "$CALL_LOG"

  : > "$CALL_LOG"
  if ${missing}/bin/writing-credentials-test > stdout 2> stderr; then
    echo 'Missing protected NixOS wrapper must fail closed' >&2
    exit 1
  fi
  test ! -s "$CALL_LOG"
  grep -q 'programs._1password.enable' stderr

  ${other}/bin/writing-credentials-test
  printf 'fallback\ntarget\n' > expected
  cmp expected "$CALL_LOG"

  : > "$CALL_LOG"
  NOVELAI_API_TOKEN=synthetic-token ${pkgs.bash}/bin/bash -x ${missing}/bin/writing-credentials-test > stdout 2> stderr
  printf 'target\n' > expected
  cmp expected "$CALL_LOG"
  if grep -q synthetic-token stderr; then
    echo 'Shell tracing must not expose credentials' >&2
    exit 1
  fi

  : > "$CALL_LOG"
  if NOVELAI_API_TOKEN_REF=invalid ${nixos}/bin/writing-credentials-test > stdout 2> stderr; then
    exit 1
  fi
  test ! -s "$CALL_LOG"

  unset NOVELAI_API_TOKEN_REF
  printf '%s\n' 'op://Synthetic/NovelAI/password' > "$HOME/.config/blogctl/novelai-token-reference"
  ${nixos}/bin/writing-credentials-test
  printf 'protected\ntarget\n' > expected
  cmp expected "$CALL_LOG"

  touch "$out"
''
