# Final residual payload and filter coverage

Run focused callbacks (or run the complete current baseline) from `frontend/`:

```bash
bun x --package @playwright/cli playwright-cli -s=pf-baseline --raw run-code --filename=e2e/baseline/relay-secrets.playwright
bun x --package @playwright/cli playwright-cli -s=pf-baseline --raw run-code --filename=e2e/baseline/mcp-secrets.playwright
bun x --package @playwright/cli playwright-cli -s=pf-baseline --raw run-code --filename=e2e/baseline/endpoint-secrets.playwright
bun x --package @playwright/cli playwright-cli -s=pf-baseline --raw run-code --filename=e2e/baseline/settings-secrets.playwright
bun x --package @playwright/cli playwright-cli -s=pf-baseline --raw run-code --filename=e2e/baseline/billing-filters.playwright
bash e2e/baseline/run-baseline.sh
```

The combined runner resets the stateful fixture per callback and includes the
existing four-slice, route-payload, OAuth/quota and upstream callbacks together
with attempt-8 quota, MCP maintenance and domain retry callbacks. It merges all
callback JSON from that invocation into `e2e/baseline/evidence.json`; the strict
matrix validator fails on missing residual reports, callback errors or missing
assertions. Historical 143-capture evidence is replaced by this invocation, not
reused as the current run. The callbacks use synthetic secrets only and reports
retain secret payload values only as in-memory assertions; they never serialize
secret strings.

## Focused observations

- Relay update: all four stored PEM/key fields produce `{mode: "keep"}` with no
  value on an untouched save; replacing each sends exact synthetic payloads;
  choosing clear sends value-free `{mode: "clear"}` for each and reload shows
  the fields unset. The fixture now applies those modes to response-side presence
  flags only, without retaining synthetic secret text.
- MCP Basic auth: a new synthetic password is sent, subsequent blank password
  is masked and represented by empty string (server contract means keep), and a
  replacement sends the new value. MCP server proxy replacement, untouched keep
  (omitted request field), clear (`""`) and round-trip were asserted. The server
  source explicitly preserves Basic credentials on blank/absent values and when
  auth mode changes, so a clear operation is not applicable; no UI clear claim
  is made.
- Upstream endpoint API-key rows: untouched keys remain omitted/empty, replacing
  sends a new synthetic key, and removing a non-required row removes it from the
  request. Endpoint proxy settings likewise cover replace, untouched omission,
  explicit clear and the resulting direct/no-proxy state. The fixture now treats
  empty proxy string as a clear instead of incorrectly preserving its flag.
- Raw object storage: empty secret inputs send keep patches. After a synthetic
  access/secret replacement plus a prefix edit, injected PUT 503 preserves all
  drafts; the retry sends the same captured payload, succeeds, and clears the
  transient secret inputs. The UI exposes only keep/replace, so clear is not an
  applicable control. Cache-alert similarly tests blank keep and 503 draft plus
  retry replacement; its mapper has keep/replace only. These are the settings
  slices covered here, not every settings tab's mutation error case.
- Billing: selects admin user/operator, key ID 13, matching public model and
  endpoint, unknown usage and unpriced status. The actual charge GET includes
  every corresponding generated query key and the stateful fixture returns the
  matching unknown/unpriced row. This proves controls, query mapping and a
  consistent filtered result—not live billing persistence.
- MCP record detail and maintenance and users/MCP/relays create retries remain
  covered by attempt8 callbacks, now integrated in the combined run.

## Fixture corrections

The record-clear fixture and secret-presence behavior were corrected from
observed app contracts; no product source, API schema, dependency, installation,
or production config changed. MCP catalog/test callbacks now resolve route IDs
against seeded servers rather than accidentally parsing the route with the
single-server regex. No arbitrary external requests are introduced.

## Final combined run and remaining scope

`bash e2e/baseline/run-baseline.sh` completed with exit 0. It ran all 13 focused
callbacks serially with fixture resets, merged their current-run reports, and
passed the strict matrix validator: 143 captures, 38 scenario flows, 22 routes,
6 redirects, 28 scenarios, and 134 legacy tests mapped. These counts describe
this current baseline run and do not imply every route's required migration
scenario is complete.

Full all-settings per-section failure/retry coverage is not complete: this slice
covers storage and cache-alert; review only remains an inspected read surface.
Other settings writes and any applicable secret actions need their own UI/request
assertions. No prune control is exposed: `src/stores/request-records-actions.ts`
exports `pruneHistory`, but active-source usages end at the store export while the
MCP toolbar exposes clear-history only. Prune is source-backed as not a visible
UI action, not an untested required control. Matrix statuses remain partial; P0
is not foundation-ready until every required migration scenario has evidence.

Additional gates after fixture freeze: `bun run typecheck`, `bun run format:check`,
`bun run test` (41 passed), `bun test tests` (883 passed, 0 failed), focused
fixture handler TypeScript check, and `git diff --check` all passed. The full
format check passed after removing the task-created Playwright CLI snapshot from
`.playwright-cli`; no pre-existing or shared artifacts were removed. The owned
fixture and Vite ports are stopped.
