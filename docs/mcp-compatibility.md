# MCP protocol compatibility

Prompt Ferry exposes one aggregated MCP endpoint plus per-server named
endpoints. A client talks to Prompt Ferry with a single protocol; it never
configures a protocol per upstream server.

## Supported protocol versions

| Client lifecycle | Protocol version |
| --- | --- |
| Modern `server/discover` | `2026-07-28` |
| Legacy `initialize` | `2025-11-25` |

The version is negotiated per client session. The aggregate endpoint and every
named endpoint advertise the same supported versions, and the version a client
negotiates does not depend on which upstream a request is routed to.

Upstream servers are negotiated independently by the existing lifecycle
fallback: a modern `server/discover` probe first, then the legacy `initialize`
handshake for servers that cannot speak the probe. One upstream negotiating an
older version never downgrades the client-facing endpoint.

## Result compatibility

For `tools/call`, `resources/read`, `prompts/get`, and `completion/complete`,
ordinary completed results from a built-in or older upstream are returned to a
`2026-07-28` client with `resultType: "complete"`. This includes the built-in
MiniMax bridge, whose native result carries only `content`.

A legacy client receives the wire shape of `2025-11-25`: `resultType` is not
added, and `resources/read` does not expose the newer `ttlMs`/`cacheScope`
cache metadata even when an upstream supplies it.

`resources/read` results for a modern client always carry `ttlMs` and
`cacheScope`; values supplied by an upstream are preserved.

## Multi-round tool requests (MRTR)

`input_required` results are forwarded with their real `resultType`,
`requestState`, and `inputRequests` when both the client and the upstream
negotiated `2026-07-28`. A follow-up request's `inputResponses` and
`requestState` reach that upstream unchanged.

A client on `2025-11-25` cannot express `input_required`, so such a result is
reported as a protocol error instead of a fabricated success.

A continuation cannot be expressed to an upstream that negotiated a version
older than `2026-07-28`. Prompt Ferry rejects that request with an explicit
error instead of dropping the continuation or pretending the retry completed.

## Not supported

The endpoint advertises only tools, resources, prompts, and completions. It does
not implement or claim:

- the `2026-07-28` tasks extension;
- progress notifications, subscriptions, or logging;
- MRTR rounds that require state kept only inside an upstream session.

A result that cannot be mapped losslessly is returned as an explicit JSON-RPC
error rather than silently discarded. An upstream JSON-RPC error keeps its code,
message, and data, associated with the downstream request id. Transport, parse,
and internal failures keep the internal-error envelope and are never reported as
an upstream error.
