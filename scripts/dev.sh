#!/usr/bin/env bash
# Local development entrypoint for prompt-ferry.
# Usage: bash scripts/dev.sh <help|backend|full>
#
# Loads the root .env as data, applies development defaults, and delegates the
# relay/worker/frontend process lifecycle to scripts/dev-supervisor.sh.

if [ -z "${BASH_VERSION:-}" ]; then
  printf 'scripts/dev.sh must run under bash: bash scripts/dev.sh <help|backend|full>\n' >&2
  exit 1
fi

set -euo pipefail

if [ "${BASH_VERSINFO[0]}" -lt 4 ]; then
  printf 'scripts/dev.sh needs bash 4 or newer (associative arrays); found %s\n' "$BASH_VERSION" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DOTENV_FILE="$ROOT/.env"
SUPERVISOR="$ROOT/scripts/dev-supervisor.sh"

declare -A DOTENV=()

usage() {
  cat <<'EOF'
Usage: bash scripts/dev.sh <help|backend|full>

Commands:
  backend   Start relay and worker
  full      Start relay, worker, and frontend Vite dev server

Config:
  Reads root .env automatically and treats it as the source of truth
  DATABASE_URL is required
  Switch local/remote settings by editing or commenting values in .env
  PROMPT_FERRY_LOGGING__LEVEL defaults to info
  managed dev defaults include a local relay secret master key if unset
  frontend deps auto-install once with bun if frontend/node_modules is missing

Requirements:
  bash, cargo, and curl on PATH
  full mode additionally requires bun
EOF
}

die() {
  printf 'dev.sh: %s\n' "$1" >&2
  if [ "$#" -gt 1 ]; then
    printf '%s\n' "$2" >&2
  fi
  exit 1
}

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
# still reach the supervisor, as the previous Nushell `with-env` did.
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

# First entry of a JSON array value, or the value itself when it is a bare URL.
first_relay_url() {
  local value="$1" first
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  case "$value" in
    \[*)
      first="${value#\[}"
      first="${first%%,*}"
      first="${first%]}"
      first="${first#\"}"
      first="${first%\"}"
      first="${first#\'}"
      first="${first%\'}"
      printf '%s' "$first"
      ;;
    *)
      printf '%s' "$value"
      ;;
  esac
}

relay_is_local() {
  case "$1" in
    ws://127.0.0.1:* | wss://127.0.0.1:* | ws://localhost:* | wss://localhost:*) return 0 ;;
    *) return 1 ;;
  esac
}

print_startup_summary() {
  local mode="$1" relay_url="$2"
  printf 'Starting local dev stack (%s)\n' "$mode"
  if relay_is_local "$relay_url"; then
    printf '  relay               http://127.0.0.1:8787\n'
    printf '  worker listener     http://127.0.0.1:8788\n'
  else
    printf '  relay               external %s\n' "$relay_url"
  fi
  printf '  worker admin api    http://127.0.0.1:8789/api/v1\n'
  if [ "$mode" = "full" ]; then
    printf '  frontend            http://127.0.0.1:5171\n'
  fi
  printf 'Stop with Ctrl+C\n'
}

require_tools() {
  local mode="$1" tool
  local missing=()

  for tool in cargo curl; do
    if ! command -v "$tool" > /dev/null 2>&1; then
      missing+=("$tool")
    fi
  done
  if [ "$mode" = "full" ] && ! command -v bun > /dev/null 2>&1; then
    missing+=("bun")
  fi

  if [ "${#missing[@]}" -gt 0 ]; then
    die "missing required tool(s) for '$mode' mode: ${missing[*]}" \
      "Install them, then re-run: bash scripts/dev.sh $mode"
  fi
}

ensure_backend_binary() {
  printf 'Building prompt-ferry once before startup...\n'
  cargo build
}

ensure_frontend_deps() {
  if [ ! -d "$ROOT/frontend/node_modules" ]; then
    printf 'Installing frontend dependencies with bun...\n'
    (cd "$ROOT/frontend" && bun install --no-save)
  fi
}

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

run_stack() {
  local mode="$1"

  require_tools "$mode"
  [ -f "$SUPERVISOR" ] || die "missing $SUPERVISOR" "Run from a complete checkout of the repository."

  load_dotenv "$DOTENV_FILE"
  export_dotenv
  export_dev_env

  if is_blank "${DATABASE_URL:-}"; then
    die "DATABASE_URL is not set" "Set DATABASE_URL in .env, then re-run: bash scripts/dev.sh $mode"
  fi

  ensure_backend_binary
  if [ "$mode" = "full" ]; then
    ensure_frontend_deps
  fi

  print_startup_summary "$mode" "$(first_relay_url "${PROMPT_FERRY_WORKER__RELAY_URLS}")"

  exec bash "$SUPERVISOR" "$mode"
}

main() {
  if [ "$#" -gt 1 ]; then
    usage >&2
    die "expected a single command, got: $*" "Run: bash scripts/dev.sh help"
  fi

  local action="${1:-help}"
  case "$action" in
    help) usage ;;
    backend | full) run_stack "$action" ;;
    *) usage >&2; die "unknown command: $action" "Run: bash scripts/dev.sh help" ;;
  esac
}

main "$@"
