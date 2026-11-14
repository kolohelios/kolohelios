#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == deploy ]]
printf '%s\n' "$@" > "$PIPE_TEST_ARGS"
attempt=0
if [[ -f "$PIPE_TEST_COUNTER" ]]; then
  IFS= read -r attempt < "$PIPE_TEST_COUNTER"
fi
attempt=$((attempt + 1))
printf '%s\n' "$attempt" > "$PIPE_TEST_COUNTER"
printf 'consumer stdout secret=%s\n' "$PIPE_TEST_SECRET"
printf 'consumer stderr secret=%s\n' "$PIPE_TEST_SECRET" >&2
# Deliberately print this even on failure: a log line is not an exit status.
printf -- '--dry-run: exiting now.\n'
printf 'consumer-finished-%s\n' "$attempt"
if [[ "$PIPE_TEST_RETRIES" == 1 && "$attempt" == 1 ]]; then
  printf 'transient-pipe-test\n' >&2
  exit 23
fi
exit "$PIPE_TEST_STATUS"
