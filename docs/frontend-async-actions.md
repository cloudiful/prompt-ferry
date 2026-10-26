# Frontend asynchronous actions

[简体中文](frontend-async-actions.zh-CN.md)

Remote mutations claim a key in the shared guard. `src/lib/async-action.ts` owns
the pending map and exposes `runAsyncAction`, `getPendingActions` and
`subscribeToAsyncActions`; `src/lib/use-async-action.ts` is the React adapter
(`useAsyncAction`) that the pages and the domain modules use. There is one guard
instance per exclusion unit, and every entry point for that resource uses the
same key. A held key returns `{ executed: false }`; UI handlers perform
success-only effects — notifications, navigation, dialog close, and draft reset —
only when `executed` is true.

Keys are built by the domain, not by the component.
`src/stores/record-action-types.ts` owns the grammar: `recordActionKey(prefix, id)`
yields `<prefix>:<id>`, a record that does not exist yet shares one
`<prefix>:create` slot, and `capturePayload` snapshots the body before the first
await. Each domain adds its own helper (`endpointActionKey`,
`modelRouteActionKey`, `mcpActionKey`, `userActionKey`, `clientKeyActionKey`,
`relayActionKey`, `approvalActionKey`, `billingPriceRuleActionKey`, `settingKey`,
`usageConversationActionKey`).

Keep the key held through the mutation and required refresh. Capture record
identity, selected owner, and submitted payload before awaiting. Propagate pending
state to conflicting controls: disable their inputs and dismissal, keep unrelated
resources usable, and show the spinner only on the action that started. Errors
remain visible and drafts retryable. Do not create a component-local guard
around a shared store action.

## Action coverage

- **Endpoint and model routes** — `endpoint:<id>` covers endpoint save/delete/toggle and OAuth device/browser start, each poll, completion, refresh, and clear. `modelroute:<id>` covers route save/delete/toggle. Probes use independent `networktest:<domain>:<id>` keys. Pending state reaches rows and the endpoint dialog. The OAuth controls receive the page-owned callback through the dialog; each device poll claims separately, while human approval does not hold the key. Evidence: `src/react-tests/endpoints.test.tsx`, `src/react-tests/endpoints-oauth.test.tsx`, `src/react-tests/model-routes.test.tsx`, `src/react-tests/endpoints-model.test.ts`.
- **MCP servers** — `mcp:<id>` covers save/delete and `mcp:create` covers create; the catalog probe uses `mcptest:<id>`. Pending state reaches the row and dialog. Catalog edits are local drafts. Evidence: `src/react-tests/mcp.test.tsx`.
- **Users** — `user:<id>` covers save/delete/password reset; `user:create` covers create. Pending state reaches the user row/dialog and reset control. Evidence: `src/react-tests/users.test.tsx`.
- **Client keys** — `clientkey:<id>` covers label/save/delete/toggle; create uses a key scoped to the captured selected user. Pending state reaches the key row/dialog. Evidence: `src/react-tests/api-keys.test.tsx`.
- **Relays** — `relay:<id>` covers save/delete/reconnect. The owner pending map reaches row controls and the dialog; fields and dismissal stay disabled through the mutation and refresh, and only the originating action spins. Evidence: `src/react-tests/relays.test.tsx`.
- **Relay host** — `host:local` covers role/settings/restart as one host resource. The shared pending state disables conflicting controls, only the origin spins, and role/settings completion preserves fields edited after submission. Evidence: `src/react-tests/relay-host.test.tsx`, `src/react-tests/relay-host-model.test.ts`.
- **Settings** — `setting:<section>` covers each settings section: `rawRetention`, `streamDeltaBatching`, `modelRouteWhitelist`, `relayIpWhitelist`, `llmReview`, `storage` and `cacheAlert`. The request-content-logging and usage-retention forms are one stored value, so they share `setting:rawRetention`; the held-off form stays dirty and retries once the key is released. Evidence: `src/react-tests/settings-requests.test.tsx`, `src/react-tests/settings-review.test.tsx`, `src/react-tests/settings-network.test.tsx`, `src/react-tests/settings-storage.test.tsx`, `src/react-tests/settings-cache-alert.test.tsx`.
- **Redaction** — `redaction:rules` covers save and `redaction:preview` covers the non-persisting preview. Their pending feedback is separate; stale preview responses are ignored. Evidence: `src/react-tests/redaction-model.test.ts`, `src/react-tests/redaction.test.tsx`.
- **Config archive** — `configArchive:export` and `configArchive:import` are independent keys, and the import key is shared by the preview and the confirmed apply. Confirmation, file validation, and retryable draft state stay in the wizard. Evidence: `src/react-tests/config-archive.test.tsx`.
- **Billing** — `billingrule:<id>` is shared by rule save/delete/toggle; `billing:reprice` is separate. Evidence: `src/react-tests/billing-state.test.ts`, `src/react-tests/billing.test.tsx`.
- **Approvals** — `approval:<id>` is shared by approve/reject. The chosen action spins and its conflicting alternative disables. Evidence: `src/react-tests/approvals.test.tsx`.
- **Usage maintenance and perspective** — `usage:maintenance` is shared by clear/prune; `usage:perspective` is independent and ignores stale overview results. The existing clear dialog remains the maintenance UI. Evidence: `src/react-tests/usage-store.test.ts`, `src/react-tests/usage-clear.test.tsx`.
- **Conversation routing** — `conversation:<id>` is shared by override save/clear and session-affinity reset. Same-conversation actions and dialog dismissal conflict; other conversations stay independent. Evidence: `src/react-tests/usage-detail-store.test.ts`.
- **Authentication** — the Worker session and the relay token own separate keys and separate stores in `src/lib/session.ts`: `session:authentication` for the Worker session and `relay-session:authentication` for the relay management token. Busy spans each mutation and nested bootstrap; duplicate login cannot navigate or clear a form. Bootstrap/status reads do not clear a write claim. Evidence: `src/react-tests/parity-routes.test.ts`, `src/react-tests/locale-theme.test.tsx`, and the rendered acceptance in `frontend/e2e/foundation/browser-result.json`.

The guard itself is covered by `src/react-tests/async-action.test.tsx`: the key is
claimed synchronously, held through the required refresh, released after a
failure, and the published pending map is replaced rather than mutated.

## Reads and UI conventions

Search, list, pagination, bootstrap, status, and quota reads keep their existing
fetch state and stale-response rules; they do not use a mutation key. A follow-up
read may stay inside its write callback when the action must remain pending
through refresh. Local-only editor drafts do not receive fake network loading.

An independent async read keeps its own loading state: the control shows its
pending state while its handler runs, and a spinner is honest only when the
handler returns the actual promise. Read loading never substitutes the shared
domain mutation guard. Use one completion path and one notifier; a refused call
must not duplicate a success toast. UI-only action work leaves public APIs and
generated types unchanged.
