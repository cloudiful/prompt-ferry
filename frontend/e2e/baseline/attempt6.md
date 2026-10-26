# Upstream payload and negative authentication slice

With fixture/Vite running, execute each callback through the named Playwright
CLI session `pf-baseline` using `--raw run-code --filename=...`:

- `e2e/baseline/upstream-payloads.playwright`
- `e2e/baseline/auth-quota-negative.playwright`

Decode the raw JSON string once to obtain each report object and retain it as
`upstream-payloads.json` or `auth-quota-negative.json`. Run:

```bash
bun run e2e/baseline/validate-upstream-negative.ts
```

## Asserted observations

R-ENDPOINTS-UPSTREAMS: subscription has no key input and submits an empty key
array; Platform exposes the key editor and submits one synthetic key row.
Manual Embeddings survives save/reopen. Switching to generic submits the
Platform plan, null region and disabled MCP. Request ledger secret fields are
redacted; no real credential or external authentication is used.

Synthetic device and browser start failures return 500. Escape cannot dismiss
the pending login; navigation removes it. One already in-flight delayed poll
completes, but no subsequent poll or OAuth success-status refresh occurs, and
reopening remains logged out. Quota history list returns injected 500 then 200
on actual retry. Assertions inspect generated requests, not screenshots alone.

Fixture persistence honors saved native protocol. Delayed successful faults now
continue into the real handler instead of returning an error-shaped 200. This
changes the shared fixture: older combined evidence has NOT been rerun against
this revision and must not be treated as current combined certification.

## Remaining acceptance

Quota stale state; MCP details/maintenance; users/relays/MCP/settings failed
mutations and retries, and supported secret keep/replace/clear; remaining billing
filter controls. No new product defect or missing external prerequisite was
identified in this slice. Earlier accepted form and happy OAuth slices were not
repeated. No route is globally complete. Final combined sequential callbacks,
merged run identities and frontend 883/41 suites remain required before P0 can
close or React foundation can begin.
