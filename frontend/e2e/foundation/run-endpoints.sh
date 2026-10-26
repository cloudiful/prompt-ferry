#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../.."
BASE="http://127.0.0.1:5171"
FIXTURE="http://127.0.0.1:8789"
SESSION="${PLAYWRIGHT_CLI_SESSION:-pf-endpoints}"
MERGED_REPORT="e2e/foundation/endpoints-result.json"
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

# A run that stops early must leave no merged report: an older or partial report
# at that path would still read as a current run.
rm -f "$MERGED_REPORT"

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

CALLBACKS=(
  endpoints
  endpoints-create
  endpoints-update
  endpoints-mutations
  endpoints-delete
  endpoints-quota
  endpoints-oauth-status
  endpoints-oauth-device
  endpoints-oauth-browser
  endpoints-finish
)

# The screenshots each callback owes. A callback that does not produce its own
# evidence fails here, where it is produced, instead of at the merged report.
callback_screenshots() {
  case "$1" in
    endpoints)
      printf '%s\n' 'e2e/foundation/screenshots/endpoints-en-light-1440.png'
      ;;
    endpoints-create)
      printf '%s\n' 'e2e/foundation/screenshots/endpoints-created-en-light-1440.png'
      ;;
    endpoints-quota)
      printf '%s\n' \
        'e2e/foundation/screenshots/endpoints-quota-current-en-light-1440.png' \
        'e2e/foundation/screenshots/endpoints-quota-chart-en-light-1440.png' \
        'e2e/foundation/screenshots/endpoints-quota-history-en-light-1440.png' \
        'e2e/foundation/screenshots/endpoints-subscription-history-en-light-1440.png'
      ;;
    endpoints-finish)
      printf '%s\n' \
        'e2e/foundation/screenshots/endpoints-en-light-1024.png' \
        'e2e/foundation/screenshots/endpoints-en-light-768.png' \
        'e2e/foundation/screenshots/endpoints-en-light-390.png' \
        'e2e/foundation/screenshots/endpoints-zh-dark-1440.png' \
        'e2e/foundation/screenshots/endpoints-en-light-1440-final.png' \
        'e2e/foundation/screenshots/endpoints-quota-fresh-en-light-1440.png'
      ;;
    *) : ;;
  esac
}

# Every callback report keeps its own path, so no callback can overwrite the
# merged report and the merged report can never be a raw callback report.
callback_report() {
  printf 'e2e/foundation/%s-callback.json\n' "$1"
}

run_callback() {
  local name="$1"
  local path="e2e/foundation/${name}.playwright"
  local result
  result="$(callback_report "$name")"
  bun x --package @playwright/cli@0.1.22 playwright-cli \
    -s="$SESSION" --raw run-code --filename="$path" > "$result"
  if [[ ! -s "$result" ]]; then
    echo "callback $name produced no report: $path" >&2
    exit 1
  fi
  bun -e '
    const path = process.argv[1]
    let raw
    try {
      raw = await Bun.file(path).json()
    } catch (cause) {
      console.error(`callback report is not valid JSON: ${path}`)
      console.error(String(cause))
      process.exit(1)
    }
    if (raw === null || typeof raw !== "object") {
      console.error(`callback report is not a report object: ${path}`)
      process.exit(1)
    }
    const report = typeof raw === "string" ? JSON.parse(raw) : raw
    await Bun.write(path, `${JSON.stringify(report, null, 2)}\n`)
  ' "$result"
  bun x prettier --write "$result" >/dev/null
}

verify_callback_evidence() {
  local name="$1"
  local result
  result="$(callback_report "$name")"
  local expected=()
  while IFS= read -r line; do
    expected+=("$line")
  done < <(callback_screenshots "$name")
  if [[ "${#expected[@]}" -eq 0 ]]; then
    return 0
  fi
  bun -e '
    const [, name, path, ...expected] = process.argv
    let report
    try {
      report = await Bun.file(path).json()
    } catch {
      console.error(`${name}: callback report is not valid JSON: ${path}`)
      process.exit(1)
    }
    const started = Date.parse(report?.startedAt ?? "")
    const finished = Date.parse(report?.finishedAt ?? "")
    if (!Number.isFinite(started) || !Number.isFinite(finished) || finished < started) {
      console.error(`${name}: callback report has no usable start/finish window`)
      process.exit(1)
    }
    const listed = Array.isArray(report.screenshots) ? report.screenshots : []
    const tolerance = 5000
    const problems = []
    for (const screenshot of expected) {
      if (!listed.includes(screenshot)) {
        problems.push(`${name}: callback report does not list ${screenshot}`)
        continue
      }
      const file = Bun.file(screenshot)
      if (!(await file.exists())) {
        problems.push(`${name}: screenshot file is missing on disk: ${screenshot}`)
        continue
      }
      const mtime = Number((await file.stat()).mtimeMs)
      if (!Number.isFinite(mtime) || mtime < started - tolerance || mtime > finished + tolerance) {
        problems.push(`${name}: screenshot is stale, outside this callback window: ${screenshot}`)
      }
    }
    if (problems.length > 0) {
      for (const problem of problems) console.error(problem)
      process.exit(1)
    }
    console.log(`${name}: ${expected.length} callback screenshots verified in this run window`)
  ' "$name" "$result" "${expected[@]}"
}

curl -fsS -X POST "$FIXTURE/__fixtures/reset" >/dev/null
bun x --package @playwright/cli@0.1.22 playwright-cli \
  -s="$SESSION" open --browser=chromium about:blank >/dev/null

for callback in "${CALLBACKS[@]}"; do
  run_callback "$callback"
  verify_callback_evidence "$callback"
done

# Merge only after every callback and every per-callback screenshot check passed.
bun -e '
  const names = process.argv.slice(1)
  const reports = []
  for (const name of names) {
    const path = `e2e/foundation/${name}-callback.json`
    let report
    try {
      report = await Bun.file(path).json()
    } catch {
      console.error(`callback report is not valid JSON: ${path}`)
      process.exit(1)
    }
    reports.push(report)
  }
  const checks = Object.assign({}, ...reports.map(report => report.checks ?? {}))
  checks.scenarioCoverage = {
    "route-render": checks.routeAndNavigation?.routeId === "R-ENDPOINTS-UPSTREAMS",
    "admin-denial": checks.adminDenial?.adminApiCalls === 0 && checks.adminDenial?.redirectedToApiKeys === true,
    pagination: checks.pagination?.secondPageRows === 3 && checks.pageSizePersistence === true,
    "dialog-open": checks.createDialogAndRetry?.createdId != null,
    "mutation-create": checks.createDialogAndRetry?.errorVisible === true,
    "mutation-toggle": checks.toggleAndIndependentProbe?.restored === true,
    "mutation-update": checks.updateFailureDraftRetry?.retryStatus === 200,
    "mutation-delete": checks.deleteCancelAndSuccess?.successfulDelete === true,
    "mutation-conflict-guard": checks.updateFailureDraftRetry?.rowToggleDisabled === true && checks.updateFailureDraftRetry?.cancelDisabled === true,
    "mutation-failure-draft": checks.createDialogAndRetry?.draftRetained === true && checks.updateFailureDraftRetry?.draftRetained === true,
    "long-values": checks.createPayloads?.[0]?.name.length > 40 && checks.responsiveContainment?.length === 4,
    "loading-state": checks.loadingState === true,
    "error-state": checks.errorState?.visible === true && checks.errorState?.retried === true,
  }
  const merged = {
    status: "complete",
    runId: reports[0].runId,
    startedAt: reports[0].startedAt,
    finishedAt: reports.at(-1).finishedAt,
    callbacks: names,
    checks,
    screenshots: [...new Set(reports.flatMap(report => report.screenshots ?? []))],
    segments: names.map((name, index) => ({ name, startedAt: reports[index].startedAt, finishedAt: reports[index].finishedAt })),
  }
  const target = "e2e/foundation/endpoints-result.json"
  const temporary = `${target}.tmp`
  await Bun.write(temporary, `${JSON.stringify(merged, null, 2)}\n`)
  await Bun.$`mv ${temporary} ${target}`
' "${CALLBACKS[@]}"
bun x prettier --write "$MERGED_REPORT"
cat "$MERGED_REPORT"
