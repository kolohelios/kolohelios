#!/usr/bin/env bash
set -euo pipefail
[[ "$*" == '--disable --config -' ]]
input=$(cat)
[[ "$input" == *'Cookie: session='* ]]
if [[ "$NOTES_TEST_SCENARIO" == failure ]]; then
  printf '%s\n' 'test-only-session-secret' >&2
  exit 22
fi
n=0
if [[ -f "$NOTES_TEST_COUNTER" ]]; then read -r n < "$NOTES_TEST_COUNTER"; fi
n=$((n + 1))
printf '%s\n' "$n" > "$NOTES_TEST_COUNTER"
case "$NOTES_TEST_SCENARIO:$n" in
  clean:*|pagination:2|repair:3) response=clean ;;
  stale:*) response=stale ;;
  pagination:1) response=page ;;
  repair:1) response=stale ;;
  repair:2|hook-create:*|hook-update:*) response=status ;;
  search:*) response=search ;;
  *) exit 8 ;;
esac
cat "$NOTES_TEST_FIXTURES/responses/$response.json"
