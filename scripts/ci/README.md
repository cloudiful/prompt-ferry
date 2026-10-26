# CI coverage scripts

`#569 Phase 0` measures coverage and gates only the redaction chain. These
scripts are reproducible locally with Docker or any reachable Valkey.

## Scripts

| Script                       | Purpose                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| ---------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `test-env.sh`                | Sourced helper for the test-only dotenv settings: reads the project `.env` as data through `scripts/dev-env.sh` and exports only the quota PostgreSQL pair, `PROMPT_FERRY_TEST_VALKEY_URL`, the eight `PROMPT_FERRY_TEST_S3_*` settings, and an absolute `PROMPT_FERRY_TEST_TMPDIR`/`TMPDIR`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `with-test-env.sh`           | Runs any command with those settings loaded (`bash scripts/ci/with-test-env.sh <command> [args...]`). The command runs inside one fresh run directory owned by `test_run_tmp.py` (see below), with the original arguments, exit status, and signals.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `test_run_tmp.py`            | Owns one `run-<uuid>` directory under the exported test root per invocation, points `TMPDIR`/`PROMPT_FERRY_TEST_TMPDIR` at it, forwards signals to the owned process group, preserves the exit status, and removes exactly that directory afterwards. Reports (never deletes) leftovers from SIGKILLed runs. When the effective Cargo compiler wrapper (`RUSTC_WRAPPER`/`RUSTC_WORKSPACE_WRAPPER`, resolved from env — bare name beats `CARGO_BUILD_*` — then Cargo config files, nearest `.cargo` first with legacy `config` shadowing `config.toml`, global `CARGO_HOME` config last) names the sccache executable, the key is set empty in the owned child environment only: per the Cargo Book, empty overrides the config and disables the wrapper, while deleting the key would fall back to a file wrapper. Parent env, config files, the shared cache daemon, and ordinary wrappers are untouched, so test builds compile directly instead of binding a daemon to the disposable directory. Requires Python ≥ 3.11 (stdlib `tomllib`). Cargo `--config` CLI overrides and config `include` directives are not inspected. |
| `test_test_run_tmp.py`       | Unit checks for that lifecycle (`uv run --no-project python scripts/ci/test_test_run_tmp.py`): stdlib only, synthetic roots, no services.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `require-test-postgres.py`   | Verifies the dedicated quota test database: the URL must address the operator-confirmed `host:port/database`, and that database must be reachable, authenticated, migrated, and report its own matching identity (never migrates or provisions), plus `public.request_records` must cover UTC yesterday/today/tomorrow — missing coverage fails read-only by default, or is created bounded when the partition opt-in is explicitly `true`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `test_pg_target.py`          | Pure grammar for the dedicated target — parses and compares the URL against the confirmed `host:port/database` with no process, network, or SQL. Imported by `require-test-postgres.py`, which re-exports every name it defines.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `test_s3_target.py`          | Pure grammar for the dedicated S3 test target — all eight settings must be present and agree on one confirmed endpoint-root/bucket, with no process, network, or object-store I/O. Executed by `require-services.sh`; mirrored independently by the Rust S3 test guard.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `test_test_s3_target.py`     | Unit checks for that grammar (`uv run --no-project python scripts/ci/test_test_s3_target.py`): pure fixtures, no credentials, no I/O.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `require-services.sh`        | The no-silent-skip guarantee: fails when either dedicated test service is unset, malformed, unreachable, or unready (never prints credentials, only scheme/host/port/database).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `run-tests.sh`               | Loads the test settings through `with-test-env.sh`, then runs `cargo test --workspace -- --test-threads=1`, then `check-test-summary.py`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `test-env.test.sh`           | Sandbox checks for the entrypoint, the resolution rules, and the preflight (`test-env-test-lib.sh`, `test-env-test-assertions.sh`, `test-env-test-cases.sh`, `test-env-test-target-cases.sh`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `check-test-summary.py`      | Rejects a run with failures, `#[ignore]`d tests, or no results at all.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `coverage.sh`                | Runs `cargo llvm-cov --workspace --lcov`, writes the workspace + redaction baseline report, then enforces the diff gate.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `coverage_summary.py`        | Renders workspace/redaction line coverage, the redaction baseline, and the zero-hit "blind spot" list from an lcov tracefile.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `redaction_diff_coverage.py` | Enforces the minimum coverage of changed redaction-chain lines.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `redaction-scope.txt`        | The redaction-chain globs the gate applies to; `!`-prefixed lines exclude a matched path.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `bench_compare.py`           | Saves/compares the Criterion redaction benchmarks against `benches/redaction_bench_baseline.json`; gates only the `quick` subset.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |

## Test-only settings

`test-env.sh` loads the project `.env` as data — through the existing
`scripts/dev-env.sh` parser, never by executing the file — and exports exactly
twelve keys:

| Setting                                    | Purpose                                                                                                               |
| ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------- |
| `PROMPT_FERRY_QUOTA_TEST_DATABASE_URL`     | The dedicated PostgreSQL database the quota integration tests use.                                                    |
| `PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET` | The operator-confirmed `host:port/database` that URL is allowed to address.                                           |
| `PROMPT_FERRY_TEST_VALKEY_URL`             | The Valkey instance the response-affinity regression tests use.                                                       |
| `PROMPT_FERRY_TEST_S3_ENDPOINT`            | The S3-compatible service root the raw-payload integration tests use.                                                 |
| `PROMPT_FERRY_TEST_S3_BUCKET`              | The dedicated test bucket (always `prompt-ferry-test`).                                                               |
| `PROMPT_FERRY_TEST_S3_REGION`              | The region the S3 client signs for.                                                                                   |
| `PROMPT_FERRY_TEST_S3_ACCESS_KEY`          | The test-only S3 access key (presence asserted, never printed).                                                       |
| `PROMPT_FERRY_TEST_S3_SECRET_KEY`          | The test-only S3 secret key (presence asserted, never printed).                                                       |
| `PROMPT_FERRY_TEST_S3_ALLOW_HTTP`          | Strict `true`/`false`; plain HTTP is refused without an explicit `true`.                                              |
| `PROMPT_FERRY_TEST_S3_PATH_STYLE`          | Strict `true`/`false`; must always be set explicitly.                                                                 |
| `PROMPT_FERRY_TEST_S3_CONFIRMED_TARGET`    | The operator-confirmed endpoint-root/bucket the other seven must agree on.                                            |
| `PROMPT_FERRY_TEST_TMPDIR`                 | Absolute project-local directory for the SQLite fixtures, exported as `TMPDIR`; defaults to `<repo>/target/test-tmp`. |

An absent S3 setting stays unset rather than invented, so a focused run
without S3 exports nothing S3-shaped and the Rust guard reports an explicit
skip. A value already present in the process environment wins over the `.env` entry, and
an explicitly blank process value stays unconfigured instead of silently falling
back to the file. `PROMPT_FERRY_TEST_ENV_FILE` selects another dotenv file (absolute,
or relative to the repository root); when it is set, the file must exist, while an
absent default `<repo>/.env` is fine for an environment-only CI run. A relative
`PROMPT_FERRY_TEST_TMPDIR` resolves against the repository root, never the
invocation directory, and the ambient `TMPDIR` is ignored when choosing it. The
directory is created when missing and never purged: SQLite fixtures keep their own
UUID-suffixed files and WAL/SHM sidecars there, and a concurrent run may still own
them. Nothing else is exported, so `DATABASE_URL`, provider keys, and other
application settings never reach a test process.

## The dedicated test target

`PROMPT_FERRY_QUOTA_TEST_DATABASE_URL` and
`PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET` are two halves of one authority. The
confirmed target is the `host:port/database` an operator approved for this
project; the URL supplies the credentials for it. Both keys are required, and
the preflight refuses anything whose `host:port/database` does not match — so
repointing only the URL, at a production database or any other service, fails
before a connection is attempted instead of quietly becoming the test target.

`test_pg_target.py` owns that grammar as a pure, side-effect-free module, and
`tests/support/test_pg_target.rs` is the Rust side of it, so the shell preflight
and the Rust helper accept and refuse exactly the same targets:

- only the `postgres` and `postgresql` schemes;
- the host is percent-decoded and lowercased, an absent port reads as `5432`,
  and the database must be named explicitly;
- a query string, an `options` segment, or a fragment is refused, so connection
  options cannot repoint the search path away from the confirmed target;
- only `postgres`/`postgresql` credentials count, `.pgpass` is disabled
  (`PGPASSFILE=/dev/null`, `PgConnectOptions::new_without_pgpass`), and no test
  reads `DATABASE_URL`;
- a mismatch, a missing half, or a malformed value fails closed — never a
  silent skip.

After connecting, the preflight also compares `current_database()` with the
confirmed database name, so a redirect or a wrong default database is caught
rather than assumed away. Once connected, the Rust helper uses the password the
dedicated URL carried; it is passed to the server through the connection, never
through argv, a log line, or a parser message.

Confirming a different target is an operator decision: provision and migrate
that database yourself (the preflight never does), and keep it distinct from
every application or production database.

`request_records` is a daily `RANGE (created_at)` parent, so the preflight also
requires UTC yesterday/today/tomorrow children (`request_records_YYYYMMDD`
with exact day-edge bounds, attached to the parent) before cargo starts. A
missing day fails read-only with the safe date diagnostic — never DDL — unless
`PROMPT_FERRY_QUOTA_TEST_AUTO_CREATE_PARTITIONS` reads exactly `true`, in
which case one bounded transaction creates only the missing days under an
advisory lock, verifies exact bounds and attachment, and commits; anything else
(a missing, blank, or `false` flag, a foreign same-named relation, or
unexpected bounds) refuses without modification and never drops or recreates.
The flag is strict `true`/`false`, travels with the other dedicated test keys
(process environment wins, an explicit blank stays unconfigured), and the
confirmed target alone never authorizes the creation.

The S3 side follows the same two-halves shape with eight settings instead of
two: the endpoint, bucket, region, credentials, and flags supply the target,
and `PROMPT_FERRY_TEST_S3_CONFIRMED_TARGET` must read exactly the confirmed
endpoint-root/bucket. `test_s3_target.py` owns that grammar as a pure,
side-effect-free module — http/https endpoints only, no userinfo, query,
fragment, or non-root path, the bucket must be the dedicated
`prompt-ferry-test`, booleans are strict `true`/`false`, plain HTTP needs the
explicit opt-in, and the path-style flag must always be set. A missing half, a
partial set, or a mismatch fails closed before any object-store I/O, and a run
with no S3 settings at all is an explicit skip, never silent coverage. The
Rust S3 tests (`src/raw_payload_store/s3_tests.rs`) enforce the same rules
independently before building the production store, and each test writes only
under its own `run-<uuid>` prefix, registering every key before its first PUT
and deleting exactly those keys afterwards — so a green run leaves zero owned
objects. A SIGKILLed run cannot clean up; its prefix is reported for the
operator, never auto-deleted, and no run ever lists a whole bucket or touches
unknown keys.

## Local reproduction

The dedicated services are provisioned outside this repository; the preflight
verifies them and never creates, migrates, or starts anything.

```bash
bash scripts/ci/run-tests.sh                            # full run: needs all services
bash scripts/ci/with-test-env.sh cargo test -p prompt-ferry   # focused run
bash scripts/ci/test-env.test.sh                         # entrypoint checks, no service needed
uv run --no-project python scripts/ci/test_test_postgres_partitions.py  # partition-guard checks, no service needed
uv run --no-project python scripts/ci/test_test_s3_target.py  # S3 grammar checks, no service needed
bash scripts/ci/with-test-env.sh cargo test -p prompt-ferry raw_payload_store -- --test-threads=1  # raw store incl. real S3
```

`run-tests.sh` re-enters itself through `with-test-env.sh` exactly once
(`PROMPT_FERRY_TEST_ENV_LOADED` is the guard), runs both preflights before cargo
starts, and preserves `SQLX_OFFLINE=true`, the `target/ci/test.log` summary, and
cargo's exit status. A missing or unready service fails the run instead of
degrading into a silent skip. A focused SQLite-only run through the wrapper needs
no database service, but it cannot claim PostgreSQL coverage; likewise a focused
run without the eight S3 settings skips the real S3 tests explicitly and cannot
claim S3 coverage.

```bash
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

Three dedicated services back the suite: the Valkey instance the response-affinity
regression tests read, the dedicated PostgreSQL database the quota
integration tests read, and the dedicated S3 bucket the raw-payload
integration tests read. A group backed by one of them either returns early and
reports itself as skipped when its setting is absent, or fails outright — so
`require-services.sh` checks all three before cargo starts: it requires the
settings, then verifies that the database URL addresses the operator-confirmed
`host:port/database`, that it accepts an authenticated connection, that the
connected database reports a matching identity, and that its schema is
migrated, plus the existing credential-free Valkey reachability probe, plus the
eight-setting S3 target confirmation. Once all pass, no branch can trigger, and a green summary means the service-backed
tests really ran. `check-test-summary.py` is the complementary summary guard (no
failures, no ignored tests, results present). The suite is never run with
`--nocapture`, and no script logs a connection URL, its userinfo, a password,
or an S3 secret key.

That per-test behavior is the fallback outside this gate, because a bare
`cargo test` loads no `.env`: it sees only variables already exported in the
environment. Unset, the Valkey and quota snapshot/service tests return early and
report themselves as skipped, while `tests/support/sql_lookup_regression.rs`
fails outright. Exported, the tests connect to whatever those variables name. A
bare run therefore says nothing about service-backed coverage in either
direction, which is why every documented entrypoint loads the settings first.
Migrations are exercised by `cargo run --bin db_init` in the development
environment — the preflight reports an unmigrated test database, it never
migrates one.

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
executes `require-services.sh` first, so the service-backed tests cannot silently
skip, and sets `RUST_TEST_THREADS=1` to keep the global-redaction-lock suite
serialized; `cargo-mutants` itself stays sequential. The report lands in
`mutants.out/`. Target: survivors below 10%, with each surviving mutant either
killed by a new assertion or waived in writing. `cargo mutants --shard k/n`
splits one batch across bounded runs.

```bash
cargo install cargo-mutants --locked
bash scripts/ci/with-test-env.sh scripts/ci/require-services.sh
RUST_TEST_THREADS=1 bash scripts/ci/with-test-env.sh \
  cargo mutants --in-place --no-shuffle --workspace
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

| turn                           | 10k     | 100k     | 500k      |
| ------------------------------ | ------- | -------- | --------- |
| redact, all rules, 0 secrets   | 1.31 ms | 13.50 ms | 70.96 ms  |
| redact, all rules, 50 secrets  | 1.72 ms | 15.83 ms | 77.69 ms  |
| redact, all rules, 200 secrets | 2.68 ms | 32.89 ms | 162.38 ms |
| restore, all rules, 0 secrets  | 0.17 ms | 1.76 ms  | 8.75 ms   |

Per-round redaction grows roughly linearly with context (about 0.14 us/char)
and with the custom-string count; restore stays an order of magnitude cheaper
and is independent of the secret count. The committed absolute budget lives in
`src/redaction_budget_tests.rs` (always-on `--lib`): one 100 KB turn with all
rules and 50 secrets must keep its median under 1500 ms. Measured p50 at
100 KB/50 secrets: ~15.8 ms release, ~388 ms debug; the budget absorbs the
debug profile and shared-runner contention. Criterion medians drift with CPU
contention, so the factor is a coarse regression tripwire, not a tight budget.

## Frontend tests

The frontend has three test scopes.

| Scope                             | Runner               | Selection                                                                                                  | Size      |
| --------------------------------- | -------------------- | ---------------------------------------------------------------------------------------------------------- | --------- |
| React and form-mapper unit suites | Vitest               | `src/admin-mappers/forms/**/*.test.ts` and `src/react-tests/**/*.{test.ts,test.tsx}`                       | 830 cases |
| React suites only                 | Vitest               | `src/react-tests/**`                                                                                       | 789 cases |
| Retained native suites            | `bun:test`           | `tests/theme-first-paint.test.ts`, `tests/dev-proxy-scope.test.ts`                                         | 22 cases  |
| Generated SDK                     | `bun run openapi-ts` | `tools/codegen` runs `@hey-api/openapi-ts` 0.99.0 on TypeScript 6.0.3; the output must stay byte-identical | —         |

`frontend/vitest.config.ts` scopes the Vitest runner to those two source
globs, so `frontend/tests` is never picked up by Vitest. The two suites under
`frontend/tests` are the only native ones left and run through the explicit
`test:legacy` script, which names both paths; they are not migrated.

```bash
cd frontend
bun install --no-save --minimum-release-age 604800
bun run openapi-ts
bun run typecheck
bun run build
bun run test
bun run test:foundation
bun run test:legacy
bun run format:check
```

`bun run test` executes `vitest run` over both Vitest scopes, `bun run
test:foundation` narrows it to `src/react-tests`, and `bun run test:legacy` runs
the two native suites with `bun test --isolate`. `bun run build` is
`typecheck` plus `vite build`; `dist/index.html` is the artefact the Rust admin
server embeds. `frontend/package.json` pins the Bun version (`1.3.14`) that
`format:check`, `typecheck`, `test`, and `build` run with. The install uses
`--no-save`, so the gitignored `frontend/bun.lock` is never written or
committed, and `frontend/bunfig.toml` keeps the same `minimumReleaseAge = 604800`
age gate for every install.
