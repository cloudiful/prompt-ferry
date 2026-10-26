import type { TokenPlanUsageResponse } from '@/generated/admin-api'
import type { TokenPlanFetchState } from '@/models/endpoints/quota'
import { fetchTokenPlanUsage } from '@/stores/endpoints-api'

export const TOKEN_PLAN_CACHE_TTL_MS = 60_000

export type TokenPlanUsageSnapshot = {
  state: TokenPlanFetchState
  usage: TokenPlanUsageResponse | null
}

export type TokenPlanUsageRequest = TokenPlanUsageSnapshot & {
  requested: boolean
  failed: boolean
  cause: unknown
}

type CachedUsage = {
  usage: TokenPlanUsageResponse | null
  fetchedAt: number
  lastAttemptAt: number
  refreshFailed: boolean
  refreshing: boolean
}

type CachedUsageEntry = CachedUsage & {
  loadingSnapshot: TokenPlanUsageSnapshot
  errorSnapshot: TokenPlanUsageSnapshot
  readySnapshot: TokenPlanUsageSnapshot | null
  staleSnapshot: TokenPlanUsageSnapshot | null
}

type TokenPlanFetchOutcome = {
  ok: boolean
  cause: unknown
}

type Listener = () => void

type ExpiryListener = (expiredIds: readonly string[]) => void

const EMPTY_SNAPSHOT: TokenPlanUsageSnapshot = Object.freeze({
  state: 'loading',
  usage: null,
})
const cache = new Map<string, CachedUsageEntry>()
const inflight = new Map<string, Promise<TokenPlanFetchOutcome>>()
const listenersByEndpoint = new Map<string, Set<Listener>>()
const revisionListeners = new Set<Listener>()
const expiryListeners = new Set<ExpiryListener>()
const expiryDeadlines = new Map<string, number>()
let revision = 0
let expiryTimer: ReturnType<typeof setTimeout> | null = null
let expiryGeneration = 0

function immutableSnapshot(
  state: TokenPlanFetchState,
  usage: TokenPlanUsageResponse | null,
): TokenPlanUsageSnapshot {
  return Object.freeze({ state, usage })
}

function makeEntry(value: CachedUsage): CachedUsageEntry {
  const usage = value.usage
  return {
    ...value,
    loadingSnapshot: EMPTY_SNAPSHOT,
    errorSnapshot: immutableSnapshot('error', null),
    readySnapshot: usage === null ? null : immutableSnapshot('ready', usage),
    staleSnapshot: usage === null ? null : immutableSnapshot('stale', usage),
  }
}

function notify(endpointIds: readonly string[]): void {
  if (endpointIds.length === 0) return
  revision += 1
  for (const listener of [...revisionListeners]) listener()
  for (const endpointId of endpointIds) {
    for (const listener of [...(listenersByEndpoint.get(endpointId) ?? [])]) {
      listener()
    }
  }
}

export function getTokenPlanUsageRevision(): number {
  return revision
}

export function subscribeTokenPlanUsageRevision(
  listener: Listener,
): () => void {
  revisionListeners.add(listener)
  return () => revisionListeners.delete(listener)
}

export function subscribeTokenPlanUsage(
  endpointId: string,
  listener: Listener,
): () => void {
  if (!endpointId) return () => undefined
  let listeners = listenersByEndpoint.get(endpointId)
  if (!listeners) {
    listeners = new Set()
    listenersByEndpoint.set(endpointId, listeners)
  }
  listeners.add(listener)
  return () => {
    listeners?.delete(listener)
    if (listeners?.size === 0) listenersByEndpoint.delete(endpointId)
  }
}

export function subscribeTokenPlanUsageExpiry(
  listener: ExpiryListener,
): () => void {
  expiryListeners.add(listener)
  return () => expiryListeners.delete(listener)
}

function cancelExpiryTimer(): void {
  expiryGeneration += 1
  if (expiryTimer !== null) clearTimeout(expiryTimer)
  expiryTimer = null
}

function scheduleNextExpiry(): void {
  cancelExpiryTimer()
  if (typeof window === 'undefined' || expiryDeadlines.size === 0) return

  let deadline = Number.POSITIVE_INFINITY
  for (const value of expiryDeadlines.values()) {
    deadline = Math.min(deadline, value)
  }

  const scheduledGeneration = expiryGeneration
  const timer = setTimeout(
    () => {
      if (scheduledGeneration !== expiryGeneration) return
      expiryTimer = null
      const now = Date.now()
      const expiredIds: string[] = []
      for (const [endpointId, expiresAt] of expiryDeadlines) {
        if (expiresAt <= now) {
          expiryDeadlines.delete(endpointId)
          expiredIds.push(endpointId)
        }
      }
      scheduleNextExpiry()
      if (expiredIds.length > 0) {
        notify(expiredIds)
        for (const listener of [...expiryListeners]) listener(expiredIds)
      }
    },
    Math.max(0, deadline - Date.now()),
  )
  expiryTimer = timer
  ;(timer as ReturnType<typeof setTimeout> & { unref?: () => void }).unref?.()
}

function scheduleExpiry(endpointId: string, deadline: number): void {
  expiryDeadlines.set(endpointId, deadline)
  scheduleNextExpiry()
}

function hasFlight(endpointId: string): boolean {
  return (
    inflight.has(`automatic:${endpointId}`) ||
    inflight.has(`force:${endpointId}`)
  )
}

function setEntry(endpointId: string, value: CachedUsage): CachedUsageEntry {
  const entry = makeEntry(value)
  cache.set(endpointId, entry)
  return entry
}

function getSnapshotFromEntry(entry: CachedUsageEntry): TokenPlanUsageSnapshot {
  if (entry.usage === null) {
    return entry.refreshing ? entry.loadingSnapshot : entry.errorSnapshot
  }
  const stale =
    entry.refreshFailed ||
    Date.now() - entry.fetchedAt >= TOKEN_PLAN_CACHE_TTL_MS
  return stale ? entry.staleSnapshot! : entry.readySnapshot!
}

export function getTokenPlanUsageSnapshot(
  endpointId: string,
): TokenPlanUsageSnapshot {
  if (!endpointId) return EMPTY_SNAPSHOT
  const entry = cache.get(endpointId)
  return entry ? getSnapshotFromEntry(entry) : EMPTY_SNAPSHOT
}

export function getCachedTokenPlanUsage(
  endpointId: string,
): TokenPlanUsageResponse | null {
  return getTokenPlanUsageSnapshot(endpointId).usage
}

export function isTokenPlanUsageFresh(endpointId: string): boolean {
  const entry = cache.get(endpointId)
  if (!entry) return false
  const checkedAt = entry.usage === null ? entry.lastAttemptAt : entry.fetchedAt
  return (
    (!entry.refreshFailed || entry.usage === null) &&
    Date.now() - checkedAt < TOKEN_PLAN_CACHE_TTL_MS
  )
}

function fetchTokenPlanUsageOnce(
  endpointId: string,
  force = false,
): Promise<TokenPlanFetchOutcome> {
  const flightKey = `${force ? 'force' : 'automatic'}:${endpointId}`
  const existingFlight = inflight.get(flightKey)
  if (existingFlight) return existingFlight

  const existing = cache.get(endpointId)
  if (existing) {
    cache.set(endpointId, { ...existing, refreshing: true })
  } else {
    setEntry(endpointId, {
      usage: null,
      fetchedAt: 0,
      lastAttemptAt: Date.now(),
      refreshFailed: false,
      refreshing: true,
    })
  }

  const request = (async (): Promise<TokenPlanFetchOutcome> => {
    try {
      const usage = await fetchTokenPlanUsage(endpointId, force)
      const now = Date.now()
      setEntry(endpointId, {
        usage,
        fetchedAt: now,
        lastAttemptAt: now,
        refreshFailed: false,
        refreshing: true,
      })
      scheduleExpiry(endpointId, now + TOKEN_PLAN_CACHE_TTL_MS)
      notify([endpointId])
      return { ok: true, cause: null }
    } catch (cause) {
      const now = Date.now()
      const current = cache.get(endpointId)
      if (current && current.usage !== null) {
        setEntry(endpointId, {
          ...current,
          lastAttemptAt: now,
          refreshFailed: true,
          refreshing: true,
        })
      } else {
        setEntry(endpointId, {
          usage: null,
          fetchedAt: 0,
          lastAttemptAt: now,
          refreshFailed: true,
          refreshing: true,
        })
      }
      scheduleExpiry(endpointId, now + TOKEN_PLAN_CACHE_TTL_MS)
      notify([endpointId])
      return { ok: false, cause }
    } finally {
      inflight.delete(flightKey)
      const current = cache.get(endpointId)
      if (current) {
        const refreshing = hasFlight(endpointId)
        if (current.refreshing !== refreshing) {
          cache.set(endpointId, { ...current, refreshing })
          if (current.usage === null && !refreshing) notify([endpointId])
        }
      }
    }
  })()
  inflight.set(flightKey, request)
  return request
}

function warmRequest(endpointId: string): TokenPlanUsageRequest {
  return {
    ...getTokenPlanUsageSnapshot(endpointId),
    requested: false,
    failed: false,
    cause: null,
  }
}

function resolvedRequest(
  endpointId: string,
  outcome: TokenPlanFetchOutcome,
): TokenPlanUsageRequest {
  return {
    ...getTokenPlanUsageSnapshot(endpointId),
    requested: true,
    failed: !outcome.ok,
    cause: outcome.cause,
  }
}

async function requestTokenPlanUsageWithPolicy(
  endpointId: string,
  options: { refresh?: boolean },
  joinForceOnColdRead: boolean,
): Promise<TokenPlanUsageRequest> {
  if (!endpointId) return warmRequest(endpointId)
  if (options.refresh) {
    return resolvedRequest(
      endpointId,
      await fetchTokenPlanUsageOnce(endpointId, true),
    )
  }

  const entry = cache.get(endpointId)
  if (!entry) {
    return resolvedRequest(
      endpointId,
      await fetchTokenPlanUsageOnce(endpointId),
    )
  }

  if (entry.usage === null && entry.refreshing) {
    const automatic = inflight.get(`automatic:${endpointId}`)
    const force = inflight.get(`force:${endpointId}`)
    const pending = automatic ?? (joinForceOnColdRead ? force : undefined)
    if (pending) return resolvedRequest(endpointId, await pending)
    if (force) return { ...warmRequest(endpointId), requested: true }
  }

  if (entry.refreshing) {
    return { ...warmRequest(endpointId), requested: true }
  }
  if (isTokenPlanUsageFresh(endpointId)) return warmRequest(endpointId)

  const retryDue = Date.now() - entry.lastAttemptAt >= TOKEN_PLAN_CACHE_TTL_MS
  if (!retryDue) return warmRequest(endpointId)
  void fetchTokenPlanUsageOnce(endpointId)
  return { ...warmRequest(endpointId), requested: true }
}

export function requestTokenPlanUsage(
  endpointId: string,
  options: { refresh?: boolean } = {},
): Promise<TokenPlanUsageRequest> {
  return requestTokenPlanUsageWithPolicy(endpointId, options, true)
}

export async function prefetchTokenPlanUsage(
  endpointId: string,
): Promise<TokenPlanUsageResponse | null> {
  return (await requestTokenPlanUsage(endpointId)).usage
}

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
      const endpointId = ids[index++]
      if (endpointId === undefined) break
      if (!isEligible(endpointId)) continue
      await requestTokenPlanUsageWithPolicy(endpointId, {}, false)
      const automatic = inflight.get(`automatic:${endpointId}`)
      if (automatic) await automatic
    }
  })
  await Promise.all(workers)
}

export function __resetTokenPlanCacheForTests(): void {
  cache.clear()
  inflight.clear()
  expiryDeadlines.clear()
  cancelExpiryTimer()
  listenersByEndpoint.clear()
  revisionListeners.clear()
  expiryListeners.clear()
  revision = 0
}
