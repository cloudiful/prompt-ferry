# Anthropic Stream Retry Packaging

## Goal

Make Anthropic-compatible streaming 5xx and upstream transport failures recognizable as retryable by OpenCode, while preserving non-retryable Anthropic client errors and the existing no-replay rule for already committed streams.

## Tracking

LOCAL_PLAN. The completed plan is archived under `.opencode/plans/archive/`.

## Background

The relay returns HTTP 200 after a streaming response starts. If the upstream stream then fails, the worker emits an internal `upstream_stream_error` with status 502. The relay currently maps Anthropic streaming errors through `anthropic_outward_code`; OpenCode 1.18.18 and the Anthropic SDK require a retryable Anthropic error classification for this SSE-only failure path. The worker must not replay a stream after downstream output has been committed.

## Baseline

- HEAD: `68761be1f0cba84487702eddfda01e7f330afacf`
- Existing staged paths: none
- Existing unstaged paths: none
- Existing untracked paths: none
- Baseline paths are user-owned and out of scope for every phase: none

## Constraints

- Only the orchestrator may create the checkpoint commit.
- Executors and reviewers must not stage, commit, or modify git state.
- Do not change OpenCode installation, databases, services, or secrets.
- Do not add worker-side replay for `CommittedStream` failures.
- Preserve authentication, validation, quota, and ordinary rate-limit error semantics.
- Do not stage `Cargo.lock`.

## Acceptance Criteria

- Anthropic streaming upstream transport/5xx failures are emitted with the standard retryable Anthropic SSE error type expected by the configured OpenCode/Anthropic SDK path.
- Existing Anthropic non-retryable status mappings remain unchanged.
- Existing `overloaded_error` behavior remains unchanged.
- The response remains valid Anthropic SSE with HTTP 200 after the stream has started.
- Regression tests cover retryable transport/5xx mapping and non-retryable boundaries.
- `cargo fmt --all -- --check` passes.
- `cargo test --workspace` passes.
- Phase reviewer reports no confirmed P0-P2 findings.

## Phases

### Phase 1: Implement Anthropic streaming retry packaging

Outcome: update the Anthropic outward error classification and focused relay tests.

In scope:

- `src/relay/public_proxy/ai.rs`

Out of scope:

- all other source, test, configuration, documentation, database, and OpenCode installation paths

Acceptance criteria:

- 5xx/transport errors from the committed Anthropic stream map to the retryable Anthropic error type.
- 401/403/429 and ordinary client error mappings do not become retryable.
- Focused tests assert the exact SSE envelope and boundary mappings.

Validation commands:

- `cargo test --lib relay::public_proxy::ai`
- `cargo fmt --all -- --check`

Dependencies: baseline only.

### Phase 2: Full validation and aggregate review

Outcome: validate the phase across the workspace and inspect the committed change for regressions.

In scope:

- phase 1 changed paths and plan path

Out of scope:

- unrelated worktree paths, OpenCode installation, and service state

Acceptance criteria:

- `cargo test --workspace` passes.
- Final reviewer confirms the requested behavior and no unresolved P0-P2 findings.

Validation commands:

- `cargo fmt --all -- --check`
- `cargo test --workspace`
- `git diff --cached --check` before checkpoint commit

Dependencies: phase 1 implementation and review pass.

## Decisions

- Keep `CommittedStream` non-retryable inside prompt-ferry. OpenCode may retry the whole session only after receiving a retryable SSE error; prompt-ferry must not duplicate a partially delivered stream itself.
- Use Anthropic protocol error classification for the Anthropic endpoint rather than OpenAI `server_error` JSON, because the endpoint is consumed through the Anthropic SDK.
- Keep the change focused on outward stream classification and tests; do not alter non-stream error handling or worker attempt policy in this phase.

## Review History

- Phase 1: implemented. `anthropic_outward_code` now maps any 500..=599 status
  to `overloaded_error` (the canonical Anthropic retryable error class),
  regardless of the internal code; the 401/403/429/`api_error` boundaries
  are preserved. The relay's stream envelope still emits HTTP 200 with
  `event: error` framing. Regression coverage: focused unit tests on
  `anthropic_outward_code` for 500/502/503/504/529 and the 401/429/400
  non-retryable boundaries, plus a `/v1/messages` integration test that
  exercises a `502 upstream_stream_error` mid-stream and asserts the exact
  SSE envelope `{"type":"error","error":{"type":"overloaded_error",...}}`.
- Phase 1 validation: `cargo fmt --all -- --check` passed; `cargo test --lib
  relay::public_proxy::ai` passed with 8 tests.
- Phase 1 review: `PASS`. Reviewer confirmed the helper is only used by the
  Anthropic streaming error branch, non-streaming and OpenAI/Responses paths
  are unchanged, and no P0-P2 findings remain.
- Phase 1 changed paths: `src/relay/public_proxy/ai.rs` and this plan file.
- Phase 1 remaining risk: a non-OpenCode Anthropic-compatible SDK may apply
  different retry policy, but the mapping matches `@ai-sdk/anthropic` and
  OpenCode 1.18.18.
- Phase 2 validation: `cargo fmt --all -- --check` passed; `cargo test
  --workspace` passed with all workspace unit, integration, and doc tests
  green (532 library tests plus the integration suites).
- Phase 2 aggregate review: `PASS_WITH_NOTES`. No P0-P2 findings remain.
  Non-blocking P3 notes: the stream helper comment can more precisely say
  internal codes are retained in diagnostics rather than the SSE envelope,
  and the inclusive 599 boundary is not explicitly unit-tested.

## Blocked Questions

- None.

## Final Status

COMPLETE

## Residual Risks

- A client using an Anthropic-compatible SDK with custom retry rules may classify standard Anthropic `api_error` differently from OpenCode. The implementation must be validated against the local OpenCode 1.18.18 behavior where possible.
- A stream can only report an error after partial output; no client can retract bytes already displayed.
