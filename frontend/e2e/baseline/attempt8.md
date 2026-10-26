# Residual MCP maintenance and domain retries

Run from `frontend/` with the fixture and Vite servers described in the e2e README:

```bash
bun x --package @playwright/cli playwright-cli -s=pf-baseline open --browser=chromium about:blank
bun x --package @playwright/cli playwright-cli -s=pf-baseline --raw run-code --filename=e2e/baseline/mcp-maintenance.playwright
bun x --package @playwright/cli playwright-cli -s=pf-baseline --raw run-code --filename=e2e/baseline/domain-retries.playwright
bun run e2e/baseline/validate-residual.ts
bun x --package @playwright/cli playwright-cli -s=pf-baseline close
```

The callbacks reset fixtures and authenticate through the real login UI. Decode
returned JSON strings when refreshing the reports. `mcp-maintenance.json` stores
a compact projection of the successful output; it does not retain the full page
text after the clear. No synthetic credentials are captured in either report.

## Observed

- `R-USAGE-MCP`: switch from overview to Records, click the explicit View Details
  action, observe request and response payloads. Cancel clear-history without a
  POST; reopen, choose all users (`全部记录` scope option), check all records,
  confirm once, observe one successful clear POST and empty records refresh.
- `R-USERS`, `R-MCP`, `R-RELAYS`: creation returns an injected 503. The actual
  dialog and identifying draft remain; retry succeeds and only then closes.
  Each sanitized ledger has exactly the failed POST followed by the successful
  POST to the same resource collection. These checks do not yet compare whole
  payloads or certify secret replacement/clear semantics.

## Fixture correction

The clear endpoint has no category parameter in `RequestRecordsClearRequest`.
The previous fixture read a nonexistent query category and cleared only AI rows,
so a successful clear on the MCP page left the list populated. It now uses the
generated request body and filters both collections by scope/user/time. This was
a fixture defect, not a product defect; no Vue source was changed.

## Validation

- Both focused CLI callbacks ran successfully against the current fixture.
- `bun run e2e/baseline/validate-residual.ts`: pass.
- `bun run e2e/baseline/validate-matrix.ts`: structural pass; historical 143/38
  evidence is unchanged and not a current complete run.
- `bun run typecheck`, `bun run format:check`: pass.
- `bun run test`: 41 Vitest tests pass; `bun test tests`: 883 pass, 0 fail.
- Focused changed fixture check passes:
  `bun x --no-install tsc --ignoreConfig --noEmit --target esnext --module esnext --moduleResolution bundler --allowImportingTsExtensions --skipLibCheck e2e/fixtures/handlers/records.ts`.
  An attempted server check with `--types bun` could not resolve Bun typings;
  no dependency was installed. This focused check does not cover the Bun server
  ambient API.

## Remaining

Supported secret keep/replace/clear payload assertions; settings individual
failure/retry; remaining billing filters; prune if exposed; serial combined
runner/current run identities and strict complete per-scenario validation. No
new confirmed product defect or authorization decision blocks these; they are
unexecuted work, not waived acceptance. P0 remains PARTIAL and P1 is not started.
