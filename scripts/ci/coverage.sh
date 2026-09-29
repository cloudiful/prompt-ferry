#!/usr/bin/env bash
# Generate the repo + redaction-chain coverage report and enforce the
# incremental gate on changed redaction-chain lines (#569 Phase 0).
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

scripts/ci/require-services.sh

OUT_DIR="target/coverage"
mkdir -p "$OUT_DIR"
LOG="$OUT_DIR/coverage-tests.log"
LCOV="$OUT_DIR/lcov.info"

# cargo-llvm-cov manages its own rustc wrapper; a preconfigured sccache wrapper
# only interferes with instrumentation.
unset RUSTC_WRAPPER

set +e
SQLX_OFFLINE=true cargo llvm-cov --workspace --lcov --output-path "$LCOV" \
  -- --test-threads=1 2>&1 | tee "$LOG"
status="${PIPESTATUS[0]}"
set -e

python3 scripts/ci/check-test-summary.py --log "$LOG" --label "coverage tests" || exit 1

if [ "$status" -ne 0 ]; then
  echo "error: coverage test run failed (exit ${status})" >&2
  exit "$status"
fi

python3 scripts/ci/coverage_summary.py \
  --lcov "$LCOV" \
  --root "$ROOT" \
  --scope-file scripts/ci/redaction-scope.txt \
  --out-dir "$OUT_DIR"

BASE="${DIFF_BASE:-}"
if [ -z "$BASE" ] || ! git rev-parse --verify --quiet "${BASE}^{commit}" >/dev/null; then
  BASE=""
  for candidate in origin/main main; do
    if git rev-parse --verify --quiet "${candidate}^{commit}" >/dev/null; then
      BASE="$candidate"
      break
    fi
  done
fi

if [ -n "$BASE" ]; then
  merge_base="$(git merge-base "$BASE" HEAD 2>/dev/null || true)"
  if [ -n "$merge_base" ]; then
    BASE="$merge_base"
  fi
fi

if [ -z "$BASE" ]; then
  if [ "${REQUIRE_DIFF_BASE:-false}" = "true" ]; then
    echo "error: no diff base resolved for the redaction diff coverage gate" >&2
    exit 1
  fi
  echo "warning: no diff base resolved; redaction diff coverage gate skipped"
else
  python3 scripts/ci/redaction_diff_coverage.py \
    --lcov "$LCOV" \
    --root "$ROOT" \
    --scope-file scripts/ci/redaction-scope.txt \
    --base "$BASE" \
    --head HEAD \
    --min "${REDACTION_DIFF_COVERAGE_MIN:-90}" \
    --out "$OUT_DIR/redaction-diff-coverage.md"
fi
