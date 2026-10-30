#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../.."
BASE="http://127.0.0.1:5188"
FIXTURE="http://127.0.0.1:8798"
SESSION="pf-1148-billing-$$"
FIXTURE_PID=""
VITE_PID=""

cleanup() {
  if [[ -n "$VITE_PID" ]]; then
    kill "$VITE_PID" 2>/dev/null || true
    wait "$VITE_PID" 2>/dev/null || true
  fi
  if [[ -n "$FIXTURE_PID" ]]; then
    kill "$FIXTURE_PID" 2>/dev/null || true
    wait "$FIXTURE_PID" 2>/dev/null || true
  fi
  bun x --package @playwright/cli@0.1.22 playwright-cli -s="$SESSION" close >/dev/null 2>&1 || true
}
trap cleanup EXIT

port_open() {
  ss -ltn 2>/dev/null | rg -q "127.0.0.1:$1 "
}

if port_open 8798 || port_open 5188; then
  echo "Isolated fixture or Vite port is occupied; refusing reuse" >&2
  exit 1
fi
FIXTURE_PORT=8798 bun e2e/fixtures/server.ts >/dev/null 2>&1 &
FIXTURE_PID=$!
bun run dev -- --host 127.0.0.1 --port 5188 --strictPort >/dev/null 2>&1 &
VITE_PID=$!

ready=false
for _ in $(seq 1 80); do
  if kill -0 "$FIXTURE_PID" "$VITE_PID" 2>/dev/null && port_open 8798 && port_open 5188; then
    ready=true
    break
  fi
  sleep 0.25
done
if [[ "$ready" != true ]]; then
  echo "fixture API or Vite did not become ready" >&2
  exit 1
fi

curl -fsS -X POST "$FIXTURE/__fixtures/reset" >/dev/null
bun x --package @playwright/cli@0.1.22 playwright-cli \
  -s="$SESSION" open --browser=chromium about:blank >/dev/null
bun x --package @playwright/cli@0.1.22 playwright-cli \
  -s="$SESSION" --raw run-code --filename=e2e/foundation/billing.playwright \
  > e2e/foundation/billing-result.json
if [[ ! -s e2e/foundation/billing-result.json ]]; then
  echo "Playwright callback produced no current-run report" >&2
  exit 1
fi
bun -e '
  const path = "e2e/foundation/billing-result.json"
  const raw = await Bun.file(path).json()
  const report = typeof raw === "string" ? JSON.parse(raw) : raw
  await Bun.write(path, `${JSON.stringify(report, null, 2)}\n`)
'
bun x prettier --write e2e/foundation/billing-result.json >/dev/null
cat e2e/foundation/billing-result.json
