#!/bin/sh
set -eu
if [ "${NOVELAI_API_TOKEN+x}" = x ] || [ "${OPENROUTER_API_KEY+x}" = x ]; then
    printf '%s\n' 'Provider credentials reached the editor' >&2
    exit 8
fi
cp "$BLOGCTL_TEST_BODY" "$1"
