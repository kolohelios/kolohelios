#!/usr/bin/env bash
set -euo pipefail
[[ "$*" == 'read op://vault/item/password' ]]
printf '%s\n' 'test-credential-do-not-log'
