#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" != "worker" ]]; then
    exec /usr/local/bin/prompt-ferry "$@"
fi

shift

export PROMPT_FERRY_WORKER__ADMIN_BIND="${PROMPT_FERRY_WORKER__ADMIN_BIND:-0.0.0.0:80}"

admin_bind_args=(--admin-bind "$PROMPT_FERRY_WORKER__ADMIN_BIND")
for arg in "$@"; do
    if [[ "$arg" == "--admin-bind" || "$arg" == --admin-bind=* ]]; then
        admin_bind_args=()
        break
    fi
done

exec /usr/local/bin/prompt-ferry worker "${admin_bind_args[@]}" "$@"
