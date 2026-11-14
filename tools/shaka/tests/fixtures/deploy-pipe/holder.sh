#!/usr/bin/env bash
set -euo pipefail
printf 'pipe-holder-ready\n'
# No sleeps or polling: the test releases this descendant over stdin.
IFS= read -r release || true
