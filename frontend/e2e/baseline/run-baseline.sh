#!/usr/bin/env bash
# Run the issue #1060 full-app browser baseline and validate its evidence.
#
# From `frontend/`:
#   bash e2e/baseline/run-baseline.sh
#
# The script owns only the servers it starts. A listener already bound to
# 127.0.0.1:8789 or 127.0.0.1:5171 is reused and never killed.
set -euo pipefail

cd "$(dirname "$0")/../.."
FRONTEND_DIR="$(pwd)"
CACHE_DIR="$FRONTEND_DIR/../.cache/ai/react-migration"
SESSION="${PLAYWRIGHT_CLI_SESSION:-pf-baseline}"
FIXTURE_PID=""
VITE_PID=""

mkdir -p "$CACHE_DIR" "$FRONTEND_DIR/e2e/baseline/screenshots" "$FRONTEND_DIR/e2e/baseline/downloads"

cleanup() {
  if [[ -n "$VITE_PID" ]]; then
    kill "$VITE_PID" 2>/dev/null || true
  fi
  if [[ -n "$FIXTURE_PID" ]]; then
    kill "$FIXTURE_PID" 2>/dev/null || true
  fi
  bun x --package @playwright/cli playwright-cli -s="$SESSION" close >/dev/null 2>&1 || true
}
trap cleanup EXIT

port_open() {
  ss -ltn 2>/dev/null | grep -q "127.0.0.1:$1 "
}

if ! port_open 8789; then
  echo "starting fixture API on 127.0.0.1:8789"
  bun e2e/fixtures/server.ts > "$CACHE_DIR/fixture-server.log" 2>&1 &
  FIXTURE_PID=$!
fi

if ! port_open 5171; then
  echo "starting Vite dev server on 127.0.0.1:5171"
  bun run dev -- --host 127.0.0.1 > "$CACHE_DIR/vite-dev.log" 2>&1 &
  VITE_PID=$!
fi

for _ in $(seq 1 80); do
  if port_open 8789 && port_open 5171; then break; fi
  sleep 0.25
done
if ! port_open 8789 || ! port_open 5171; then
  echo "fixture API or Vite dev server did not become ready" >&2
  exit 1
fi

curl -fsS -X POST http://127.0.0.1:8789/__fixtures/reset >/dev/null

bun x --package @playwright/cli playwright-cli \
  -s="$SESSION" open --browser=chromium about:blank >/dev/null
bun x --package @playwright/cli playwright-cli \
  -s="$SESSION" --raw run-code --filename=e2e/baseline/capture.playwright \
  > "$CACHE_DIR/evidence-core-raw.json"
bun x --package @playwright/cli playwright-cli \
  -s="$SESSION" --raw run-code --filename=e2e/baseline/scenarios.playwright \
  > "$CACHE_DIR/evidence-scenarios-raw.json"
for callback in four-slices form-payloads advanced auth-quota-negative upstream-payloads quota-stale mcp-maintenance domain-retries relay-secrets mcp-secrets endpoint-secrets settings-secrets billing-filters attempt11-missing attempt11-settings attempt11-long-values; do
  bun x --package @playwright/cli playwright-cli \
    -s="$SESSION" --raw run-code --filename="e2e/baseline/$callback.playwright" \
    > "$CACHE_DIR/$callback-raw.json"
done

CACHE_DIR="$CACHE_DIR" \
  EVIDENCE_CORE="$CACHE_DIR/evidence-core-raw.json" \
  EVIDENCE_SCENARIOS="$CACHE_DIR/evidence-scenarios-raw.json" \
  EVIDENCE_OUT="$FRONTEND_DIR/e2e/baseline/evidence.json" \
  bun -e '
const parse = raw => (typeof raw === "string" ? JSON.parse(raw) : raw)
const core = parse(await Bun.file(process.env.EVIDENCE_CORE).json())
const scenarios = parse(await Bun.file(process.env.EVIDENCE_SCENARIOS).json())
const names = ["four-slices", "form-payloads", "advanced", "auth-quota-negative", "upstream-payloads", "quota-stale", "mcp-maintenance", "domain-retries", "relay-secrets", "mcp-secrets", "endpoint-secrets", "settings-secrets", "billing-filters", "attempt11-missing", "attempt11-settings", "attempt11-long-values"]
const residual = await Promise.all(names.map(async name => ({name, report: parse(await Bun.file(`${process.env.CACHE_DIR}/${name}-raw.json`).json())})))
const callbackReports = [{name: "capture", report: core}, {name: "scenarios", report: scenarios}, ...residual]
const runId = `p0-${core.startedAt}-${Date.now()}`
const runManifest = {
  runId,
  startedAt: core.startedAt,
  finishedAt: residual.at(-1)?.report.finishedAt ?? scenarios.finishedAt ?? core.finishedAt,
  callbacks: callbackReports.map(({name, report}) => ({name, runId, startedAt: report.startedAt, finishedAt: report.finishedAt})),
}
const evidence = {
  ...core,
  runId,
  runManifest,
  captures: [...(core.captures ?? []), ...(scenarios.captures ?? [])],
  mutations: [...(core.mutations ?? []), ...(scenarios.mutations ?? [])],
  checks: { ...(core.checks ?? {}), ...(scenarios.checks ?? {}) },
  requestLog: scenarios.requestLog ?? core.requestLog ?? [],
  residual: residual.map(({name, report}) => ({name, runId, report: {...report, runId}})),
  finishedAt: runManifest.finishedAt,
  knownBaselineDefects: [
    {rowId: "R-USAGE-AI", scenario: "sorting", disposition: "known-baseline-defect", evidence: "residual/four-slices/checks/sort", futureFixId: "REACT-AI-SORT-ACCESSIBLE", targetRequired: true},
    {rowId: "R-BILLING", scenario: "rule-pagination", disposition: "known-baseline-defect", evidence: "residual/four-slices/checks/billing", futureFixId: "REACT-BILLING-RULES-PAGINATION", targetRequired: true},
  ],
}
if (scenarios.failure) evidence.scenarioFailure = scenarios.failure
if (core.failure) evidence.failure = core.failure
const residualFailures = residual.flatMap(({name, report}) => report.failures?.length ? [`${name}: ${report.failures.map(failure => typeof failure === "string" ? failure : `${failure.id ?? "callback"}: ${failure.error ?? JSON.stringify(failure)}`).join("; ")}`] : [])
if (residualFailures.length) evidence.residualFailures = residualFailures
const {buildScenarioCoverage} = await import("./e2e/baseline/scenario-coverage.ts")
evidence.scenarioCoverage = buildScenarioCoverage(evidence)
await Bun.write(process.env.EVIDENCE_OUT, JSON.stringify(evidence, null, 2))
if (evidence.failure || evidence.scenarioFailure || residualFailures.length) {
  console.error(`baseline failed: ${evidence.failure ?? evidence.scenarioFailure ?? residualFailures.join("; ")}`)
  process.exit(1)
}
console.log(`captured ${evidence.captures.length} screenshots and ${evidence.mutations.length} scenario flows (${runId})`)
'

bun x prettier --write e2e/baseline/evidence.json >/dev/null
bun run e2e/baseline/validate-matrix.ts
