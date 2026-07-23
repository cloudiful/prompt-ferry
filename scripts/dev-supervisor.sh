#!/usr/bin/env bash

set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

mode="${1:-backend}"

services=()
pids=()
log_pids=()
tmp_files=()
stopping=0

gray=$'\033[90m'
red=$'\033[31m'
green=$'\033[32m'
yellow=$'\033[33m'
blue=$'\033[34m'
magenta=$'\033[35m'
cyan=$'\033[36m'
reset=$'\033[0m'

timestamp() {
  date '+%H:%M:%S'
}

log_line() {
  local service="$1"
  local color="$2"
  local line="$3"
  printf '%s%s%s %s[%s]%s %s\n' "$gray" "$(timestamp)" "$reset" "$color" "$service" "$reset" "$line"
}

prefix_output() {
  local service="$1"
  local color="$2"
  while IFS= read -r line; do
    log_line "$service" "$color" "$line"
  done
}

service_env=(
  "PROMPT_FERRY_LOGGING__LEVEL=${PROMPT_FERRY_LOGGING__LEVEL:-info}"
  "PROMPT_FERRY_DEV_DATABASE_URL=${PROMPT_FERRY_DEV_DATABASE_URL:-${PROMPT_FERRY_WORKER__DATABASE_URL:-}}"
  "PROMPT_FERRY_RELAY__BIND=${PROMPT_FERRY_RELAY__BIND:-127.0.0.1:8787}"
  "PROMPT_FERRY_RELAY__WORKER_BIND=${PROMPT_FERRY_RELAY__WORKER_BIND:-127.0.0.1:8788}"
  "PROMPT_FERRY_RELAY__CLIENT_TOKEN=${PROMPT_FERRY_RELAY__CLIENT_TOKEN:-dev-client-token}"
  "PROMPT_FERRY_RELAY__WORKER_TOKEN=${PROMPT_FERRY_RELAY__WORKER_TOKEN:-dev-worker-token}"
  "PROMPT_FERRY_RELAY__REQUEST_TIMEOUT_SECONDS=${PROMPT_FERRY_RELAY__REQUEST_TIMEOUT_SECONDS:-300}"
  "PROMPT_FERRY_WORKER__RELAY_URLS=${PROMPT_FERRY_WORKER__RELAY_URLS:-[\"ws://127.0.0.1:8788/ws/worker\"]}"
  "PROMPT_FERRY_WORKER__WORKER_TOKEN=${PROMPT_FERRY_WORKER__WORKER_TOKEN:-dev-worker-token}"
  "PROMPT_FERRY_WORKER__DATABASE_URL=${PROMPT_FERRY_WORKER__DATABASE_URL:-${PROMPT_FERRY_DEV_DATABASE_URL:-}}"
  "PROMPT_FERRY_WORKER__ADMIN_BIND=${PROMPT_FERRY_WORKER__ADMIN_BIND:-127.0.0.1:8789}"
  "PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_LOGIN=${PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_LOGIN:-admin}"
  "PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_PASSWORD=${PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_PASSWORD:-change-me-now}"
)

worker_relay_urls="${PROMPT_FERRY_WORKER__RELAY_URLS:-[\"ws://127.0.0.1:8788/ws/worker\"]}"
worker_relay_url="${worker_relay_urls#\[\"}"
worker_relay_url="${worker_relay_url%%\"*}"

relay_is_local() {
  case "$worker_relay_url" in
    ws://127.0.0.1:*|wss://127.0.0.1:*|ws://localhost:*|wss://localhost:*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

start_service() {
  local service="$1"
  local color="$2"
  local dir="$3"
  shift 3

  local fifo
  fifo="$(mktemp -u "${TMPDIR:-/tmp}/prompt-ferry-dev-${service}.XXXXXX")"
  mkfifo "$fifo"
  tmp_files+=("$fifo")

  prefix_output "$service" "$color" < "$fifo" &
  local log_pid=$!

  (
    cd "$dir" || exit 1
    exec env "${service_env[@]}" "$@"
  ) > "$fifo" 2>&1 &
  local pid=$!

  rm -f "$fifo"

  services+=("$service")
  pids+=("$pid")
  log_pids+=("$log_pid")
}

is_running() {
  local pid="$1"
  kill -0 "$pid" 2>/dev/null
}

any_running() {
  local pid
  for pid in "${pids[@]}"; do
    if is_running "$pid"; then
      return 0
    fi
  done
  return 1
}

all_core_stopped() {
  local i
  for i in 0 1; do
    if [ "${pids[$i]+set}" = "set" ] && is_running "${pids[$i]}"; then
      return 1
    fi
  done
  return 0
}

cleanup() {
  local exit_code=$?
  if [ "$stopping" -eq 1 ]; then
    exit "$exit_code"
  fi
  stopping=1
  trap '' INT TERM

  if any_running; then
    printf 'Stopping local dev stack...\n'
    local i
    for i in "${!pids[@]}"; do
      if is_running "${pids[$i]}"; then
        log_line "${services[$i]}" "$gray" "stopping..."
        kill "${pids[$i]}" 2>/dev/null || true
      fi
    done

    local waited=0
    while any_running && [ "$waited" -lt 50 ]; do
      sleep 0.1
      waited=$((waited + 1))
    done

    for i in "${!pids[@]}"; do
      if is_running "${pids[$i]}"; then
        log_line "${services[$i]}" "$red" "force killing..."
        kill -9 "${pids[$i]}" 2>/dev/null || true
      fi
    done

    printf 'Stopped local dev stack\n'
  fi

  local file
  for file in "${tmp_files[@]}"; do
    rm -f "$file"
  done
}

trap 'cleanup; exit 130' INT TERM
trap 'cleanup' EXIT

endpoint_ready() {
  curl -sS --max-time 0.5 -o /dev/null "$1" 2>/dev/null
}

http_stack_ready() {
  if relay_is_local; then
    endpoint_ready "http://127.0.0.1:8787/healthz" &&
      endpoint_ready "http://127.0.0.1:8788/healthz" &&
      endpoint_ready "http://127.0.0.1:8789/api/v1/auth/login"
  else
    endpoint_ready "http://127.0.0.1:8789/api/v1/auth/login"
  fi
}

wait_for_http_stack() {
  if relay_is_local; then
    printf 'Waiting for relay and worker admin to become reachable...\n'
  else
    printf 'Waiting for worker admin to become reachable...\n'
  fi
  local i
  for i in $(seq 1 120); do
    if all_core_stopped; then
      if relay_is_local; then
        printf 'Warning: relay or worker stopped before stack became healthy\n'
      else
        printf 'Warning: worker stopped before stack became healthy\n'
      fi
      return 1
    fi
    if http_stack_ready; then
      return 0
    fi
    sleep 0.25
  done
  printf 'Warning: timed out waiting for local services to become ready\n'
  return 1
}

wait_until_stopped() {
  while any_running; do
    sleep 0.25
  done
}

if relay_is_local; then
  start_service "relay" "$cyan" "." "target/debug/prompt-ferry" "relay"
fi
start_service "worker" "$green" "." "target/debug/prompt-ferry" "worker"

case "$mode" in
  full)
    if wait_for_http_stack; then
      start_service "frontend" "$magenta" "frontend" "bun" "run" "dev"
      wait_until_stopped
    fi
    ;;
  backend)
    wait_until_stopped
    ;;
  *)
    printf 'unknown supervisor mode: %s\n' "$mode" >&2
    exit 2
    ;;
esac
