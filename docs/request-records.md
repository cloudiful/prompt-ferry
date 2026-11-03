# Request records

The admin console lists the recorded requests and summarizes them in an overview. The AI category and the MCP category are separate datasets, and the MCP dataset is split further by a scope that decides which MCP records a query covers.

## MCP scope

The scope splits the MCP records by record identity, never by request path or by a display label:

- `service` keeps the records that reached an MCP service. A record belongs to a service when it carries an MCP server id or a stored server name, so a name-only failed attempt and the historical id-only records of a deleted server stay on the service side.
- `gateway` keeps the gateway chatter: the records that carry neither a server id nor a server name.
- `all` keeps both.

The scope applies to the MCP category only; the AI records, their counts, their facets, and their overview never change with it. The console selects the `service` scope when the MCP category is entered or reset, and the scope is query-level state: it is separate from the row filters, and resetting the row filters never changes it. Changing the scope resets the page and drops the filters that cannot match the new scope, so a service filter never hides the gateway records. The selected scope applies consistently to the records, their count, their facets, the overview metrics, trend, and breakdown, and the overview cache keeps one entry per scope. The scope selector is available in the records view and in the overview view, and both act on the same selection.

The overview response carries the trend granularity the server selected (`hour` or `day`), so the client labels the buckets without re-deriving the server's window logic. The day buckets are logical UTC calendar dates and stay on that date regardless of the database session time zone.

## Display time zone

The admin console renders every timestamp in a display time zone. The preference lives in the general settings and is browser-local, like the language and the theme: `auto` follows the zone the browser reports, and a manual IANA zone overrides it. `UTC` and `Asia/Shanghai` are always selectable, and the full zone list is searchable when the runtime can enumerate it.

The preference changes labels only. It never changes an instant, never changes what the server stores, and never changes a reporting period or a date range: the statistical day buckets keep their logical UTC calendar date, while the hour buckets follow the chosen zone. The preference is persisted in the browser's local storage; when the browser blocks local storage the selection still applies in the current session, but it is not remembered after a reload.

The record date and the record time of a row always describe the same day in the chosen zone, and both follow the preference immediately, without refetching the records. The overview trend uses the granularity the server selected: a day bucket is labelled with its logical UTC calendar date, and an hour bucket is an instant in the chosen zone.

## API

The records, facets, and overview endpoints accept an optional `mcp_scope` parameter with the values `service`, `gateway`, and `all`:

```text
GET /api/v1/admin/request-records?request_category=mcp&mcp_scope=service
GET /api/v1/admin/request-records/facets?request_category=mcp&mcp_scope=service
GET /api/v1/admin/request-records/overview?request_category=mcp&mcp_scope=service
```

An omitted `mcp_scope` means `all`, which keeps the historical behaviour of an MCP query. An invalid value is rejected. The parameter is additive: an existing client keeps working unchanged.

The MCP distribution opens the records it describes. Every distribution row carries the scope its records were grouped by, so the drilldown never guesses from a display label or from a missing server id: a gateway row opens the gateway scope, a service row with a server id opens that server, and a name-only service row opens the service scope filtered by its stored server name.

## MCP tool calls: the selected key and the actual outcome

An MCP tool call runs inside the session task that serves the request, so the
record is settled from a tracker that the request, that task and the
settlement share. The record therefore carries the endpoint key that served
the call and the one-based MCP token slot, and its outcome follows the tool
call rather than the HTTP status alone:

- a tool call that failed inside a successful HTTP response is recorded as a
  failed request, with a stable internal error code (`mcp_tool_error`,
  `mcp_upstream_error`, `mcp_call_failed` or `mcp_quota_exhausted`) and a
  fixed diagnostic;
- a completed tool call is recorded as completed;
- a call that succeeded on another key after a quota rejection is recorded as
  completed, and the record carries the key that served it, not the key that
  failed first.

The recorded diagnostic is built from the failure kind only: a fixed sentence
plus a bounded numeric JSON-RPC, HTTP or provider code. The provider's error
text, a transport exception, the query and an image payload never reach the
record, whatever the content logging mode is, so an upstream that echoes a
credential or the request input cannot leak it into request metadata. The MCP
client receives that same fixed diagnostic, so the protocol error body never
carries the provider message, a transport exception, the query, the image
source or any configured credential either.

The snapshot is taken at the actual settlement, after the response stream has
been consumed, so a tool outcome that happens after the response headers still
reaches the record.

A later success never hides an error that already happened in the same
request. A JSON-RPC batch that carries several tool calls is aggregated the
same way: the first failure wins, and a later success does not clear it.

The tracker observes tool calls. A request that never reaches one — a
non-tool method, or a request the MCP protocol rejects before the handler
sees it — records no selected key and no tool outcome, and keeps the outcome
its transport status describes. The token slot is the tracker's slot at settlement, so the record keeps the
slot of the key that served the settled attempt, and a call that never
selected a key records no slot instead of inheriting one captured with the
response. The tracker belongs to one request, so two sessions that use the
same JSON-RPC id never see each other's key or outcome.
