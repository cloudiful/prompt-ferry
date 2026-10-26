# Quota stale observation baseline

Run with fixture/Vite listening on the documented local ports:

```bash
bun x --package @playwright/cli playwright-cli -s=pf-baseline open --browser=chromium about:blank
bun x --package @playwright/cli playwright-cli -s=pf-baseline --raw run-code --filename=e2e/baseline/quota-stale.playwright
bun run e2e/baseline/validate-quota-stale.ts
bun x --package @playwright/cli playwright-cli -s=pf-baseline close
```

The callback returns a JSON-encoded string. Decode it to the report object and
save as `quota-stale.json` when refreshing evidence.

`R-ENDPOINTS-UPSTREAMS:quota-stale` observes the actual quota dialog rendering
cached observation, failed refresh and next retry labels with retained 62%
remaining / 38 of 100 values. After the fixture observation becomes fresh, a
new route read/dialog opening removes all stale/error/retry labels while keeping
the same values. Fixture control `/__fixtures/quota` changes only observation
metadata; it does not simulate a UI success or change the quota math.

Still unverified: `R-USAGE-MCP` detail/history maintenance;
`R-USERS`, `R-RELAYS`, `R-MCP` and settings mutation failure/retry plus supported
secret semantics; remaining `R-BILLING` filters; final serial combined assembly
with current run identities and frontend full test/typecheck gates. MCP record
navigation was inspected, but clicking the row body did not open a detail; this
is not diagnosed as a product defect or promoted as detail coverage. No required
behavior is waived. P0 remains partial; foundation is not authorized.
