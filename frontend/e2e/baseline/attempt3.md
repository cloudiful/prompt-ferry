# Four-slice baseline disposition

Focused repeatable command (with local fixture/Vite servers running):

```bash
cd frontend
bun x --package @playwright/cli playwright-cli -s=pf-baseline open --browser=chromium about:blank
bun x --package @playwright/cli playwright-cli -s=pf-baseline --raw run-code --filename=e2e/baseline/four-slices.playwright > ../.cache/ai/react-migration/four-slices-raw.json
bun -e 'const raw = await Bun.file("../.cache/ai/react-migration/four-slices-raw.json").json(); await Bun.write("e2e/baseline/four-slices.json", JSON.stringify(JSON.parse(raw), null, 2))'
bun run e2e/baseline/validate-four-slices.ts
bun x --package @playwright/cli playwright-cli -s=pf-baseline close
```

`four-slices.json` is separate from the retained combined baseline; its five
screenshots and assertions supplement, not replace, that evidence.

- **R-SETTINGS-CONFIG-ARCHIVE:** cancel confirmation issues no import POST and
  clears the wizard. Confirmed apply issues a real injected 500 then, after
  re-preview and confirmation, a real 200. Ledger includes exact synthetic
  archive/passphrase payload. Success renders `配置已导入`, refreshes audit and
  shows the newly appended import audit row. Observed distinction: preview
  failure clears passphrase; apply failure retains it until cancellation or
  success. No production behavior was changed to force secret retention/clearing.
- **R-BILLING:** 32 charges make a second 25-row page observable. Heading-scoped
  charges pagination sends `first=25&rows=25`, changes charges, leaves rules
  unchanged. Model filter sends `requested_model=minimax-m2` on summary and
  charges; fixture applies field/date filters and derives summary from matching
  charges (16 matches in the final run). The price-rule pager click still issues
  no request or row change even when heading-scoped. The owned Vite server
  subsequently reported `TypeError: $setup.billing.refreshPriceRules is not a function`
  at `src/pages/BillingPage.vue:222` through `BillingPriceRulesPanel.vue:150`.
  This is a concrete current product defect, not fixture pagination failure.
  It is reported to the parent for disposition; no product fix is authorized.
  Other filter controls remain pending, although the fixture supports their
  declared fields.
- **R-USAGE-AI:** `known-baseline-defect`, source diagnosis recon 20557: string
  headers have no sorting toggle. Actual click produces no records request, no
  aria-sort change and no header button. Required future fix ID
  `REACT-AI-SORT-ACCESSIBLE`: accessible keyboard-operable buttons and generated
  `sort_field`/`sort_order` queries. This is evidence of current broken behavior,
  not a waiver for React acceptance.
- **R-REDACTION-RULES:** case-insensitive server search sends `search=SECRET-10`,
  `first=0`, `rows=10`, returns only `fixture-secret-10`. Invalid `[` regex sets
  aria-invalid, shows the warning and issues no PATCH on save. Correcting it
  produces PATCH. Two identical `contains/text` signatures show duplicate
  warnings; a `contains/line` signature shows the supported conflict warning;
  save payload has exactly two signatures (identical pair deduped, differing
  scope retained). Fixture does not implement a regex/redaction engine.

All rows remain partial: this is a bounded slice, not full P0 completion.
See `remaining.md` for other domain work. Matrix validation and the focused
validator do not waive untested scenarios. Original combined command must run
again when the final P0 baseline is assembled.
