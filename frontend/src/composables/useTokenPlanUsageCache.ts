import { reactive } from 'vue'
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
const inflight = new Map<string, Promise<TokenPlanUsageResponse | null>>()

// Stale-while-revalidate read. Returns the last snapshot regardless of
// freshness so the table keeps rendering the previous numbers while the
// background refresh resolves. Callers that need to know whether to
// trigger the revalidate themselves should pair this with
// `isTokenPlanUsageFresh` (e.g. `useTokenPlanBadges` kicks the prefetch
// on every setup, which is the path that drives revalidation).
export function getCachedTokenPlanUsage(
  endpointId: string,
): TokenPlanUsageResponse | null {
  const entry = cache[endpointId]
  if (!entry) return null
  return entry.usage
}

export function isTokenPlanUsageFresh(endpointId: string): boolean {
  const entry = cache[endpointId]
  if (!entry) return false
  return Date.now() - entry.fetchedAt < TOKEN_PLAN_CACHE_TTL_MS
}

// Shared fetch path: cold fetch and background refresh both go through
// here so the in-flight dedup and the negative-cache handling live in
// exactly one place. On success we always overwrite the snapshot; on
// failure we keep whatever snapshot was already cached and only bump
// `fetchedAt` so the SWR layer backs off rather than retrying
// immediately. A cold fetch (no prior snapshot) falls through to the
// negative-cache write so we don't storm the upstream on a dead key.
function refreshTokenPlanUsage(
  endpointId: string,
): Promise<TokenPlanUsageResponse | null> {
  const pending = inflight.get(endpointId)
  if (pending) return pending
  const promise = (async () => {
    try {
      const usage = await fetchTokenPlanUsage(endpointId)
      cache[endpointId] = { usage, fetchedAt: Date.now() }
      return usage
    } catch {
      const existing = cache[endpointId]
      if (existing && existing.usage !== null) {
        // Refresh failed but we still have a usable snapshot — keep it
        // and refresh the timestamp so the SWR layer treats the entry
        // as fresh for one TTL window (back-off). The table never
        // blanks out for one network RTT while the upstream recovers.
        existing.fetchedAt = Date.now()
        return existing.usage
      }
      // Cold fetch failed (no prior snapshot): pin a negative entry so
      // a follow-up within the TTL window skips the network.
      cache[endpointId] = { usage: null, fetchedAt: Date.now() }
      return null
    } finally {
      inflight.delete(endpointId)
    }
  })()
  inflight.set(endpointId, promise)
  return promise
}

export async function prefetchTokenPlanUsage(
  endpointId: string,
): Promise<TokenPlanUsageResponse | null> {
  if (!endpointId) return null
  const entry = cache[endpointId]
  // Cold fetch: first observation pays the network cost.
  if (!entry) return refreshTokenPlanUsage(endpointId)
  // Fresh hit: avoid the network round-trip.
  if (isTokenPlanUsageFresh(endpointId)) return entry.usage
  // Stale entry: stale-while-revalidate — return the snapshot
  // immediately and kick a background refresh so the next observation
  // sees the fresh payload without forcing the caller to await it.
  void refreshTokenPlanUsage(endpointId)
  return entry.usage
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
