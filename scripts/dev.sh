#!/usr/bin/env bash
# Local development entrypoint for prompt-ferry.
# Usage: bash scripts/dev.sh <help|backend|full>
#
# Loads the root .env as data via scripts/dev-env.sh, applies development
# defaults, and delegates the node/frontend process lifecycle to
# scripts/dev-supervisor.sh.

if [ -z "${BASH_VERSION:-}" ]; then
  printf 'scripts/dev.sh must run under bash: bash scripts/dev.sh <help|backend|full>\n' >&2
  exit 1
fi

set -euo pipefail

if [ "${BASH_VERSINFO[0]}" -lt 4 ]; then
  printf 'scripts/dev.sh needs bash 4 or newer (associative arrays); found %s\n' "$BASH_VERSION" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

die() {
  printf 'dev.sh: %s\n' "$1" >&2
  if [ "$#" -gt 1 ]; then
    printf '%s\n' "$2" >&2
  fi
  exit 1
}

# The repository helper, never the .env file: it defines the parser and defaults.
ENV_HELPER="$SCRIPT_DIR/dev-env.sh"
[ -f "$ENV_HELPER" ] || die "missing $ENV_HELPER" "Run from a complete checkout of the repository."
source "$ENV_HELPER"

DOTENV_FILE="${PROMPT_FERRY_DOTENV:-$ROOT/.env}"
SUPERVISOR="$ROOT/scripts/dev-supervisor.sh"

usage() {
  cat <<'EOF'
Usage: bash scripts/dev.sh <help|backend|full>

Commands:
  backend   Start the node
  full      Start the node and frontend Vite dev server

Config:
  Reads PROMPT_FERRY_DOTENV or root .env as data
  PROMPT_FERRY__MASTER_SECRET is required and must remain stable
  Vite proxies /api/ to 127.0.0.1:8787; use the default bind for full mode
  frontend deps auto-install once with bun if frontend/node_modules is missing

Requirements:
  bash, cargo, and curl on PATH
  full mode additionally requires bun
EOF
}

print_startup_summary() {
  printf 'Starting node on %s (%s)\n' "$PROMPT_FERRY__BIND" "$1"
  [ "$1" != full ] || printf '  frontend http://127.0.0.1:5171\n'
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

run_stack() {
  local mode="$1"

  require_tools "$mode"
  [ -f "$SUPERVISOR" ] || die "missing $SUPERVISOR" "Run from a complete checkout of the repository."

  load_dotenv "$DOTENV_FILE"
  export_dotenv
  export_dev_env

  local key
  for key in ${!PROMPT_FERRY_RELAY__@} ${!PROMPT_FERRY_WORKER__@}; do
    die "legacy environment namespace detected: $key" "Use a separate node environment; see MIGRATING.md."
  done
  if is_blank "${PROMPT_FERRY__MASTER_SECRET:-}"; then
    die "PROMPT_FERRY__MASTER_SECRET is required" "Configure a stable secret; see README.md."
  fi
  if [ "$mode" = full ] && { [ "$PROMPT_FERRY__BIND" != 127.0.0.1:8787 ] || [ "${PROMPT_FERRY__TLS_MODE:-off}" != off ]; }; then
    die "full mode requires plaintext on 127.0.0.1:8787" "Vite proxies to that local address; use a trusted development environment."
  fi

  ensure_backend_binary
  if [ "$mode" = "full" ]; then
    ensure_frontend_deps
  fi

  print_startup_summary "$mode"

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
