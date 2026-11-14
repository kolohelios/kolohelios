cp .env.example .env
nix develop . --command bash -c '
  export PATH="$CFDEPLOY_SHAKA:$PATH"
  args=(ci mask-and-run --env-file=.env)
  if [[ "$PIPE_TEST_RETRIES" == 1 ]]; then
    args+=(--retry-on transient-pipe-test --max-retries 1 --retry-delay 0)
  fi
  exec shaka "${args[@]}" -- wrangler deploy --dry-run
' 2>&1 | tee "$PIPE_TEST_LOG"
exit "${PIPESTATUS[0]}"
