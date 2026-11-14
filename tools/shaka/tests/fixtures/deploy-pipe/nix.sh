#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == develop && "$2" == . && "$3" == --command ]]
shift 3
# Model the confirmed Nix/Crashpad failure: this descendant inherits the
# pipeline's output descriptors but outlives the consumer and Nix itself.
if [[ "${NIX_SENTRY_ENDPOINT-unset}" != "" ]]; then
  "$PIPE_TEST_BASH" "$PIPE_TEST_HOLDER" <&0 &
fi
# Wait for the real consumer so the negative control can synchronize on
# actual process termination, not Wrangler's printed completion notice.
if "$@"; then
  status=0
else
  status=$?
fi
printf 'consumer-process-exited-%s\n' "$status"
exit "$status"
