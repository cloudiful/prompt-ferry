import { getCurrentScope, onScopeDispose, watch } from 'vue'
import {
  prefetchTokenPlanBatch,
  subscribeTokenPlanUsageExpiry,
} from './useTokenPlanUsageCache'

// Shared foreground revalidation engine: desktop table and CSS-hidden mobile
// cards mount simultaneously, so listener ownership is module-level with
// refcounted ids. The first consumer installs visibility/focus/pageshow
// listeners plus the cache-expiry subscription; the last release removes all
// of them. Same-tick events coalesce into one serialized sweep; the existing
// inflight/TTL/negativeTTL policy owns throttling, so no bespoke delay here.

const refcounts = new Map<string, number>()
let unsubscribeExpiry: (() => void) | null = null
let sweepRunning = false
let sweepDirty = false
// Epoch of the generation that set the dirty flag: a rerun is only scheduled
// when it still matches the current generation, so genuine new-generation
// events survive an old sweep while disposed-generation noise never revives.
let dirtyEpoch = 0
// Epoch-tagged scheduling flag: a queued microtask owns only its own
// generation and must never clear or consume a newer generation's event.
let scheduledForEpoch: number | null = null
// Consumer epoch: bumped every time the last consumer releases (idle), so a
// disposed generation's queued work can never fetch for a newer one.
let consumerEpoch = 0

function currentIds(): string[] {
  return [...refcounts.keys()]
}

function isForegroundVisible(): boolean {
  if (typeof document === 'undefined') return false
  const state = (document as { visibilityState?: string }).visibilityState
  if (state === undefined) return true
  return state === 'visible'
}

async function runSweep(): Promise<void> {
  if (refcounts.size === 0) return
  if (sweepRunning) {
    sweepDirty = true
    dirtyEpoch = consumerEpoch
    return
  }
  sweepRunning = true
  sweepDirty = false
  const sweepEpoch = consumerEpoch
  try {
    const ids = currentIds()
    if (ids.length === 0) return
    if (!isForegroundVisible()) return
    // Live predicate per queued id, evaluated immediately before each id
    // starts: the captured sweep epoch plus current registration and
    // visibility. Release plus immediate same-id re-registration bumps the
    // epoch, so old queued items cannot resume even when the ids match
    // again; already-started requests still finish into the cache.
    await prefetchTokenPlanBatch(
      ids,
      4,
      (id) =>
        sweepEpoch === consumerEpoch &&
        refcounts.has(id) &&
        isForegroundVisible(),
    )
  } finally {
    sweepRunning = false
    if (sweepDirty && dirtyEpoch === consumerEpoch && refcounts.size > 0) {
      sweepDirty = false
      scheduleSweep()
    } else {
      sweepDirty = false
    }
  }
}

function scheduleSweep(): void {
  if (refcounts.size === 0) return
  if (sweepRunning) {
    sweepDirty = true
    dirtyEpoch = consumerEpoch
    return
  }
  // Coalesce same-generation events; a newer generation always queues its
  // own microtask instead of being consumed by a stale one.
  if (scheduledForEpoch === consumerEpoch) return
  const epoch = consumerEpoch
  scheduledForEpoch = epoch
  queueMicrotask(() => {
    // Only clear our own generation's flag: a stale microtask must not wipe
    // a newer generation's scheduling state.
    if (scheduledForEpoch === epoch) scheduledForEpoch = null
    // A disposed generation's queued callback never runs.
    if (epoch !== consumerEpoch) return
    if (refcounts.size === 0) return
    void runSweep()
  })
}

function handleVisibility(): void {
  scheduleSweep()
}

function handleFocus(): void {
  scheduleSweep()
}

function handlePageShow(): void {
  scheduleSweep()
}

function handleExpiry(expiredIds: readonly string[]): void {
  for (const id of expiredIds) {
    if (refcounts.has(id)) {
      scheduleSweep()
      return
    }
  }
}

function ensureInstalled(): void {
  if (unsubscribeExpiry !== null) return
  unsubscribeExpiry = subscribeTokenPlanUsageExpiry(handleExpiry)
  if (typeof document !== 'undefined') {
    document.addEventListener('visibilitychange', handleVisibility)
  }
  if (typeof window !== 'undefined') {
    window.addEventListener('focus', handleFocus)
    window.addEventListener('pageshow', handlePageShow)
  }
}

function teardownIfIdle(): void {
  if (refcounts.size > 0) return
  if (typeof document !== 'undefined') {
    document.removeEventListener('visibilitychange', handleVisibility)
  }
  if (typeof window !== 'undefined') {
    window.removeEventListener('focus', handleFocus)
    window.removeEventListener('pageshow', handlePageShow)
  }
  unsubscribeExpiry?.()
  unsubscribeExpiry = null
  sweepDirty = false
  dirtyEpoch = consumerEpoch
  consumerEpoch += 1
  // A queued microtask from the disposed generation may still fire, but its
  // epoch check drops it without touching the new generation's scheduling
  // flag. An in-flight sweep may complete into the shared cache, while its
  // dirty rerun requires a matching dirty epoch.
}

// Register eligible ids for foreground revalidation. Dedupes across the
// simultaneously mounted desktop/mobile surfaces via refcounts; the returned
// release must be called on disposal. Registration alone never fetches, so
// initial mounts keep their existing prefetch semantics and negativeTTL is
// never reset by merely observing an id.
export function registerTokenPlanForegroundIds(
  ids: Iterable<string>,
): () => void {
  const unique = Array.from(new Set(ids)).filter(Boolean)
  for (const id of unique) {
    refcounts.set(id, (refcounts.get(id) ?? 0) + 1)
  }
  if (unique.length > 0) ensureInstalled()
  let released = false
  return () => {
    if (released) return
    released = true
    for (const id of unique) {
      const count = refcounts.get(id) ?? 0
      if (count <= 1) refcounts.delete(id)
      else refcounts.set(id, count - 1)
    }
    teardownIfIdle()
  }
}

// Vue wrapper: watches the stable deduped id list and releases on scope
// disposal. The engine itself is Vue-free so bare engine tests never hit a
// lifecycle warning.
export function useTokenPlanForegroundRefresh(
  idsGetter: () => Iterable<string>,
): void {
  let release: (() => void) | null = null
  const stop = watch(
    () => Array.from(new Set(idsGetter())).filter(Boolean).sort().join('|'),
    (key) => {
      release?.()
      const ids = key ? key.split('|') : []
      release = ids.length > 0 ? registerTokenPlanForegroundIds(ids) : null
    },
    { immediate: true },
  )
  const scope = getCurrentScope()
  const dispose = () => {
    stop()
    release?.()
    release = null
  }
  if (scope) onScopeDispose(dispose)
}

export function __resetTokenPlanForegroundForTests(): void {
  refcounts.clear()
  if (typeof document !== 'undefined') {
    document.removeEventListener('visibilitychange', handleVisibility)
  }
  if (typeof window !== 'undefined') {
    window.removeEventListener('focus', handleFocus)
    window.removeEventListener('pageshow', handlePageShow)
  }
  unsubscribeExpiry?.()
  unsubscribeExpiry = null
  sweepRunning = false
  sweepDirty = false
  dirtyEpoch = 0
  scheduledForEpoch = null
  consumerEpoch = 0
}
