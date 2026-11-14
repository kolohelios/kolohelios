#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == run ]]
shift
case "$1" in
  --env-file) shift 2 ;;
  --env-file=*) shift ;;
  *) exit 90 ;;
esac
[[ "$1" == -- ]]
shift
export PIPE_TEST_SECRET=fixture-credential-not-a-real-secret
exec "$@"
