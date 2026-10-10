# ChatGPT quota snapshots

The admin console keeps a bounded history of successful OpenAI ChatGPT subscription quota observations. The quota dialog shows the observation time and whether the displayed values are cached, stale, refreshing, or being shown after a failed refresh. A failed refresh does not replace the last successful observation.

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

## Interpreting observations

Window percentages preserve known zero separately from unknown usage. Window labels use the reported duration; no default five-hour or weekly window is inferred. The observation timestamp is the time the successful upstream response was received, not the time the dialog was opened. If a later refresh fails, the UI keeps the last successful values and marks them as cached/stale with a generic refresh warning.
