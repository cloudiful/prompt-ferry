#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../.."
BASE="http://127.0.0.1:5171"
FIXTURE="http://127.0.0.1:8789"
SESSION="${PLAYWRIGHT_CLI_SESSION:-pf-redaction-rules}"
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

if port_open 8789; then
  if ! curl -fsS "$FIXTURE/__fixtures/summary" | jq -e '.requests | type == "number"' >/dev/null; then
    echo "127.0.0.1:8789 is occupied by a non-fixture service; refusing to use or stop it" >&2
    exit 1
  fi
else
  bun e2e/fixtures/server.ts >/dev/null 2>&1 &
  FIXTURE_PID=$!
fi

if port_open 5171; then
  if ! curl -fsS "$BASE/src/main.tsx" | rg -q 'createRoot'; then
    echo "127.0.0.1:5171 is occupied by a non-React frontend; refusing to use or stop it" >&2
    exit 1
  fi
else
  bun run dev -- --host 127.0.0.1 >/dev/null 2>&1 &
  VITE_PID=$!
fi

ready=false
for _ in $(seq 1 80); do
  if port_open 8789 && port_open 5171; then
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
  -s="$SESSION" --raw run-code --filename=e2e/foundation/redaction-rules.playwright \
  > e2e/foundation/redaction-rules-result.json
if [[ ! -s e2e/foundation/redaction-rules-result.json ]]; then
  echo "Playwright callback produced no current-run report" >&2
  exit 1
fi
bun -e '
  const path = "e2e/foundation/redaction-rules-result.json"
  const raw = await Bun.file(path).json()
  const report = typeof raw === "string" ? JSON.parse(raw) : raw
  await Bun.write(path, `${JSON.stringify(report, null, 2)}\n`)
'
bun x prettier --write e2e/foundation/redaction-rules-result.json >/dev/null
cat e2e/foundation/redaction-rules-result.json
