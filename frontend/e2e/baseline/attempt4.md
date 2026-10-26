# Advanced baseline slice

Current focused commands, from `frontend/`, with the local fixture and Vite
servers running:

```bash
bun x --package @playwright/cli playwright-cli -s=pf-baseline open --browser=chromium about:blank
bun x --package @playwright/cli playwright-cli -s=pf-baseline --raw run-code --filename=e2e/baseline/advanced.playwright > ../.cache/ai/react-migration/advanced-raw.json
bun -e 'const raw = await Bun.file("../.cache/ai/react-migration/advanced-raw.json").json(); await Bun.write("e2e/baseline/advanced.json", JSON.stringify(JSON.parse(raw), null, 2))'
bun run e2e/baseline/validate-advanced.ts
bun x --package @playwright/cli playwright-cli -s=pf-baseline close
```

## Recorded

- R-ENDPOINTS-UPSTREAMS: actual subscription edit dialog refresh, injected refresh
  failure with dialog retained, retry, clear, device login with pending then
  complete polls, clear, browser-flow start and completion using a synthetic
  loopback callback. No authorization link opened and no external account used.
  Stateful fixture endpoint/status becomes subscription+credential-present on
  completion and platform+credential-absent on clear. No token plaintext exists
  in the fixture responses or screenshots. Generated OAuth request/response
  contracts are reused; unknown flows fail rather than returning success.
- Quota: actual quota dialog renders a known 62% remaining window and an unknown
  weekly window; opening per-key history requests the trend endpoint. This does
  not certify stale/error/list-history/retry or quota aggregation.
- R-AVAILABLE-MODELS: same-origin empty collection response renders `没有数据`.
- R-BILLING: parent-approved baseline defect now tracked in machine matrix as
  `REACT-BILLING-RULES-PAGINATION`, mandatory repair rather than a waived target.
  Source/runtime evidence remains in attempt3 and four-slices artifacts.

## Exact next slice (not repeated work)

R-ENDPOINTS-UPSTREAMS and R-ENDPOINTS-ROUTES still require provider/plan/protocol
switch payloads; protocol-dependent model suggestions; dimension/chat override
roundtrip; canonical all-day vs inherit schedule; target ordering/enabled state;
quota list/stale/error/retry; OAuth stale/cancel/device/browser failure paths.
OAuth happy pending/complete/refresh/clear and refresh failure above need not be
repeated during diagnosis.

R-MCP detail/maintenance; R-USERS/R-RELAYS/R-MCP/settings domain write failures
and secret keep/replace/clear payload rules; remaining billing filter controls
are still pending. Existing earlier evidence remains authoritative for covered
flows. No whole route is promoted and foundation is not unblocked.

The complete combined baseline has not been rerun for this focused slice; run
all core, scenario, four-slice and advanced callbacks with merged run identities
at final P0 assembly. Current focused validators passing only certify their
named recorded assertions, not all P0 acceptance.
