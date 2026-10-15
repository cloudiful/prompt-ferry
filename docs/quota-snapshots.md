# ChatGPT quota snapshots

The admin console keeps a bounded history of successful OpenAI ChatGPT subscription quota observations. The quota dialog shows the observation time and whether the displayed values are cached, stale, refreshing, or being shown after a failed refresh. A failed refresh does not replace the last successful observation. The per-key default-text status is unknown for stale, failed, or reset-passed observations; raw windows and history remain visible. See [quota feedback](quota-feedback.md) for status and summary behavior.

## Requirements and collection

- Persistent quota snapshots and history require the PostgreSQL worker backend. Standalone SQLite continues to show the current quota directly but does not provide persistent history or autonomous collection.
- Periodic collection uses Valkey coordination. If Valkey coordination is unavailable, periodic collection stays disabled; manual refresh and selected-request wakeups remain available.
- Active endpoints refresh on a five-minute cadence when an actual request was recorded in the preceding 30 minutes. Idle endpoints refresh hourly. A selected OpenAI subscription request can enqueue a nonblocking refresh when its observation is older than five minutes.
- Opening an endpoint with no saved observation may perform one fetch. The history panel loads only when expanded; each page contains at most 200 rows.
- Successful observations are retained for 30 days. The API applies the retention window during reads, and periodic cleanup removes expired rows in bounded batches.

Migrations are applied by the application's normal deployment initialization with `db_init` before workers start. Use the deployment's authorized database migration procedure.

## History API

Administrators can request a page for one endpoint:

```text
GET /api/v1/admin/endpoints/{endpoint_id}/quota-history?limit=50
```

`limit` defaults to 50 and must be between 1 and 200. Continue with the opaque decimal `before_id` returned as `next_cursor`; snapshot IDs are strings to preserve 64-bit precision in JavaScript. The response contains observation time, plan type, the optional upstream limit flag, normalized windows, source (`manual`, `request`, or `periodic`), the next cursor, and the retention period.

History is endpoint-scoped and remains available for disabled OpenAI endpoints. It contains only successful normalized quota windows; it never contains access tokens, account identifiers, raw upstream payloads, API keys, or raw error text. History reads do not contact the quota provider. A PostgreSQL storage error is returned as a generic service error rather than a partial or fabricated quota record.

## Generic per-key quota history

All capable providers (MiniMax, CommandCode, OpencodeGo, OpenRouter, GLM, DeepSeek, and mirrored ChatGPT subscription results) record one observation row per API key for every actual fetch — manual, request-triggered, or periodic sampling. Cache hits never append rows. The quota dialog mounts a separate per-key history panel for every provider except generic; the legacy OpenAI panel stays available unchanged beside it. Rows render at their own observation timestamp, so a window that has since expired still shows what was observed rather than a current-quota claim. Failed fetches appear as honest failure rows with a stable error code and no synthetic percentage.

Administrators can read one endpoint's rows, optionally filtered to a single key:

```text
GET /api/v1/admin/endpoints/{endpoint_id}/quota-observations?key_id=&limit=50
```

`limit` defaults to 50 and must be between 1 and 200; continue with the opaque decimal `before_id` returned as `next_cursor`. Without `key_id`, the endpoint-wide query returns every historical key — including keys since removed from the configuration — newest first; with `key_id`, only that key's rows for this endpoint are returned. History is retained for 30 days and pruned hourly in bounded batches. Active endpoints sample every 5 minutes and idle endpoints every 30 minutes using Valkey coordination; without Valkey, periodic sampling stays disabled while manual and request-triggered captures continue.

There is no backfill: providers without a fetcher (generic) never produce rows, and history begins when sampling starts. The table is created by the same deployment `db_init` migration procedure; production databases gain the empty table only through the operator's authorized migration run. Rows carry key labels, provider codes, plan metadata, and canonical windows only — never API keys, tokens, or raw upstream bodies.

## Interpreting observations

Window percentages preserve known zero separately from unknown usage. Window labels use the reported duration; no default five-hour or weekly window is inferred. The observation timestamp is the time the successful upstream response was received, not the time the dialog was opened. If a later refresh fails, the UI keeps the last successful values and marks them as cached/stale with a generic refresh warning.
