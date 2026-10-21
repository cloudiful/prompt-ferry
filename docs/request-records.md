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
