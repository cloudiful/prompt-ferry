# CI test and coverage scripts

The retained backend suite is unit-only: every test is pure or in-memory, so
these entrypoints read no `.env`, export no test settings, and connect to no
database, Valkey, S3, socket, or subprocess. `cargo fmt`, `cargo check`, and the
unit suite are the whole backend gate.

## Entrypoints

| Script                       | Purpose                                                                                                     |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `run-tests.sh`               | Runs `cargo test --workspace --lib --no-fail-fast`, checks the summary, and prints the elapsed wall time.    |
| `check-test-summary.py`      | Rejects a run with failures, `#[ignore]`d tests, or no results at all.                                       |
| `coverage.sh`                | Runs `cargo llvm-cov --workspace --lib --lcov`, writes the workspace + redaction report, then enforces the diff gate. |
| `coverage_summary.py`        | Renders workspace/redaction line coverage, the redaction baseline, and the zero-hit "blind spot" list from an lcov tracefile. |
| `redaction_diff_coverage.py` | Enforces the minimum coverage of changed redaction-chain lines.                                              |
| `redaction-scope.txt`        | The redaction-chain globs the gate applies to; `!`-prefixed lines exclude a matched path.                    |

```bash
bash scripts/ci/run-tests.sh
cargo fmt --all -- --check
SQLX_OFFLINE=true cargo check --workspace
```

`run-tests.sh` needs no provisioning, no `.env`, and no external service. It
writes the raw log to `target/ci/test.log`, and the summary check fails when the
run reports failures, ignored tests, or no test result. The elapsed time is a
coarse tripwire, not a budget.

## Coverage and the redaction gate

```bash
cargo install cargo-llvm-cov --locked
rustup component add llvm-tools-preview
scripts/ci/coverage.sh
```

`coverage.sh` reads `DIFF_BASE` (or falls back to `origin/main`) and fails when
changed redaction-chain lines fall below `REDACTION_DIFF_COVERAGE_MIN` (default
`90`). Reports land in `target/coverage/`.

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
property test such as `src/worker/runtime/ai/stream_restore/property_tests.rs`
or `crates/**/src/tests/**` does not trip the fail-closed production check.
Production implementation paths stay in scope and keep the fail-closed
behavior.

## Mutation testing (#569 Phase 2)

`cargo-mutants` measures the assertion strength of the redaction chain. The
scope lives in `.cargo/mutants.toml`: six production files, the smallest
surface that still covers the #568 restore leak site and its helpers; the two
largest redaction production files stay out of scope so one batch stays bounded.
`--workspace` is required because this is a root-package workspace: without it
cargo-mutants mutates only the root `prompt-ferry` package and silently drops
`crates/prompt-ferry-redact-upstream/src/lib.rs`. Confirm the resolved scope
with `cargo mutants --list-files --workspace`, which must list those six files.

Mutation testing is manual-only because a batch is long-running. The run sets
`RUST_TEST_THREADS=1` to keep the global-redaction-lock suite serialized;
`cargo-mutants` itself stays sequential. The report lands in `mutants.out/`.
Target: survivors below 10%, with each surviving mutant either killed by a new
assertion or waived in writing. `cargo mutants --shard k/n` splits one batch
across bounded runs.

```bash
cargo install cargo-mutants --locked
RUST_TEST_THREADS=1 cargo mutants --in-place --no-shuffle --workspace
```

The survivor report has not been produced yet; the first manual run is pending
operator execution.

## Performance protection

Criterion benchmarks are gone with the unit-only policy. The always-on
`--lib` budget in `src/redaction_budget_tests.rs` remains: one 100 KB turn with
all rules and 50 secrets must keep its median under 1500 ms.
