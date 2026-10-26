# React frontend architecture

This document records the current architecture of the frontend after the
single-track Vue → React migration established by issue #1060. The executable
acceptance contract lives in `frontend/e2e/parity.matrix.ts` and
`docs/frontend-react-parity.md`; the executed phase history is kept under
"Phase history" below.

## Shape

One React 19 SPA rendered by the existing Rust admin server. The Vue app and
its packages are removed, so the React tree is the only frontend runtime. There
is no second mounted route tree, no Next.js/SSR runtime, no dual-track feature
flag, and no additional frontend server.

The build output stays `frontend/dist/index.html`, embedded by the unchanged
`build.rs` and `src/web_assets.rs` (`PROMPT_FERRY_FRONTEND_DIST` still overrides
the directory). History routing, deep links, cookies, `/api/v1` paths and the
generated fetch client semantics are unchanged.

```
Browser ── history routes ──► React route tree ──► domain views
                                     │                 │
                       TanStack Query (reads)     Zustand (shared UI)
                                     │                 │
                              generated admin API client
                                     │
                              Rust admin server (/api/v1)
```

## Toolchain

Dependency versions are exact pins. The application compiler is TypeScript
7.0.2. OpenAPI generation runs in a standalone nested package with TypeScript
6.0.3 and does not enter the application graph or bundle.

| Concern             | Choice                                                                                | Notes                                                                           |
| ------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| UI runtime          | `react` / `react-dom` 19.3.1                                                          | `@types/react` and `@types/react-dom` 19.3.0                                    |
| Build               | Vite 8.3.4 + `@vitejs/plugin-react` 6.1.2                                             | peer `vite ^8`                                                                  |
| Language            | TypeScript 7.0.2 (`tsc`)                                                              | stable native app compiler                                                      |
| Code generation     | `@hey-api/openapi-ts` 0.99.0 + TypeScript 6.0.3 in `tools/codegen`                    | isolated package, strict age gate, no app runtime                               |
| Styling             | Tailwind 4.3.3 + `@tailwindcss/vite` 4.3.3                                            | semantic sky/mist tokens, dark default                                          |
| Components          | source-owned shadcn components under `src/components/ui`                              | new-york/Radix, `cn` 0.5.0, `radix-ui` 1.8.1, CVA 0.7.1                         |
| Routing             | `react-router` 8.4.0 declarative/data mode                                            | no framework mode, no route codegen                                             |
| Server state        | `@tanstack/react-query` 5.104.3                                                       | cache, dedupe, invalidation, no auth-error retry                                |
| Client/shared state | `zustand` 5.0.15                                                                      | shared session state; keyed mutation guard remains framework-neutral            |
| Forms               | `react-hook-form` 7.90.0                                                              | reuse existing `admin-mappers/forms` validators and payload builders            |
| Tables              | shadcn Table components; the sortable headers are plain buttons in the page           | server pagination preserved, no virtualisation, no table package                |
| Charts              | `echarts` 6.1.0 with the existing pure option builders                                | React wrapper observes resize and disposes                                      |
| Icons               | `lucide-react` 1.58.0                                                                 | Lucide icons                                                                    |
| Tests               | Vitest 5.0.5, RTL 16.3.3, jsdom 30.1.3, Playwright CLI 0.1.22 for rendered acceptance | 830 active Vitest cases and 22 native Bun cases; the baseline is frozen history |

No second query/store/form/schema library, no runtime component kit, no CSS-in-JS
layer, and no sortable/virtualisation dependency without a documented need and
parent approval.

`bun run typecheck` runs `tsc --noEmit -p tsconfig.app.json`, whose include
list is `src/**/*.ts` plus `src/**/*.tsx`, so every surviving source file is in
the program.

## Ownership boundaries

- **Generated API** — `src/generated/admin-api/**`, `tools/codegen/openapi-ts.config.ts`
  and `openapi/admin-api.yaml` stay authoritative. Domain code calls the generated
  SDK through the shared client in `src/api.ts`. The root `openapi-ts` script
  runs the isolated codegen package before generating these files.
- **Pure domain layer** — `src/api/**`, `src/models/**`, `src/admin-mappers/**`,
  `src/format/**`, chart option builders, pagination arithmetic, redaction rules,
  request-overview/record formatters, the typed i18n catalogs and storage keys
  stay pure and are consumed directly. Keep the pure rule in those modules and
  the React adapter beside it: the adapter owns the reactive wiring, never the
  rule.
- **Server state** — TanStack Query owns reads, cache keys, invalidation and
  read-generation/stale-response handling. Query pending state is not a write
  exclusion mechanism.
- **Query cache ownership** — the application root keeps one query client for
  the login page and for the relay host, which are not bound to a Worker
  identity. The subtree behind the Worker auth gate reads through a client of
  its own, one per authenticated session epoch: a new principal, a new role, a
  new login of the same principal and the end of a session each start a new
  epoch with a new client, and the retired client's cache is emptied. The
  epoch is part of the session store (`src/lib/session.ts`), the client owner
  is `src/lib/query.ts`, and the boundary is applied by the gate
  (`src/lib/router.tsx`). Consumers read their client from the React context;
  no page holds a module-global client, and no Worker read is served from the
  root client.
- **Mutation exclusion** — one framework-neutral keyed guard
  (`src/lib/async-action.ts`: claim synchronously, immutable pending map, hold
  through the required refresh, release in `finally`, success effects only for
  `executed: true`) with a React adapter (`src/lib/use-async-action.ts`) on
  `useSyncExternalStore`. The keys are built by the domains:
  `src/stores/record-action-types.ts` owns the grammar (`recordActionKey`,
  `capturePayload`), `src/lib/settings-keys.ts` owns the settings sections, and
  each domain keeps its own helper (`endpoint:<id>`, `modelroute:<id>`,
  `clientkey:<id>`, `setting:<section>`, `configArchive:export`,
  `session:authentication`, …). A component-local duplicate guard is forbidden.
- **shadcn utilities** — generated components import the single `cn` 0.5.0 class
  merger; source utilities re-export it from `src/lib/utils.ts`. Keep CVA 0.7.1
  and Radix 1.8.1 for the selected components; do not duplicate class merging
  with `clsx` and `tailwind-merge`.
- **Session and role** — one Worker session guard and one independent relay-token
  guard. `/relay-host` never reads `session.me`/`isAdmin` and never maps relay
  identity onto admin. Admin-only routes redirect a non-admin to the first
  visible section; the same nav visibility rules drive the sidebar.
- **i18n and theme** — typed zh-CN/en-US catalogs and the existing storage keys
  (`prompt-ferry:locale`, `prompt-ferry:theme-mode`, display-time-zone key) stay.
  `src/lib/i18n.tsx` and `src/lib/theme.tsx` are the current providers; the
  first-paint theme script and the `class="dark"`/`color-scheme` behaviour stay.
- **Charts, dialogs and destructive actions** — preserve chart semantics and
  palettes, focus/dismissal guards, and irreversible-action confirmation. Visual
  density may change; capability may not.

## Request and error semantics

The client keeps `credentials: include`, `responseStyle: data`,
`throwOnError: true`, nullable omission (`null` vs absent), owner scoping, error
masking and the backend error envelope. No endpoint, schema, route, cookie or
backend migration is invented in the frontend port.

## Acceptance architecture

- `frontend/e2e/parity.matrix.ts` is the route/role/action contract with stable
  IDs, read endpoints, exact mutation keys, mapped legacy tests, required
  scenarios, gate phase and status.
- `frontend/e2e/fixtures/` is the typed stateful full-app mock: real login and
  router flow, admin/non-admin identities, an independent relay token, paginated
  collections, mutations, error envelopes, injected failures/latency and the
  zero-token/header-stress datasets. It never calls a production endpoint and
  rejects unmocked API paths.
- `frontend/e2e/baseline/` holds the frozen Playwright CLI baseline, screenshots,
  evidence and the matrix validator; it is immutable history and is never
  regenerated in place. `frontend/e2e/foundation/` holds the React foundation
  callback, route-test mappings, screenshots and the active-entry graph check.
  Foundation markers identify placeholders; they do not mark domain parity.
- Rendered acceptance runs against the real app; isolated component tests are
  supplementary. The baseline capture stays frozen and the React acceptance runs
  serially against the stateful fixture, never side by side with a baseline
  capture.

## Phase history

The serial phases from record 20315, constrained by the issue body, as executed.
This table is history: it records what each phase did, not a current gate, and
none of its rows is a final acceptance status.

| Phase | Scope                                                                                                                        | Gate (executed)                                                                                                                                                                                                                                               |
| ----- | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P0    | parity matrix, architecture, typed fixtures, baseline evidence                                                               | matrix validator + baseline evidence (this phase)                                                                                                                                                                                                             |
| P1    | install stack, single React entry, providers/guard, generated client, embedding smoke                                        | named foundation checkpoint `1060-react-foundation`: age-enforced `bun install --no-save`, codegen, TS7 typecheck, foundation tests, Vite build, active-entry graph check, stateful Playwright CLI auth/shell smoke, `test -f dist/index.html`, `cargo build` |
| P2    | pure modules + test port; only then remove Vue runtime                                                                       | no Vue-exclusive implementation remains                                                                                                                                                                                                                       |
| P3    | shell, auth, navigation, roles, relay-host identity                                                                          | login/redirect, admin/non-admin nav, relay independence                                                                                                                                                                                                       |
| P4    | reads+core dialogs by domain (keys/users/models, endpoints/routes, MCP/relays, usage, billing/approvals, redaction/settings) | matrix rows and browser scenarios                                                                                                                                                                                                                             |
| P5    | writes in the same domain order                                                                                              | exact mutation keys, conflict/failure/draft evidence                                                                                                                                                                                                          |
| P6    | cleanup, full matrix, full tests, one final audit                                                                            | no partial/placeholder routes                                                                                                                                                                                                                                 |

Each implementation phase got an exact allowlist and matrix slice before
delegation. The foundation checkpoint (`1060-react-foundation`, review record 20982) passed its own boundary and did not replace the final audit. During that
checkpoint the legacy Vue sources and dependencies were reference-only and
outside the active React import graph; they were not a second mounted
application or a release fallback, and they are now removed.

## Open risks

- The nested generator must resolve TypeScript 6.0.3 independently of the TS7 app
  compiler; its generated files must remain byte-identical.
- The zero-token fold rule is a product decision already made: all rows with
  `total_tokens === 0` fold by default (success-only, mixed-failure and all-failed
  alike), the reveal shows them, aggregation is untouched, unknown tokens are
  never coerced to `0`, and labels must say zero-token rather than all-failed.
- Header layout must eliminate the measured overflow; pixel parity is not a gate.
- Fixture coverage proves observable behavior, not live persistence.
