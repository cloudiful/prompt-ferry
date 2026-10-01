# CI coverage scripts

`#569 Phase 0` measures coverage and gates only the redaction chain. These
scripts are reproducible locally with Docker or any reachable Valkey.

## Scripts

| Script | Purpose |
| --- | --- |
| `require-services.sh` | The no-silent-skip guarantee: fails when `PROMPT_FERRY_TEST_VALKEY_URL` is unset or unreachable (never prints credentials, only scheme/host/port). |
| `run-tests.sh` | Runs `cargo test --workspace -- --test-threads=1`, then `check-test-summary.py`. |
| `check-test-summary.py` | Rejects a run with failures, `#[ignore]`d tests, or no results at all. |
| `coverage.sh` | Runs `cargo llvm-cov --workspace --lcov`, writes the workspace + redaction baseline report, then enforces the diff gate. |
| `coverage_summary.py` | Renders workspace/redaction line coverage, the redaction baseline, and the zero-hit "blind spot" list from an lcov tracefile. |
| `redaction_diff_coverage.py` | Enforces the minimum coverage of changed redaction-chain lines. |
| `redaction-scope.txt` | The redaction-chain globs the gate applies to; `!`-prefixed lines exclude a matched path. |
| `bench_compare.py` | Saves/compares the Criterion redaction benchmarks against `benches/redaction_bench_baseline.json`; gates only the `quick` subset. |

## Local reproduction

```bash
export PROMPT_FERRY_TEST_VALKEY_URL=redis://<host>:6379
scripts/ci/run-tests.sh

cargo install cargo-llvm-cov --locked
rustup component add llvm-tools-preview
scripts/ci/coverage.sh
```

`coverage.sh` reads `DIFF_BASE` (or falls back to `origin/main`) and fails when
changed redaction-chain lines fall below `REDACTION_DIFF_COVERAGE_MIN` (default
`90`). Reports land in `target/coverage/`.

## Gate semantics

Only lines a change adds to the redaction-chain scope count; whole-repo
coverage stays informational. For a file present in the coverage report the
denominator is its changed executable lines, so comments and blank lines never
count against a change. A changed in-scope **production** file missing from the
report was never instrumented, so its changed lines count as uncovered and the
gate fails instead of passing vacuously. A change with no production
redaction-chain line passes.

### Test-only exclusions

cargo-llvm-cov's default `--ignore-filename-regex` intentionally omits test
targets (any `tests`/`examples`/`benches` directory and any `tests.rs` /
`*_tests.rs` / `*-tests.rs` file), so those files never appear in LCOV. The
scope file excludes them with `!` globs (`!**/tests/**`, `!**/tests.rs`,
`!**/*_tests.rs`, `!**/*-tests.rs`, `!**/examples/**`, `!**/benches/**`) so a
Phase 1 property test such as
`src/worker/runtime/ai/stream_restore/property_tests.rs` or
`crates/**/src/tests/**` does not trip the fail-closed production check.
Production implementation paths stay in scope and keep the fail-closed
behavior. The informational workspace/redaction summary is unaffected: omitted
test files never appear in LCOV in the first place.

## No-silent-skip guarantee

The only service-backed test reads `PROMPT_FERRY_TEST_VALKEY_URL` (Valkey), so
`require-services.sh` requiring and reachability-probing it is the primary
guarantee: once it passes, the skip branch cannot trigger. PostgreSQL is not a
test service: the suite reads no `.env` and needs no `DATABASE_URL`, and every
database-shaped test helper is a lazy pool pinned to an isolated `pfy_test_*`
schema that never connects. Migrations are exercised by `cargo run --bin
db_init` in the development environment. `check-test-summary.py` is the
complementary summary guard (no failures, no ignored tests, results present).
The suite is never run with `--nocapture`, and no script logs a connection URL.

## Mutation testing (#569 Phase 2)

`cargo-mutants` measures the assertion strength of the redaction chain. The
scope lives in `.cargo/mutants.toml`: six production files, the smallest
surface that still covers the #568 restore leak site and its helpers; the two
largest redaction production files stay out of scope so one batch stays bounded.
`--workspace` is required because this is a root-package workspace: without it
cargo-mutants mutates only the root `prompt-ferry` package and silently drops
`crates/prompt-ferry-redact-upstream/src/lib.rs`. Confirm the resolved scope
with `cargo mutants --list-files --workspace`, which must list those six files.

Mutation testing is manual-only because a batch is long-running. The run
executes `require-services.sh` first, so the Valkey-backed tests cannot silently
skip, and sets `RUST_TEST_THREADS=1` to keep the global-redaction-lock suite
serialized; `cargo-mutants` itself stays sequential. The report lands in
`mutants.out/`. Target: survivors below 10%, with each surviving mutant either
killed by a new assertion or waived in writing. `cargo mutants --shard k/n`
splits one batch across bounded runs.

```bash
cargo install cargo-mutants --locked
export PROMPT_FERRY_TEST_VALKEY_URL=redis://<host>:6379
scripts/ci/require-services.sh
RUST_TEST_THREADS=1 cargo mutants --in-place --no-shuffle --workspace
```

The survivor report has not been produced yet; the first manual run is
pending operator execution.

## Performance benchmarks (#569 Phase 3)

`benches/redaction_pipeline.rs` and `benches/redaction_restore.rs` are Criterion
benches over the matrix `context {10k, 100k, 500k} x rules {domain, all9} x
secrets {0, 50, 200}`. The redact side replays one fully re-sent turn against a
prior session (the `#564` per-round full rescan); the restore side replays one
restore pass over the redacted text of the same context. Scenario ids carry a
`quick`/`full` tier; only the bounded `quick` subset (all rules, at most 50
secrets, contexts at or below 100k) is gated.

```bash
cargo bench --bench redaction_pipeline --bench redaction_restore            # full matrix (local)
cargo bench --bench redaction_pipeline --bench redaction_restore -- quick    # gated subset
scripts/ci/bench_compare.py --mode save    # rewrite benches/redaction_bench_baseline.json
scripts/ci/bench_compare.py --mode check   # compare a run against the baseline
```

`benches/redaction_bench_baseline.json` is the committed measured baseline
(medians in ns, `gate` marks the quick subset). `bench_compare.py --mode check`
fails when a gated median exceeds its baseline by `max_regression_factor`
(default `2.5`, override with `--max-factor`) or when a gated benchmark
produced no measurement; ungated full-matrix entries are reported but never
fail. Only the bounded `quick` subset is gated; mutation testing and full-load
benchmarks stay manual.

Release medians from the committed baseline (reference host, `--test-threads`
irrelevant here):

| turn | 10k | 100k | 500k |
| --- | --- | --- | --- |
| redact, all rules, 0 secrets | 1.31 ms | 13.50 ms | 70.96 ms |
| redact, all rules, 50 secrets | 1.72 ms | 15.83 ms | 77.69 ms |
| redact, all rules, 200 secrets | 2.68 ms | 32.89 ms | 162.38 ms |
| restore, all rules, 0 secrets | 0.17 ms | 1.76 ms | 8.75 ms |

Per-round redaction grows roughly linearly with context (about 0.14 us/char)
and with the custom-string count; restore stays an order of magnitude cheaper
and is independent of the secret count. The committed absolute budget lives in
`src/redaction_budget_tests.rs` (always-on `--lib`): one 100 KB turn with all
rules and 50 secrets must keep its median under 1500 ms. Measured p50 at
100 KB/50 secrets: ~15.8 ms release, ~388 ms debug; the budget absorbs the
debug profile and shared-runner contention. Criterion medians drift with CPU
contention, so the factor is a coarse regression tripwire, not a tight budget.

## Frontend mapper tests (#569 Phase 4)

Vitest covers the admin mapper form helpers, starting with the endpoint and
model-route `*FormToRequest` schedules (including the #514 `days` round-trip).
`frontend/vitest.config.ts` scopes the runner to
`src/admin-mappers/forms/**/*.test.ts`, so the existing `frontend/tests`
suites keep running on `bun:test` and are never picked up or migrated.

```bash
cd frontend
bun install --no-save
bun run test
```

`bun run test` executes `vitest run`. `frontend/package.json` pins the Bun
version (`1.3.14`) that `format:check`, `typecheck`, `test`, and `build` run
with. The install uses `--no-save`, so the gitignored `frontend/bun.lock` is
never written or committed.
