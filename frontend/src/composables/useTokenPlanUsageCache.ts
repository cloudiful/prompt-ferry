import { reactive } from 'vue'
import type { TokenPlanFetchState } from '@/models/endpoints/quota'
import type { TokenPlanUsageResponse } from '@/generated/admin-api'
import { fetchTokenPlanUsage } from '@/stores/endpoints-api'

// 60s aligns with backend `TokenPlanQuotaCache::REFRESH_AFTER` so the
// dialog and the inline badges never race past the upstream TTL. After
// the entry crosses the TTL boundary the cache keeps serving the last
// snapshot while a background refresh lands (stale-while-revalidate),
// so the table never blanks out for the ~1 network round-trip between
// expiry and the next observation.
export const TOKEN_PLAN_CACHE_TTL_MS = 60_000

type CachedUsage = {
  usage: TokenPlanUsageResponse | null
  fetchedAt: number
}

// Result of one awaited fetch. The payload is deliberately absent: the caller
// reads it back from the cache, so a failed refresh of a usable snapshot still
// resolves to the snapshot the surfaces should keep rendering. The cause rides
// along so a consumer that awaits the read can report the failure on its own
// channel instead of the cache having to throw.
type TokenPlanFetchOutcome = {
  ok: boolean
  cause: unknown
}

// Module-level singleton: every table card and the usage dialog share
// the same in-memory snapshot so re-opening the dialog after the badges
// have already fetched hits cache instead of refetching. `reactive`
// wraps the plain record so Vue templates that read `cache[id]` re-render
// when the prefetch resolves.
const cache = reactive<Record<string, CachedUsage>>({})

// In-flight promise table. When two consumers ask for the same
// endpoint within one tick (e.g. table badges + dialog open), both share
// the same pending promise — no duplicate `/api/v1/endpoints/{id}/usage`
// request.
const inflight = new Map<string, Promise<TokenPlanFetchOutcome>>()

// A cache read paired with the fetch state it represents. Surfaces that render
// a badge need both: `usage` alone reports `null` for a cold cache *and* for a
// failed cold fetch, which is how a fetch error used to render like a provider
// that reports nothing.
export type TokenPlanUsageSnapshot = {
  state: TokenPlanFetchState
  usage: TokenPlanUsageResponse | null
}

// Stale-while-revalidate read of the shared snapshot: the last payload is
// returned regardless of freshness, so the table keeps rendering the previous
// numbers while the background refresh resolves, and the fetch state says
// which of the three situations the entry represents.
export function getTokenPlanUsageSnapshot(
  endpointId: string,
): TokenPlanUsageSnapshot {
  const entry = cache[endpointId]
  if (!entry) return { state: 'loading', usage: null }
  // A stale entry is still `ready`: stale-while-revalidate keeps serving the
  // last payload, and a failed refresh of a usable snapshot keeps it too.
  return { state: entry.usage === null ? 'error' : 'ready', usage: entry.usage }
}

// Payload-only view of the same read, for the consumers that render numbers
// and have no state to show. Callers that need to know whether to trigger the
// revalidate themselves should pair this with `isTokenPlanUsageFresh` (e.g.
// `useTokenPlanBadges` kicks the prefetch on every setup, which is the path
// that drives revalidation).
export function getCachedTokenPlanUsage(
  endpointId: string,
): TokenPlanUsageResponse | null {
  return getTokenPlanUsageSnapshot(endpointId).usage
}

export function isTokenPlanUsageFresh(endpointId: string): boolean {
  const entry = cache[endpointId]
  if (!entry) return false
  return Date.now() - entry.fetchedAt < TOKEN_PLAN_CACHE_TTL_MS
}

// Shared fetch path: cold fetch, background refresh and a caller's deliberate
// refresh all go through here so the in-flight dedup and the negative-cache
// handling live in exactly one place. On success we always overwrite the
// snapshot; on failure we keep whatever snapshot was already cached and only
// bump `fetchedAt` so the SWR layer backs off rather than retrying
// immediately. A cold fetch (no prior snapshot) falls through to the
// negative-cache write so we don't storm the upstream on a dead key. The
// rejection cause rides along so a caller that awaits the read can report the
// failure without the cache having to throw.
function fetchTokenPlanUsageOnce(
  endpointId: string,
): Promise<TokenPlanFetchOutcome> {
  const pending = inflight.get(endpointId)
  if (pending) return pending
  const promise = (async (): Promise<TokenPlanFetchOutcome> => {
    try {
      const usage = await fetchTokenPlanUsage(endpointId)
      cache[endpointId] = { usage, fetchedAt: Date.now() }
      return { ok: true, cause: null }
    } catch (cause) {
      const existing = cache[endpointId]
      if (existing && existing.usage !== null) {
        // Refresh failed but we still have a usable snapshot — keep it
        // and refresh the timestamp so the SWR layer treats the entry
        // as fresh for one TTL window (back-off). The table never
        // blanks out for one network RTT while the upstream recovers.
        existing.fetchedAt = Date.now()
        return { ok: false, cause }
      }
      // Cold fetch failed (no prior snapshot): pin a negative entry so
      // a follow-up within the TTL window skips the network.
      cache[endpointId] = { usage: null, fetchedAt: Date.now() }
      return { ok: false, cause }
    } finally {
      inflight.delete(endpointId)
    }
  })()
  inflight.set(endpointId, promise)
  return promise
}

// One cache read for consumers that need the whole request policy — currently
// the token-plan usage dialog, which must not issue a second request for a
// snapshot the endpoint badges already fetched.
export type TokenPlanUsageRequest = {
  // Fetch state of the snapshot this read resolved to.
  state: TokenPlanFetchState
  usage: TokenPlanUsageResponse | null
  // `true` when the read reached the network: a cold entry, a forced refresh,
  // or the background revalidate a stale entry kicked off. A fresh hit reports
  // `false`, which is how a caller proves it added no duplicate request.
  requested: boolean
  // `true` only when a fetch this read *awaited* failed. A warm read, or a
  // pinned negative entry, reports `false` so one upstream failure is not
  // re-reported on every subsequent open.
  failed: boolean
  // The rejection cause behind `failed`, for the caller's error channel.
  cause: unknown
}

function resolvedRequest(
  endpointId: string,
  outcome: TokenPlanFetchOutcome,
): TokenPlanUsageRequest {
  const snapshot = getTokenPlanUsageSnapshot(endpointId)
  return {
    ...snapshot,
    requested: true,
    failed: !outcome.ok,
    cause: outcome.cause,
  }
}

function warmRequest(endpointId: string): TokenPlanUsageRequest {
  return {
    ...getTokenPlanUsageSnapshot(endpointId),
    requested: false,
    failed: false,
    cause: null,
  }
}

// Policy entry point shared by the badges (`prefetchTokenPlanUsage` below) and
// the usage dialog. The cache remains the only owner of TTL, stale-while-
// revalidate, negative caching, back-off and in-flight coalescing: a warm read
// never touches the network, a stale read serves the snapshot and revalidates
// in the background, and only `refresh: true` forces a read past the TTL.
export async function requestTokenPlanUsage(
  endpointId: string,
  options: { refresh?: boolean } = {},
): Promise<TokenPlanUsageRequest> {
  if (!endpointId) {
    // No id means no entry to read, which is exactly the warm shape of a cold
    // cache: no request, nothing to report as failed.
    return warmRequest(endpointId)
  }
  // Deliberate refresh: the caller wants the network read even inside the TTL
  // window, and a usable snapshot stays visible until it lands.
  if (options.refresh) {
    return resolvedRequest(
      endpointId,
      await fetchTokenPlanUsageOnce(endpointId),
    )
  }
  const entry = cache[endpointId]
  // Cold entry: first observation pays the network cost.
  if (!entry) {
    return resolvedRequest(
      endpointId,
      await fetchTokenPlanUsageOnce(endpointId),
    )
  }
  // Fresh hit: avoid the network round-trip. A pinned negative entry is fresh
  // too — its failure was already reported when it happened.
  if (isTokenPlanUsageFresh(endpointId)) return warmRequest(endpointId)
  // Stale entry: stale-while-revalidate — return the snapshot immediately and
  // kick a background refresh so the next observation sees the fresh payload
  // without forcing the caller to await it.
  void fetchTokenPlanUsageOnce(endpointId)
  return { ...warmRequest(endpointId), requested: true }
}

export async function prefetchTokenPlanUsage(
  endpointId: string,
): Promise<TokenPlanUsageResponse | null> {
  return (await requestTokenPlanUsage(endpointId)).usage
}

// Bounded-concurrency batch prefetcher. Worker-pool pattern: a shared
// cursor fans out across N workers, each awaiting one fetch before
// claiming the next id. At most `concurrency` requests are in flight at
// any instant, even when the table swaps pages mid-fetch.
//
// `concurrency=4` mirrors the spec's "并发限4"; input ids are
// de-duplicated so re-renders that re-emit the same list don't double up.
export async function prefetchTokenPlanBatch(
  endpointIds: Iterable<string>,
  concurrency = 4,
): Promise<void> {
  const ids = Array.from(new Set(endpointIds)).filter(Boolean)
  if (ids.length === 0) return
  const limit = Math.max(1, concurrency)
  let index = 0
  const workers = Array.from({ length: limit }, async () => {
    while (index < ids.length) {
      const id = ids[index++]
      if (id === undefined) break
      await prefetchTokenPlanUsage(id)
    }
  })
  await Promise.all(workers)
}

// Test-only escape hatch: the unit tests in `tests/` need to start from
// a clean state without reloading the module. Production code paths
// never call this.
export function __resetTokenPlanCacheForTests(): void {
  for (const key of Object.keys(cache)) delete cache[key]
  inflight.clear()
}
