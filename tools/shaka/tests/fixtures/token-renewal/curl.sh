#!/usr/bin/env bash
set -euo pipefail
# Refuse leaked credentials in argv. The real curl receives them over stdin.
[[ "$*" != *test-credential-do-not-log* ]]
config=$(</dev/stdin)
[[ "$config" == *'url = "https://api.cloudflare.com/client/v4/user/tokens/verify"'* ]]
[[ "$config" == *'header = "Authorization: Bearer test-credential-do-not-log"'* ]]
if [[ "${VERIFY_CURL_FAILURE:-0}" == 1 ]]; then
  printf '%s\n' 'test-credential-do-not-log' >&2
  exit 1
fi
while IFS= read -r line; do printf '%s\n' "$line"; done < "$VERIFY_RESPONSE_FILE"
