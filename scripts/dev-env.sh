#!/usr/bin/env bash
# Development environment resolution for scripts/dev.sh.
#
# Sourced by the entrypoint: defines the dotenv parser, the export helpers, and
# the development defaults. It reads the root .env as data and never runs it.

declare -A DOTENV=()

is_blank() {
  [ -z "${1//[[:space:]]/}" ]
}

# Parses KEY=VALUE lines as plain data. Values are never expanded, evaluated, or
# executed, so a .env file cannot run commands in this shell.
load_dotenv() {
  local file="$1" line key value
  [ -f "$file" ] || return 0

  while IFS= read -r line || [ -n "$line" ]; do
    line="${line%$'\r'}"
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    case "$line" in
      '' | '#'*) continue ;;
    esac

    key="${line%%=*}"
    [ "$key" != "$line" ] || continue
    [[ "$key" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] || continue

    value="${line#*=}"
    value="${value#"${value%%[![:space:]]*}"}"
    value="${value%"${value##*[![:space:]]}"}"
    if [ "${#value}" -ge 2 ]; then
      case "$value" in
        \"*\" | \'*\') value="${value:1:${#value}-2}" ;;
      esac
    fi

    DOTENV["$key"]="$value"
  done < "$file"
}

# Exports every parsed .env entry so keys this entrypoint knows nothing about
# still reach the supervisor unchanged.
export_dotenv() {
  local key
  for key in "${!DOTENV[@]}"; do
    export "$key=${DOTENV[$key]}"
  done
}

# Known keys resolve as .env wins, then ambient, then the built-in default. A
# blank .env value is authoritative and falls through to the default, because
# export_dotenv has already cleared any ambient value for that key.
resolve() {
  local key="$1" fallback="$2" value
  value="${DOTENV[$key]:-}"
  if [ -n "$value" ] && ! is_blank "$value"; then
    printf '%s' "$value"
    return 0
  fi
  value="${!key:-}"
  if [ -n "$value" ] && ! is_blank "$value"; then
    printf '%s' "$value"
    return 0
  fi
  printf '%s' "$fallback"
}

# The managed development defaults, applied on top of the exported .env entries.
export_dev_env() {
  export PROMPT_FERRY_LOGGING__LEVEL="$(resolve PROMPT_FERRY_LOGGING__LEVEL info)"
  export DATABASE_URL="$(resolve DATABASE_URL '')"
  export PROMPT_FERRY_RELAY__BIND="$(resolve PROMPT_FERRY_RELAY__BIND 127.0.0.1:8787)"
  export PROMPT_FERRY_RELAY__WORKER_BIND="$(resolve PROMPT_FERRY_RELAY__WORKER_BIND 127.0.0.1:8788)"
  export PROMPT_FERRY_RELAY__CLIENT_TOKEN="$(resolve PROMPT_FERRY_RELAY__CLIENT_TOKEN dev-client-token)"
  export PROMPT_FERRY_RELAY__WORKER_TOKEN="$(resolve PROMPT_FERRY_RELAY__WORKER_TOKEN dev-worker-token)"
  export PROMPT_FERRY_RELAY__REQUEST_TIMEOUT_SECONDS="$(resolve PROMPT_FERRY_RELAY__REQUEST_TIMEOUT_SECONDS 300)"
  export PROMPT_FERRY_WORKER__RELAY_URLS="$(resolve PROMPT_FERRY_WORKER__RELAY_URLS '["ws://127.0.0.1:8788/ws/worker"]')"
  export PROMPT_FERRY_WORKER__WORKER_TOKEN="$(resolve PROMPT_FERRY_WORKER__WORKER_TOKEN dev-worker-token)"
  export PROMPT_FERRY_WORKER__ADMIN_BIND="$(resolve PROMPT_FERRY_WORKER__ADMIN_BIND 127.0.0.1:8789)"
  export PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_LOGIN="$(resolve PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_LOGIN admin)"
  export PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_PASSWORD="$(resolve PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_PASSWORD change-me-now)"
  export PROMPT_FERRY_WORKER__RELAY_SECRET_MASTER_KEY="$(resolve PROMPT_FERRY_WORKER__RELAY_SECRET_MASTER_KEY BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=)"
}
