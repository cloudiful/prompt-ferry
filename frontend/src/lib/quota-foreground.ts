import {
  prefetchTokenPlanBatch,
  subscribeTokenPlanUsageExpiry,
} from '@/lib/quota-cache'

const refcounts = new Map<string, number>()
const registrationEpochs = new Map<string, number>()
let unsubscribeExpiry: (() => void) | null = null
let sweepRunning = false
let sweepDirty = false
let dirtyEpoch = 0
let scheduledForEpoch: number | null = null
let consumerEpoch = 0

function currentIds(): string[] {
  return [...refcounts.keys()]
}

function isForegroundVisible(): boolean {
  if (typeof document === 'undefined') return false
  const state = (document as { visibilityState?: string }).visibilityState
  return state === undefined || state === 'visible'
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
    if (ids.length === 0 || !isForegroundVisible()) return
    const registrations = new Map(
      ids.map((endpointId) => [
        endpointId,
        registrationEpochs.get(endpointId) ?? 0,
      ]),
    )
    await prefetchTokenPlanBatch(
      ids,
      4,
      (endpointId) =>
        sweepEpoch === consumerEpoch &&
        registrationEpochs.get(endpointId) === registrations.get(endpointId) &&
        refcounts.has(endpointId) &&
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
  if (scheduledForEpoch === consumerEpoch) return
  const epoch = consumerEpoch
  scheduledForEpoch = epoch
  queueMicrotask(() => {
    if (scheduledForEpoch === epoch) scheduledForEpoch = null
    if (epoch !== consumerEpoch || refcounts.size === 0) return
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
  if (expiredIds.some((endpointId) => refcounts.has(endpointId))) {
    scheduleSweep()
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
}

export function registerTokenPlanForegroundIds(
  endpointIds: Iterable<string>,
): () => void {
  const unique = Array.from(new Set(endpointIds)).filter(Boolean)
  for (const endpointId of unique) {
    const count = refcounts.get(endpointId) ?? 0
    if (count === 0) {
      registrationEpochs.set(
        endpointId,
        (registrationEpochs.get(endpointId) ?? 0) + 1,
      )
    }
    refcounts.set(endpointId, count + 1)
  }
  if (unique.length > 0) ensureInstalled()
  let released = false
  return () => {
    if (released) return
    released = true
    for (const endpointId of unique) {
      const count = refcounts.get(endpointId) ?? 0
      if (count <= 1) {
        refcounts.delete(endpointId)
        registrationEpochs.set(
          endpointId,
          (registrationEpochs.get(endpointId) ?? 0) + 1,
        )
      } else {
        refcounts.set(endpointId, count - 1)
      }
    }
    teardownIfIdle()
  }
}

export function __resetTokenPlanForegroundForTests(): void {
  refcounts.clear()
  registrationEpochs.clear()
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
