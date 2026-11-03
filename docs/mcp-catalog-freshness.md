# MCP catalog freshness

The worker keeps one MCP catalog snapshot per configured MCP server: the tools, resources, resource templates, and prompts that server declared the last time it was contacted. This document describes how long a snapshot may be served, how the refresh of one server is owned, and what the cache does not do.

## Two clocks on one snapshot

Every stored snapshot carries two independent timestamps:

- The **configuration generation** of the server it was fetched for. A configuration change (URL, transport, credentials, filters) produces a new generation, and a snapshot of an older generation is never served.
- The **acquisition timestamp**: the moment the snapshot was fetched from the upstream. This is the only clock that decides whether a snapshot may still be served.

The two are never conflated. Reading a snapshot does not move the acquisition timestamp, so an old snapshot cannot become fresh by being read. A payload written before the acquisition timestamp existed has none, and such a payload is treated as expired rather than fresh.

## Freshness states and defaults

| State | Condition | Behavior |
| --- | --- | --- |
| `missing` | no snapshot exists for the current configuration generation | the read performs a bounded refresh |
| `fresh` | younger than 5 minutes | served without any upstream call |
| `stale` | between 5 and 30 minutes | served while exactly one refresh runs |
| `expired` | older than 30 minutes | never served; the read must recover the catalog first |

The soft freshness window (5 minutes) and the maximum serving age (30 minutes) are production defaults. The local, Valkey, and SQLite backends enforce the same policy, so a snapshot has the same lifetime no matter which backend stores it.

Storage retention is independent of freshness. A stored snapshot outlives the moment it may be served: the Valkey entry expires with the configured Valkey retention window, and the SQLite entry with the same window in the standalone coordinator store. Retention decides when the bytes are dropped; freshness decides when the content may be served.

## One coordinator per cache

The bounded refresh state belongs to the catalog cache, not to an individual service instance. Every reader, the manual admin refresh, the startup bulk load, and the periodic pass that share one runtime cache therefore share one refresh coordinator:

- One owner permit bounds the actual refresh work, not only the futures of the outer reader loop. A bulk read over many stale catalogs can never exceed the configured refresh concurrency, and two service wrappers over the same cache share the same bound.
- Background refreshes are coalesced by server and configuration generation, so repeated stale reads of one server queue one refresh instead of one per read. The bound counts all outstanding work, including superseded generations, so configuration churn cannot accumulate an unbounded waiting queue; a burst that exceeds the bound is retried by the next pass.
- Every public read and manual refresh is itself a tracked operation, not only the spawned refresh tasks. Each one takes one absolute deadline and one activity before its first cache or repository call, and the deadline covers the lookup, the owner permit, the lease, the repository checks, the upstream fetch and the publication. A follower observes the same deadline and the same cancellation signal.
- Cancellation is a property of the coordinator, and it is observed inside an in-progress cache, repository or backend wait, not only between polling iterations. The managed worker stops scheduling first and then drains every tracked activity within a bounded timeout, so a shutdown cancels cold followers, read-triggered refreshes, bulk warmup and the periodic pass. A cancelled owner releases its lease explicitly inside a reserved release margin; lease expiry is only the recovery path.
- Lease cleanup stays accounted for even when the caller is aborted. The tracked operation guard is reference counted and a clone moves into the lease, so a caller future dropped mid-fetch or mid-release keeps its activity visible to the drain until the bounded cleanup finishes. A cooperative release keeps the ownership token until the backend confirms it, so a cancelled release transfers cleanup instead of silently consuming the token. A cleanup that exceeds its margin logs that the lease is recovered only by its expiry rather than claiming a deterministic release.

## One refresh owner per server

For a given server:

- Exactly one worker refreshes it at a time. Valkey owns that decision with `SET NX EX` and releases the lease with a token-safe compare-and-delete, so a lease that already expired and was taken over by another worker is left alone. The standalone SQLite backend reuses the coordinator leases and their owner token. The in-memory backend has no shared coordination and falls back to an in-process single-flight registry whose slots also carry an owner token, which deduplicates only inside one process.
- The lease lifetime always covers the whole acquisition (permit wait excluded) plus a safety margin, so a lease cannot expire while its refresh is still running. A cancelled refresh releases the lease explicitly; if that release cannot run, the bounded lease expiry recovers it.
- After taking the lease, the worker re-reads the stored snapshot before spending an upstream round trip, so two reads never fetch the same server twice.
- Before a snapshot is published, the configuration generation is rechecked and the publication is committed with a backend compare-and-swap against the exact envelope the refresh captured before its fetch. A snapshot of a superseded configuration is not published, and an invalidated entry is not resurrected by a fetch that was already running.
- Invalidation is a shared, atomic fence. SQLite reuses the coordinator `get_or_insert`/`replace_if_current` primitives; Valkey uses a payload-safe Lua compare-and-swap with the same retention policy. An invalidated entry is represented by a revisioned tombstone rather than a deleted key, so a publisher that captured the previous envelope fails its compare and leaves the shared store untouched. Readers treat a tombstone as missing and drop their local copy, so a peer cannot serve invalidated data. The local backend uses the equivalent revision compare under its own lock.
- A shared backend that cannot be read fails closed. The cache does not fall back to its in-process copy when the Valkey or SQLite authority is unavailable, because that copy's current revision is unknown: a peer may already have invalidated it. Such a read reports the catalog as missing and recovery then fails at the shared baseline, so the affected request gets an actionable "catalog is not ready" error instead of a possibly invalidated schema. This is a deliberate availability tradeoff: the local-only backend keeps serving from memory, while a shared backend with an unreachable authority prefers an error to stale data.
- A read that finds no usable snapshot waits for the refresh another worker owns and then either serves the published result or fails with an actionable error. An expired snapshot is never served as a fallback.
- Every publication carries a publication identity that is regenerated even when the content is unchanged. A forced refresh that follows another owner only succeeds once it observes a publication newer than the one it saw before attempting ownership; it never reports the pre-refresh snapshot as a refresh result. The content fingerprint stays independent of that identity and of the acquisition timestamp.

## Startup and the periodic pass

The `mcp_warmup` setting keeps its three modes:

- `all` (default) loads every enabled catalog at startup.
- `lazy` and `off` skip the bulk cold start. Their catalogs are loaded by the read that needs them.

Independently of the mode, a managed periodic pass runs every 60 seconds with a random jitter of up to 10 percent. It refreshes the catalogs that already exist and are no longer fresh, and it never cold-loads a catalog that was never fetched. The pass runs in the connected and the standalone worker, is cancelled on shutdown, and cancels an in-flight upstream fetch instead of waiting for it. The standalone request path, the admin state, and this managed task share one cache and therefore one coordinator.

## Failure handling

A failed refresh keeps the previous snapshot for diagnostics but does not extend its acquisition timestamp, so a failed refresh cannot make an old snapshot servable. A bounded backoff suppresses the next background attempt for the same server configuration generation, so a configuration change is never held back by a failure recorded for the superseded one. The admin catalog endpoint reports `mcp catalog is not ready` with a service-unavailable status when a catalog cannot be recovered; the MCP proxy reports the same condition to the client.

## Limitations

- The cache refreshes what the worker serves. It cannot update the schemas a client already holds: a client that cached an output schema keeps using it until it runs `tools/list` again. No notification is sent to clients about a changed schema.
- A refresh never replays a tool call, never widens a schema, and never mutates a result. A call that was accepted against the previous schema is not retried automatically.
- The catalog is a declaration read from the upstream. The worker does not validate the schema against the result a tool later returns.
- The publication fence detects a configuration row change through `updated_at` and a concurrent publication or invalidation through the shared envelope. An in-place change that keeps the same `updated_at` and leaves the catalog envelope untouched is not detectable by the fence.

## Deferred follow-ups

The following are explicitly out of scope here and remain open as separate work: full JSON Schema validation, automatic schema widening, result mutation, output-triggered refresh, retaining an upstream notification connection, and a downstream subscription or notification registry.
