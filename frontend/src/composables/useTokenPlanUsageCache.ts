import { reactive } from 'vue'
import type { TokenPlanFetchState } from '@/models/endpoints/quota'
import type { TokenPlanUsageResponse } from '@/generated/admin-api'
import { fetchTokenPlanUsage } from '@/stores/endpoints-api'
import { createTokenPlanCacheExpiry } from './tokenPlanCacheExpiry'

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
  lastAttemptAt: number
  refreshFailed: boolean
  refreshing: boolean
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

// Expiry subscribers (foreground sweep). Owned here so the cache never
// imports the foreground module: the foreground subscribes while consumers
// are active, the cache only notifies.
const expiryListeners = new Set<(expiredIds: readonly string[]) => void>()

export function subscribeTokenPlanUsageExpiry(
  listener: (expiredIds: readonly string[]) => void,
): () => void {
  expiryListeners.add(listener)
  return () => {
    expiryListeners.delete(listener)
  }
}

const cacheExpiry = createTokenPlanCacheExpiry({
  onExpiry: (expiredIds) => {
    for (const listener of [...expiryListeners]) listener(expiredIds)
  },
})

// A cache read paired with its fetch state: stale snapshots remain visible as
// detail but cannot claim that their quota is current.
export type TokenPlanUsageSnapshot = {
  state: TokenPlanFetchState
  usage: TokenPlanUsageResponse | null
}

// Stale-while-revalidate read of the shared snapshot: the last payload remains
// available while refresh runs, and its fetch state prevents stale quota from
// being presented as a current observation.
export function getTokenPlanUsageSnapshot(
  endpointId: string,
): TokenPlanUsageSnapshot {
  cacheExpiry.readRevision()
  const entry = cache[endpointId]
  if (!entry) return { state: 'loading', usage: null }
  if (entry.usage === null) {
    return { state: entry.refreshing ? 'loading' : 'error', usage: null }
  }
  const stale =
    entry.refreshFailed ||
    Date.now() - entry.fetchedAt >= TOKEN_PLAN_CACHE_TTL_MS
  return { state: stale ? 'stale' : 'ready', usage: entry.usage }
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
  const checkedAt = entry.usage === null ? entry.lastAttemptAt : entry.fetchedAt
  return (
    (!entry.refreshFailed || entry.usage === null) &&
    Date.now() - checkedAt < TOKEN_PLAN_CACHE_TTL_MS
  )
}

// Shared fetch path: cold fetch, background refresh and a caller's deliberate
// refresh all use one in-flight table. Failed refreshes preserve the last good
// payload and its timestamp while recording a separate retry time; cold
// failures stay negative-cached for one TTL window.
function fetchTokenPlanUsageOnce(
  endpointId: string,
  force = false,
): Promise<TokenPlanFetchOutcome> {
  const flightKey = `${force ? 'force' : 'automatic'}:${endpointId}`
  const pending = inflight.get(flightKey)
  if (pending) return pending
  const existing = cache[endpointId]
  if (existing) {
    existing.refreshing = true
  } else {
    cache[endpointId] = {
      usage: null,
      fetchedAt: 0,
      lastAttemptAt: Date.now(),
      refreshFailed: false,
      refreshing: true,
    }
  }
  const promise = (async (): Promise<TokenPlanFetchOutcome> => {
    try {
      const usage = await fetchTokenPlanUsage(endpointId, force)
      const now = Date.now()
      cache[endpointId] = {
        usage,
        fetchedAt: now,
        lastAttemptAt: now,
        refreshFailed: false,
        refreshing: false,
      }
      cacheExpiry.schedule(endpointId, now + TOKEN_PLAN_CACHE_TTL_MS)
      return { ok: true, cause: null }
    } catch (cause) {
      const now = Date.now()
      const current = cache[endpointId]
      if (current && current.usage !== null) {
        current.lastAttemptAt = now
        current.refreshFailed = true
        // Notify-only single-shot retry at the existing retry TTL: the
        // foreground sweep re-gates through retryDue plus visible/active
        // checks, so no immediate loop or hidden-tab fetch results.
        cacheExpiry.schedule(endpointId, now + TOKEN_PLAN_CACHE_TTL_MS)
        return { ok: false, cause }
      }
      cache[endpointId] = {
        usage: null,
        fetchedAt: 0,
        lastAttemptAt: now,
        refreshFailed: true,
        refreshing: false,
      }
      cacheExpiry.schedule(endpointId, now + TOKEN_PLAN_CACHE_TTL_MS)
      return { ok: false, cause }
    } finally {
      if (cache[endpointId]) cache[endpointId]!.refreshing = false
      inflight.delete(flightKey)
    }
  })()
  inflight.set(flightKey, promise)
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
      await fetchTokenPlanUsageOnce(endpointId, true),
    )
  }
  const entry = cache[endpointId]
  if (!entry) {
    return resolvedRequest(
      endpointId,
      await fetchTokenPlanUsageOnce(endpointId),
    )
  }
  if (entry.refreshing) {
    if (entry.usage === null) {
      const pending =
        inflight.get(`automatic:${endpointId}`) ??
        inflight.get(`force:${endpointId}`)
      if (pending) return resolvedRequest(endpointId, await pending)
    }
    return { ...warmRequest(endpointId), requested: true }
  }
  if (isTokenPlanUsageFresh(endpointId)) return warmRequest(endpointId)
  const retryDue = Date.now() - entry.lastAttemptAt >= TOKEN_PLAN_CACHE_TTL_MS
  if (!retryDue) return warmRequest(endpointId)
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
//
// Stale entries serve immediately (SWR) and revalidate in the background:
// after the SWR call each worker joins the actual automatic inflight for
// that id so the pool of 4 bounds real network concurrency as well as
// cold fetches. Force flights are never joined, preserving automatic/force
// isolation.
//
// The optional per-id eligibility predicate is evaluated immediately before
// starting each id (default always eligible, preserving existing callers).
// A foreground sweep passes a live predicate (visible plus still
// registered) so remaining queue entries are skipped once the last consumer
// unregisters or the tab hides; already-started requests still finish.
export async function prefetchTokenPlanBatch(
  endpointIds: Iterable<string>,
  concurrency = 4,
  isEligible: (endpointId: string) => boolean = () => true,
): Promise<void> {
  const ids = Array.from(new Set(endpointIds)).filter(Boolean)
  if (ids.length === 0) return
  const limit = Math.max(1, concurrency)
  let index = 0
  const workers = Array.from({ length: limit }, async () => {
    while (index < ids.length) {
      const id = ids[index++]
      if (id === undefined) break
      if (!isEligible(id)) continue
      await prefetchTokenPlanUsage(id)
      // Join the automatic flight this SWR kicked off (cold reads already
      // settled inside the prefetch above; the lookup is then absent).
      // Never join `force:` — a deliberate refresh stays isolated.
      const flight = inflight.get(`automatic:${id}`)
      if (flight) await flight.catch(() => undefined)
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
  cacheExpiry.clear()
}
