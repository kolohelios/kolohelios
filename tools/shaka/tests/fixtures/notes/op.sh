#!/usr/bin/env bash
set -euo pipefail
[[ "$*" == 'read --no-newline op://test/session/password' ]]
printf '%s' 'test-only-session-secret'
