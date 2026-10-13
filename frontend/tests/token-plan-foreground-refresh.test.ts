import {
  afterAll,
  afterEach,
  beforeEach,
  expect,
  jest,
  mock,
  test,
} from 'bun:test'
import type { TokenPlanUsageResponse } from '../src/generated/admin-api'
import { minimaxQuotaKey, usage } from './helpers/token-plan-quota-fixtures'

const previousDocument = Object.getOwnPropertyDescriptor(globalThis, 'document')
const previousWindow = Object.getOwnPropertyDescriptor(globalThis, 'window')

type FakeDoc = EventTarget & {
  visibilityState: string
  createElement: (tag: string) => { content: Record<string, unknown> }
  querySelector: (selector: string) => null
}

const docTarget = new EventTarget() as FakeDoc
docTarget.visibilityState = 'visible'
docTarget.createElement = () => ({ content: {} })
docTarget.querySelector = () => null
const doc: FakeDoc = docTarget
Object.defineProperty(globalThis, 'document', {
  configurable: true,
  value: doc,
  writable: true,
})
const win = new EventTarget()
Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: win,
  writable: true,
})

const fetchTokenPlanUsage = mock(
  async (_endpointId: string): Promise<TokenPlanUsageResponse> => ({
    keys: [],
    provider: 'minimax',
    provider_region: null,
  }),
)

mock.module('../src/stores/endpoints-api', () => ({ fetchTokenPlanUsage }))

const {
  __resetTokenPlanCacheForTests,
  getTokenPlanUsageSnapshot,
  prefetchTokenPlanUsage,
  TOKEN_PLAN_CACHE_TTL_MS,
} = await import('../src/composables/useTokenPlanUsageCache')
const { __resetTokenPlanForegroundForTests, registerTokenPlanForegroundIds } =
  await import('../src/composables/tokenPlanForegroundRefresh')
const { useTokenPlanBadges } =
  await import('../src/composables/useTokenPlanBadges')

async function flush(times = 20): Promise<void> {
  for (let i = 0; i < times; i += 1) await Promise.resolve()
}

function setVisible(visible: boolean): void {
  doc.visibilityState = visible ? 'visible' : 'hidden'
}

function fireResume(): void {
  doc.dispatchEvent(new Event('visibilitychange'))
  win.dispatchEvent(new Event('focus'))
  win.dispatchEvent(new Event('pageshow'))
}

beforeEach(() => {
  jest.useFakeTimers()
  jest.setSystemTime(new Date('2026-10-11T00:00:00.000Z'))
  __resetTokenPlanCacheForTests()
  __resetTokenPlanForegroundForTests()
  fetchTokenPlanUsage.mockReset()
  fetchTokenPlanUsage.mockImplementation(async () => ({
    keys: [],
    provider: 'minimax',
    provider_region: null,
  }))
  setVisible(true)
})

afterEach(() => {
  __resetTokenPlanCacheForTests()
  __resetTokenPlanForegroundForTests()
  jest.useRealTimers()
})

afterAll(() => {
  if (previousDocument) {
    Object.defineProperty(globalThis, 'document', previousDocument)
  } else {
    Reflect.deleteProperty(globalThis, 'document')
  }
  if (previousWindow) {
    Object.defineProperty(globalThis, 'window', previousWindow)
  } else {
    Reflect.deleteProperty(globalThis, 'window')
  }
})

test('hidden over 60s then visible resume revalidates registered ids once', async () => {
  const snapshot = usage('minimax', [minimaxQuotaKey('k', 'available', 50, 35)])
  fetchTokenPlanUsage.mockResolvedValueOnce(snapshot)
  await prefetchTokenPlanUsage('ep-resume')
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
  const release = registerTokenPlanForegroundIds(['ep-resume'])

  setVisible(false)
  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS + 1)
  await flush()
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
  expect(getTokenPlanUsageSnapshot('ep-resume').state).toBe('stale')

  const refreshed = usage('minimax', [
    minimaxQuotaKey('k', 'available', 60, 40),
  ])
  fetchTokenPlanUsage.mockResolvedValueOnce(refreshed)
  setVisible(true)
  fireResume()
  await flush(50)

  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(2)
  expect(getTokenPlanUsageSnapshot('ep-resume').state).toBe('ready')
  release()
})

test('fresh entries add zero network requests on visible resume', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce(
    usage('minimax', [minimaxQuotaKey('k', 'available', 50, 35)]),
  )
  await prefetchTokenPlanUsage('ep-fresh')
  const release = registerTokenPlanForegroundIds(['ep-fresh'])

  fireResume()
  await flush()

  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
  expect(getTokenPlanUsageSnapshot('ep-fresh').state).toBe('ready')
  release()
})

test('visible TTL expiry sweeps without waiting for focus', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce(
    usage('minimax', [minimaxQuotaKey('k', 'available', 50, 35)]),
  )
  await prefetchTokenPlanUsage('ep-expiry')
  const release = registerTokenPlanForegroundIds(['ep-expiry'])
  fetchTokenPlanUsage.mockClear()

  const refreshed = usage('minimax', [
    minimaxQuotaKey('k', 'available', 70, 60),
  ])
  fetchTokenPlanUsage.mockResolvedValueOnce(refreshed)
  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS + 1)
  await flush(50)

  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
  expect(getTokenPlanUsageSnapshot('ep-expiry').state).toBe('ready')
  release()
})

test('hidden expiry issues zero requests until visible', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce(
    usage('minimax', [minimaxQuotaKey('k', 'available', 50, 35)]),
  )
  await prefetchTokenPlanUsage('ep-hidden-expiry')
  const release = registerTokenPlanForegroundIds(['ep-hidden-expiry'])

  setVisible(false)
  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS + 1)
  await flush()
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)

  const refreshed = usage('minimax', [
    minimaxQuotaKey('k', 'available', 70, 60),
  ])
  fetchTokenPlanUsage.mockResolvedValueOnce(refreshed)
  setVisible(true)
  doc.dispatchEvent(new Event('visibilitychange'))
  await flush(50)

  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(2)
  release()
})

test('failed refresh keeps numbers and retries after 60s only when visible', async () => {
  const snapshot = usage('minimax', [minimaxQuotaKey('k', 'available', 50, 35)])
  fetchTokenPlanUsage.mockResolvedValueOnce(snapshot)
  await prefetchTokenPlanUsage('ep-retry')
  const release = registerTokenPlanForegroundIds(['ep-retry'])
  const badges = useTokenPlanBadges('ep-retry')

  // Queue the failure before the TTL timer fires: the expiry sweep runs
  // (possibly inside the timer advance) and must consume this rejection.
  fetchTokenPlanUsage.mockRejectedValueOnce(new Error('upstream 503'))
  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS + 1)
  await flush(50)
  // The expiry sweep revalidated once and failed; numbers stay with a stale
  // marker instead of blanking.
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(2)
  expect(getTokenPlanUsageSnapshot('ep-retry').state).toBe('stale')
  expect(badges.value.short).toBe(50)
  expect(badges.value.quotaState).toBe('unknown')

  // Inside the retry window no further request happens, even when visible.
  setVisible(true)
  fireResume()
  await flush()
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(2)

  // Past the retry TTL while hidden still issues nothing.
  setVisible(false)
  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS + 1)
  await flush()
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(2)

  // Visible again retries once and recovers.
  const recovered = usage('minimax', [
    minimaxQuotaKey('k', 'available', 80, 70),
  ])
  fetchTokenPlanUsage.mockResolvedValueOnce(recovered)
  setVisible(true)
  doc.dispatchEvent(new Event('visibilitychange'))
  await flush(50)
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(3)
  expect(getTokenPlanUsageSnapshot('ep-retry').state).toBe('ready')
  release()
})

test('released consumer while queued issues zero future requests', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce(
    usage('minimax', [minimaxQuotaKey('k', 'available', 50, 35)]),
  )
  await prefetchTokenPlanUsage('ep-dispose')
  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS + 1)
  const release = registerTokenPlanForegroundIds(['ep-dispose'])
  fetchTokenPlanUsage.mockClear()

  fireResume()
  release()
  await flush(50)

  expect(fetchTokenPlanUsage).not.toHaveBeenCalled()
})

test('overlapping visibility, focus, pageshow and expiry coalesce to one sweep', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce(
    usage('minimax', [minimaxQuotaKey('k', 'available', 50, 35)]),
  )
  await prefetchTokenPlanUsage('ep-coalesce')
  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS + 1)
  const release = registerTokenPlanForegroundIds(['ep-coalesce'])
  fetchTokenPlanUsage.mockClear()
  fetchTokenPlanUsage.mockResolvedValue(
    usage('minimax', [minimaxQuotaKey('k', 'available', 60, 40)]),
  )

  doc.dispatchEvent(new Event('visibilitychange'))
  win.dispatchEvent(new Event('focus'))
  win.dispatchEvent(new Event('pageshow'))
  doc.dispatchEvent(new Event('visibilitychange'))
  await flush(50)

  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
  release()
})

test('duplicate desktop and mobile consumers share one request', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce(
    usage('minimax', [minimaxQuotaKey('k', 'available', 50, 35)]),
  )
  await prefetchTokenPlanUsage('ep-dup')
  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS + 1)
  const releaseTable = registerTokenPlanForegroundIds(['ep-dup'])
  const releaseCard = registerTokenPlanForegroundIds(['ep-dup'])
  fetchTokenPlanUsage.mockClear()
  fetchTokenPlanUsage.mockResolvedValue(
    usage('minimax', [minimaxQuotaKey('k', 'available', 60, 40)]),
  )

  fireResume()
  await flush(50)

  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
  releaseTable()
  // One consumer remains: a further resume still revalidates once.
  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS + 1)
  await flush(50)
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(2)
  releaseCard()
  await flush()
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(2)
})

test('slow stale batches keep at most 4 automatic requests in flight', async () => {
  const ids = Array.from({ length: 8 }, (_, i) => `ep-slow-${i}`)
  for (const id of ids) {
    fetchTokenPlanUsage.mockResolvedValueOnce(
      usage('minimax', [minimaxQuotaKey('k', 'available', 50, 35)]),
    )
    await prefetchTokenPlanUsage(id)
  }
  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS + 1)
  const release = registerTokenPlanForegroundIds(ids)
  fetchTokenPlanUsage.mockClear()

  let active = 0
  let maxActive = 0
  const gates: Array<() => void> = []
  fetchTokenPlanUsage.mockImplementation(async () => {
    active += 1
    maxActive = Math.max(maxActive, active)
    await new Promise<void>((resolve) => gates.push(resolve))
    active -= 1
    return usage('minimax', [minimaxQuotaKey('k', 'available', 60, 40)])
  })

  fireResume()
  await flush()
  await flush()
  expect(maxActive).toBeLessThanOrEqual(4)
  while (gates.length > 0) {
    gates.splice(0, 4).forEach((releaseGate) => releaseGate())
    await flush()
  }
  await flush(50)

  expect(maxActive).toBeLessThanOrEqual(4)
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(8)
  release()
})

test('initial registration never resets a pinned negative entry', async () => {
  fetchTokenPlanUsage.mockRejectedValueOnce(new Error('boom'))
  await prefetchTokenPlanUsage('ep-negative')
  expect(getTokenPlanUsageSnapshot('ep-negative').state).toBe('error')
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)

  const release = registerTokenPlanForegroundIds(['ep-negative'])
  await flush()
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
  expect(getTokenPlanUsageSnapshot('ep-negative').state).toBe('error')
  release()
})

test('engine is safe without browser globals and never networks', async () => {
  const savedDoc = (globalThis as Record<string, unknown>).document
  const savedWin = (globalThis as Record<string, unknown>).window
  Reflect.deleteProperty(globalThis, 'document')
  Reflect.deleteProperty(globalThis, 'window')
  try {
    const release = registerTokenPlanForegroundIds(['ep-ssr'])
    await flush()
    expect(fetchTokenPlanUsage).not.toHaveBeenCalled()
    release()
  } finally {
    Object.defineProperty(globalThis, 'document', {
      configurable: true,
      value: savedDoc,
      writable: true,
    })
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: savedWin,
      writable: true,
    })
  }
})

async function warmStaleIds(ids: string[]): Promise<void> {
  for (const id of ids) {
    fetchTokenPlanUsage.mockResolvedValueOnce(
      usage('minimax', [minimaxQuotaKey('k', 'available', 50, 35)]),
    )
    await prefetchTokenPlanUsage(id)
  }
  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS + 1)
  fetchTokenPlanUsage.mockClear()
}

test('release after the first 4 start keeps the remaining queue unfetched', async () => {
  const ids = Array.from({ length: 8 }, (_, i) => `ep-relq-${i}`)
  await warmStaleIds(ids)
  const release = registerTokenPlanForegroundIds(ids)

  let active = 0
  let maxActive = 0
  const gates: Array<() => void> = []
  fetchTokenPlanUsage.mockImplementation(async () => {
    active += 1
    maxActive = Math.max(maxActive, active)
    await new Promise<void>((resolve) => gates.push(resolve))
    active -= 1
    return usage('minimax', [minimaxQuotaKey('k', 'available', 60, 40)])
  })

  fireResume()
  await flush()
  await flush()
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(4)
  expect(maxActive).toBeLessThanOrEqual(4)

  release()
  gates.splice(0).forEach((resolve) => resolve())
  await flush(50)

  // The remaining queue never issued once the last consumer left.
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(4)

  // Explicit active registration plus visible recovery fetches the rest.
  const recovered = registerTokenPlanForegroundIds(ids)
  fetchTokenPlanUsage.mockImplementation(async () => {
    active += 1
    maxActive = Math.max(maxActive, active)
    active -= 1
    return usage('minimax', [minimaxQuotaKey('k', 'available', 60, 40)])
  })
  doc.dispatchEvent(new Event('visibilitychange'))
  await flush(50)
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(8)
  recovered()
})

test('hide after the first 4 start keeps the remaining queue unfetched', async () => {
  const ids = Array.from({ length: 8 }, (_, i) => `ep-hideq-${i}`)
  await warmStaleIds(ids)
  const release = registerTokenPlanForegroundIds(ids)

  let active = 0
  const gates: Array<() => void> = []
  fetchTokenPlanUsage.mockImplementation(async () => {
    active += 1
    await new Promise<void>((resolve) => gates.push(resolve))
    active -= 1
    return usage('minimax', [minimaxQuotaKey('k', 'available', 60, 40)])
  })

  fireResume()
  await flush()
  await flush()
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(4)

  setVisible(false)
  gates.splice(0).forEach((resolve) => resolve())
  await flush(50)

  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(4)

  // Explicit visible recovery fetches the remainder.
  fetchTokenPlanUsage.mockImplementation(async () => {
    active += 1
    active -= 1
    return usage('minimax', [minimaxQuotaKey('k', 'available', 60, 40)])
  })
  setVisible(true)
  doc.dispatchEvent(new Event('visibilitychange'))
  await flush(50)
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(8)
  release()
})

test('a disposed generation is never revived by a dirty rerun', async () => {
  const oldIds = Array.from({ length: 4 }, (_, i) => `ep-old-${i}`)
  const newIds = Array.from({ length: 4 }, (_, i) => `ep-new-${i}`)
  await warmStaleIds([...oldIds, ...newIds])
  const releaseOld = registerTokenPlanForegroundIds(oldIds)

  const gates: Array<() => void> = []
  fetchTokenPlanUsage.mockImplementation(async () => {
    await new Promise<void>((resolve) => gates.push(resolve))
    return usage('minimax', [minimaxQuotaKey('k', 'available', 60, 40)])
  })

  fireResume()
  await flush()
  await flush()
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(4)

  // Dispose the whole generation mid-sweep, then register fresh ids with no
  // new event: nothing may schedule a sweep for the new generation on its
  // own, and the old sweep's dirty path must not revive either.
  releaseOld()
  const releaseNew = registerTokenPlanForegroundIds(newIds)
  gates.splice(0).forEach((resolve) => resolve())
  await flush(50)

  // The old sweep finished its started requests; with no genuine event for
  // the new generation, nothing refetched.
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(4)

  // Only an explicit visible event for the active registration fetches.
  fetchTokenPlanUsage.mockImplementation(async () =>
    usage('minimax', [minimaxQuotaKey('k', 'available', 60, 40)]),
  )
  doc.dispatchEvent(new Event('visibilitychange'))
  await flush(50)
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(8)
  releaseNew()
})

test('queued focus then release plus same-tick re-registration fetches nothing', async () => {
  await warmStaleIds(['ep-old-q', 'ep-new-q'])
  const releaseOld = registerTokenPlanForegroundIds(['ep-old-q'])
  fetchTokenPlanUsage.mockClear()

  // Reviewer interleaving 1: the focus queues a microtask, the generation
  // turns over synchronously, and no new event ever arrives.
  win.dispatchEvent(new Event('focus'))
  releaseOld()
  const releaseNew = registerTokenPlanForegroundIds(['ep-new-q'])
  await flush(50)

  expect(fetchTokenPlanUsage).not.toHaveBeenCalled()
  releaseNew()
})

test('release plus same-id re-registration before resolve keeps old queue dead', async () => {
  const ids = Array.from({ length: 8 }, (_, i) => `ep-sameq-${i}`)
  await warmStaleIds(ids)
  const release = registerTokenPlanForegroundIds(ids)

  const gates: Array<() => void> = []
  fetchTokenPlanUsage.mockImplementation(async () => {
    await new Promise<void>((resolve) => gates.push(resolve))
    return usage('minimax', [minimaxQuotaKey('k', 'available', 60, 40)])
  })

  fireResume()
  await flush()
  await flush()
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(4)

  // Reviewer interleaving 2: same ids return in a new generation while the
  // old batch still holds its first four flights. Resolving must not start
  // the remaining four from the disposed generation without a new event.
  release()
  const reRegistered = registerTokenPlanForegroundIds(ids)
  gates.splice(0).forEach((resolve) => resolve())
  await flush(50)

  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(4)
  reRegistered()
})

test('genuine new-generation event during an old batch recovers active ids', async () => {
  const oldIds = Array.from({ length: 8 }, (_, i) => `ep-actold-${i}`)
  const newIds = Array.from({ length: 4 }, (_, i) => `ep-actnew-${i}`)
  await warmStaleIds([...oldIds, ...newIds])
  const releaseOld = registerTokenPlanForegroundIds(oldIds)

  let active = 0
  let maxActive = 0
  const gates: Array<() => void> = []
  fetchTokenPlanUsage.mockImplementation(async () => {
    active += 1
    maxActive = Math.max(maxActive, active)
    await new Promise<void>((resolve) => gates.push(resolve))
    active -= 1
    return usage('minimax', [minimaxQuotaKey('k', 'available', 60, 40)])
  })

  fireResume()
  await flush()
  await flush()
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(4)

  // New generation arrives with a genuine event while old flights pend: it
  // must stay pending, then run serialized after the old batch settles.
  releaseOld()
  const releaseNew = registerTokenPlanForegroundIds(newIds)
  doc.dispatchEvent(new Event('visibilitychange'))
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(4)
  gates.splice(0).forEach((resolve) => resolve())
  await flush(50)

  // The old batch settled its started requests without starting old queued
  // work, and the genuine new-generation event ran serialized afterwards.
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(8)
  expect(maxActive).toBeLessThanOrEqual(4)
  // The rerun sweep holds its own deferred flights; let them land too.
  gates.splice(0).forEach((resolve) => resolve())
  await flush(50)
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(8)
  for (const id of newIds) {
    expect(getTokenPlanUsageSnapshot(id).state).toBe('ready')
  }
  releaseNew()
})
