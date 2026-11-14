#!/usr/bin/env bash
set -euo pipefail
case "$*" in
  *'--paginate --slurp')
    if [[ "$NOTES_TEST_SCENARIO" == hook-update ]]; then response=hooks; else response=empty-hooks; fi
    cat "$NOTES_TEST_FIXTURES/responses/$response.json"
    ;;
  *'--input -')
    input=$(cat)
    [[ "$input" != *'test-only-session-secret'* ]]
    [[ "$input" == *'"secret":'* ]]
    if [[ "$NOTES_TEST_SCENARIO" == hook-update ]]; then [[ "$*" == *'--method PATCH'* ]]; else [[ "$*" == *'--method POST'* ]]; fi
    cat "$NOTES_TEST_FIXTURES/responses/ok.json"
    ;;
  *) exit 7 ;;
esac
