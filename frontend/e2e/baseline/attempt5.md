# Route form payload slice

From `frontend/` with fixture/Vite running:

```bash
bun x --package @playwright/cli playwright-cli -s=pf-baseline open --browser=chromium about:blank
bun x --package @playwright/cli playwright-cli -s=pf-baseline --raw run-code --filename=e2e/baseline/form-payloads.playwright > ../.cache/ai/react-migration/form5-raw.json
bun -e 'const raw = await Bun.file("../.cache/ai/react-migration/form5-raw.json").json(); await Bun.write("e2e/baseline/form-payloads.json", JSON.stringify(JSON.parse(raw), null, 2))'
bun run e2e/baseline/validate-form-payloads.ts
bun x --package @playwright/cli playwright-cli -s=pf-baseline close
```

## Newly asserted, R-ENDPOINTS-ROUTES

- Actual target settings: invalid dimension `0` produces aria-invalid and
  disables save without PATCH. Positive `1536` saves and reopens unchanged.
  Blank dimension saves explicit null.
- Adding a schedule with both times blank and selecting weekdays saves exactly
  `00:00`–`24:00` plus `[1,2,3,4,5]`; reopen displays those times. Removing the
  row saves `active_windows: []` (inherit endpoint schedule).
- Changing embeddings to Chat exposes existing chat controls; high effort,
  self-summarize, developer-to-system and thinking adaptation save through the
  generated route request and reopen with Chat/self-summarize intact.
- Keyboard ArrowDown on the real target grip moves target/model/protocol as one
  unit. Toggle enabled state, add then remove a target, save exactly two targets
  with the reordered endpoints, original model/protocol associations and state.
- Actual model-suggestion requests include embeddings, chat and responses
  `native_api` selectors. The fixture honors those axes instead of returning
  an identical union for every selector.

Fixture corrections: route compact mode is persisted rather than forced to
passthrough on save; generated route request bodies are recorded with nonempty
proxy values redacted. No product code or pure mapper was rewritten. No
synthetic token/credential plaintext is in the screenshot or report.

## Exact next untested slice

Do not repeat the route dimension/schedule/chat/order/suggestion cases above.
Remaining R-ENDPOINTS-UPSTREAMS: actual provider/plan/protocol transitions and
normal-vs-subscription key form payloads. R-ENDPOINTS-UPSTREAMS quota list/stale/
error/retry and OAuth negative cancel/stale/device/browser paths remain pending
(happy login/refresh/clear and refresh failure already covered by attempt4).
R-MCP detail/maintenance; R-USERS/R-RELAYS/R-MCP/settings failure+retry and
resource-supported secret keep/replace/clear; remaining R-BILLING filters remain
pending. No baseline defect was diagnosed in this form slice.

Original combined evidence and other focused slices are retained, not rerun.
Final assembly still requires a combined execution and merged run identities.
No route is globally promoted; foundation stays blocked on P0 acceptance.
