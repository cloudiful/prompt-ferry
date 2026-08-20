# Cache Token Normalization

Status: IN_PROGRESS

## Tracking

- Mode: LOCAL_PLAN
- Artifact: `.opencode/plans/cache-token-normalization.md`
- Forgejo fallback: `fj whoami --remote origin` failed because the configured Forgejo URL returned an invalid content type. Continue locally; do not block implementation.
- 2026-08-20 recheck: `fj whoami --remote origin` now succeeds, but tracking remains LOCAL_PLAN because the mode is fixed for this task.

## Goal

Correct provider-specific cache token normalization and cache-rate reporting for new requests, then provide a controlled backfill for historical request records and billing snapshots where raw upstream usage is retained.

## Background

OpenAI usage normally reports total input tokens in `prompt_tokens`/`input_tokens`, with cached input as a detail. Anthropic Messages reports ordinary `input_tokens` separately from `cache_read_input_tokens` and `cache_creation_input_tokens`. The current production records for MiniMax's Anthropic-compatible `/v1/messages` path contain values such as `input_tokens=176` and `cached_tokens=82793`; the list SQL divides cached tokens by ordinary input and renders `47041%`. Raw SSE confirms these values are returned upstream.

The working tree at task start is clean at `HEAD=b28159b` (`phase(anthropic-stream-retry): make Anthropic stream failures retryable`). Baseline staged, unstaged, and untracked paths: none. Baseline paths are user-owned and out of scope for all phases.

## Constraints

- Preserve the provider-specific distinction between ordinary input, cache read, and cache write.
- Do not mutate production data directly from this coding task. Implement and validate a reviewable backfill path; execution against production requires an explicit deployment/operation step.
- Static SQL must be in `.sql` files and loaded through existing SQLx file macros.
- Do not modify baseline paths (there are none currently); executors must not stage or commit.
- Never stage or commit `Cargo.lock` or `bun.lock`.
- Keep migrations/backfill idempotent and bounded; do not overwrite records when raw usage is unavailable or ambiguous.

## Acceptance Criteria

- New OpenAI Chat/Responses records retain their provider-reported total input and cached detail without double counting.
- New Anthropic-compatible records normalize stored input to ordinary + cache read + cache write, with total tokens consistent with normalized input + output when provider total is not authoritative for the normalized representation.
- List, detail, summary, buckets, overview, and breakdown cache-rate calculations use one documented bounded denominator and never emit values above 100% for valid normalized data.
- Historical records with retained raw upstream usage can be identified and repaired idempotently, including their billing snapshots/lines; records without raw usage are left unchanged and reported.
- Tests cover OpenAI fields, Anthropic fields, malformed/inconsistent usage, cache-write handling, cache-rate bounds, and backfill dry-run/apply behavior.
- Required Rust/frontend checks pass for changed surfaces.

## Ordered Phases

### Phase 1: Usage model and new-request normalization

Outcome: provider-aware usage extraction and persistence semantics are explicit and tested.

In scope: `src/usage/**`, related Rust usage tests, and narrowly related billing normalization code if required.

Out of scope: production data mutation, broad admin UI redesign, unrelated existing changes.

Acceptance: OpenAI remains unchanged; Anthropic-compatible input is normalized exactly once; raw provider values remain available for cache meters; tests pass.

Validation: `cargo test --lib usage`, targeted compatibility tests, `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings` where practical.

### Phase 2: Unified cache-rate reporting

Outcome: all user-facing usage query surfaces calculate the same bounded cache-read rate from normalized fields and expose cache-write separately where already supported.

In scope: usage SQL files, Rust query/presentation types, focused frontend formatting/types/tests if needed, and OpenAPI only if the contract changes.

Out of scope: historical row mutation and unrelated admin surfaces.

Acceptance: list/detail/summary/buckets/overview/breakdowns agree; ordinary input and cache-read/cache-write totals are internally consistent; invalid denominators yield null rather than >100%.

Validation: SQLx compile checks, Rust usage tests, frontend typecheck/tests if configured, `git diff --check`.

### Phase 3: Historical usage backfill

Outcome: an idempotent, dry-run-first backfill tool or migration repairs eligible historical request records from retained raw SSE/JSON usage and refreshes billing snapshots/lines.

In scope: dedicated backfill tool/module, static SQL, focused tests, operator documentation only if required to run safely.

Out of scope: applying changes to production from this session, guessing values where raw usage is absent, repairing unrelated assistant artifacts.

Acceptance: dry-run reports candidates/skips; apply is transactional per bounded batch and rerunnable; raw provider usage is parsed with the same normalization logic as new requests; billing data is refreshed consistently; no eligible record is silently skipped.

Validation: unit/integration tests with fixtures, `cargo test`, and a read-only production candidate query through db-mcp if available.

### Phase 4: Aggregate review and checkpoint

Outcome: all phases independently reviewed, confirmed P0-P2 findings repaired, and the implementation committed as a controlled local checkpoint.

In scope: final diff and validation only.

Out of scope: deployment, production backfill execution, force operations, or unrelated worktree changes.

Acceptance: reviewer passes; changed paths are allowlisted; staged patch check passes; exact phase checkpoint commit succeeds; plan is archived as COMPLETE.

## Dependencies

- Phase 2 depends on the canonical usage semantics from Phase 1.
- Phase 3 depends on the same parser/normalizer being callable without duplicating provider rules.
- Phase 4 depends on all prior validation and review results.

## Decisions

- Use LOCAL_PLAN because Forgejo authentication/connectivity failed.
- Treat cache-read rate as `cache_read / (ordinary_input + cache_read + cache_write)` for normalized usage; do not use ordinary input alone as denominator.
- Do not infer historical normalized input without retained raw usage.

## Phase Results

### Phase 1: Usage model and new-request normalization

- Result: DONE, review passed.
- Executor changed: `src/usage/text.rs`, `src/usage/capture.rs`.
- Behavior: OpenAI-shaped usage is not folded; native Anthropic cache read/write is folded into canonical input exactly once; token values are clamped non-negative; focused tests cover provider shapes and SSE merging.
- Validation: `cargo test --lib usage` passed (59); `cargo test --lib anthropic` passed (26); `cargo test --lib openai_compat` passed (94); `cargo test --lib db::types::billing` passed (3); `cargo test --lib` passed (542); `cargo fmt --check` passed; `git diff --check` passed.
- Review: PASS_WITH_NOTES. No P0-P2 findings. P3 notes: future partial provider updates could replace a fold-derived input; negative clamping has no telemetry; verify any future wire-level cache-write aliases. No repair required for current provider paths.
- Remaining risk: the reviewed future partial-update and clamping-observability notes remain; historical data still requires Phase 3 backfill.
- Changed paths: `src/usage/text.rs`, `src/usage/capture.rs`, known plan path.
- Checkpoint: ready for `phase(cache-token-normalization): normalize provider usage tokens` after exact-path staging and staged diff checks.

### Phase 2: Unified cache-rate reporting

- Status: IMPLEMENTED, review pending.
- Dependency: Phase 1 checkpoint `a77ecc7` passed review and is complete.
- Current phase baseline HEAD: `a77ecc7`.
- User baseline remains empty; concurrent `.opencode/plans/mcp-list-usage-filter.md` remains out of scope.
- Executor result: DONE. List/detail/summary/bucket SQL now bounds cache rates to [0,1] and prefers `cache_read_tokens`; overview presentation tests cover bounded ratios. Executor validation reported full lib tests, formatting, SQLx prepare check, and diff check passed.
- Review focus: verify aggregate `COALESCE` semantics for legacy `cached_tokens`, generated `.sqlx` churn, and consistency with overview totals.

## Review History

- 2026-08-20: Initial plan created. No implementation review yet.
- 2026-08-20: Phase 1 executor reported DONE; independent review pending.
- 2026-08-20: Phase 1 reviewer returned PASS_WITH_NOTES; no P0-P2 findings, no repair round required.

## Blocked Questions

- None. Production execution is intentionally outside this coding task and will require an explicit operational decision after the backfill path is reviewed.

## Baseline

- HEAD: `b28159b`
- Staged paths: none
- Unstaged paths: none
- Untracked paths: none

## Final Status

Implementation pending. Production deployment/backfill execution not performed.
