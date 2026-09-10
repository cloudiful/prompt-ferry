import { afterEach, beforeEach, expect, jest, mock, test } from 'bun:test'
import type { TokenPlanUsageResponse } from '../src/generated/admin-api'

const storage = new Map<string, string>()
Object.defineProperty(globalThis, 'localStorage', {
  value: {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  },
  configurable: true,
})

// `fetchTokenPlanUsage` is the only function the cache layer consumes;
// the rest of the module's exports are stubbed so other test files can
// share the same module registry without seeing missing exports.
const fetchTokenPlanUsage = mock(
  async (_endpointId: string): Promise<TokenPlanUsageResponse> => ({
    keys: [],
    provider: 'minimax',
    provider_region: null,
  }),
)

mock.module('../src/stores/endpoints-api', () => ({
  createEndpoint: mock(async () => ({})),
  createModelRoute: mock(async () => ({})),
  deleteEndpoint: mock(async () => undefined),
  deleteEndpointById: mock(async () => undefined),
  deleteModelRoute: mock(async () => undefined),
  deleteModelRouteById: mock(async () => undefined),
  endpointFormToRequest: mock((form: unknown) => form),
  endpointToForm: mock((endpoint: unknown) => endpoint),
  expectData: mock((value: unknown) => value),
  fetchEndpointsPage: mock(async () => ({
    endpoints: [],
    first: 0,
    rows: 10,
    total: 0,
  })),
  fetchModelRoutesPage: mock(async () => ({
    first: 0,
    routes: [],
    rows: 10,
    total: 0,
  })),
  fetchTokenPlanUsage,
  listEndpoints: mock(async () => undefined),
  listModelRoutes: mock(async () => undefined),
  modelRouteFormToRequest: mock((form: unknown) => form),
  modelRouteToForm: mock((route: unknown) => route),
  persistEndpoint: mock(async () => ({})),
  persistModelRoute: mock(async () => ({})),
  runEndpointTest: mock(async () => ({
    duration_ms: 0,
    message: '',
    ok: true,
    status: 200,
  })),
  runModelRouteProbe: mock(async () => ({
    duration_ms: 0,
    endpoint_name: '',
    message: '',
    model: null,
    model_pattern: '',
    ok: true,
    preferred_endpoint_name: '',
    status: 200,
  })),
  testEndpoint: mock(async () => undefined),
  testModelRoute: mock(async () => undefined),
  tokenPlanUsage: mock(async () => undefined),
  updateEndpoint: mock(async () => ({})),
  updateEndpointEnabled: mock(async () => ({})),
  updateModelRoute: mock(async () => ({})),
  updateModelRouteEnabled: mock(async () => ({})),
  withData: mock((value: unknown) => value),
}))

const {
  __resetTokenPlanCacheForTests,
  getCachedTokenPlanUsage,
  isTokenPlanUsageFresh,
  prefetchTokenPlanBatch,
  prefetchTokenPlanUsage,
  TOKEN_PLAN_CACHE_TTL_MS,
} = await import('../src/composables/useTokenPlanUsageCache')

beforeEach(() => {
  __resetTokenPlanCacheForTests()
  fetchTokenPlanUsage.mockReset()
  fetchTokenPlanUsage.mockImplementation(
    async (_endpointId: string): Promise<TokenPlanUsageResponse> => ({
      keys: [],
      provider: 'minimax',
      provider_region: null,
    }),
  )
})

afterEach(() => {
  __resetTokenPlanCacheForTests()
})

test('cache TTL constant mirrors the backend 60s refresh window', () => {
  expect(TOKEN_PLAN_CACHE_TTL_MS).toBe(60_000)
})

test('prefetchTokenPlanUsage caches the response so a second call does not hit the network', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce({
    keys: [],
    provider: 'minimax',
    provider_region: null,
  })

  await prefetchTokenPlanUsage('ep-1')
  await prefetchTokenPlanUsage('ep-1')

  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
  expect(getCachedTokenPlanUsage('ep-1')).not.toBeNull()
})

test('prefetchTokenPlanUsage coalesces concurrent callers onto one in-flight promise', async () => {
  let release!: () => void
  const pending = new Promise<TokenPlanUsageResponse>((resolve) => {
    release = () =>
      resolve({
        keys: [],
        provider: 'minimax',
        provider_region: null,
      })
  })
  fetchTokenPlanUsage.mockImplementationOnce(() => pending)

  const first = prefetchTokenPlanUsage('ep-busy')
  const second = prefetchTokenPlanUsage('ep-busy')
  const third = prefetchTokenPlanUsage('ep-busy')

  release()
  const results = await Promise.all([first, second, third])

  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
  for (const result of results) {
    expect(result?.provider).toBe('minimax')
  }
})

test('prefetchTokenPlanBatch caps in-flight fetches at the supplied concurrency', async () => {
  let active = 0
  let maxActive = 0
  const pending: Array<() => void> = []
  fetchTokenPlanUsage.mockImplementation(async (endpointId: string) => {
    active += 1
    maxActive = Math.max(maxActive, active)
    await new Promise<void>((resolve) => pending.push(() => resolve()))
    active -= 1
    return { keys: [], provider: 'minimax', provider_region: null }
  })

  const batch = prefetchTokenPlanBatch(
    ['ep-a', 'ep-b', 'ep-c', 'ep-d', 'ep-e', 'ep-f', 'ep-g', 'ep-h'],
    4,
  )

  // Allow the microtasks to flush so the workers pick up their first
  // id, then release them in waves to keep the in-flight count <= 4.
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
  while (pending.length > 0) {
    const releaseBatch = pending.splice(0, 4)
    releaseBatch.forEach((resolve) => resolve())
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
  }

  await batch
  expect(maxActive).toBeLessThanOrEqual(4)
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(8)
})

test('prefetchTokenPlanBatch deduplicates the input id list', async () => {
  await prefetchTokenPlanBatch(['ep-1', 'ep-1', 'ep-2', 'ep-2', 'ep-2'], 4)
  expect(fetchTokenPlanUsage.mock.calls.map(([id]) => id)).toEqual([
    'ep-1',
    'ep-2',
  ])
})

test('failed fetch is recorded as a negative cache entry and still coalesces', async () => {
  fetchTokenPlanUsage.mockRejectedValueOnce(new Error('boom'))

  const first = prefetchTokenPlanUsage('ep-fail')
  const second = prefetchTokenPlanUsage('ep-fail')
  const results = await Promise.all([first, second])

  expect(results).toEqual([null, null])
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
  // The negative entry pins the failure timestamp; a fresh call inside
  // the TTL window must NOT retry the network.
  await prefetchTokenPlanUsage('ep-fail')
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
})

test('prefetchTokenPlanUsage ignores empty endpoint ids', async () => {
  await prefetchTokenPlanUsage('')
  expect(fetchTokenPlanUsage).not.toHaveBeenCalled()
})

test('getCachedTokenPlanUsage returns null when no entry exists or the cache is reset', async () => {
  expect(getCachedTokenPlanUsage('missing')).toBeNull()
  await prefetchTokenPlanUsage('fresh')
  expect(getCachedTokenPlanUsage('fresh')).not.toBeNull()
  __resetTokenPlanCacheForTests()
  expect(getCachedTokenPlanUsage('fresh')).toBeNull()
})

// Stale-while-revalidate: after the TTL window elapses the cache must
// keep serving the last snapshot so the table doesn't blank out while
// the background refresh lands. We drive Date.now() with bun's
// `setSystemTime` so the test stays fast and deterministic.
test('getCachedTokenPlanUsage returns the stale value (not null) after the TTL window', async () => {
  const start = new Date('2026-09-10T00:00:00.000Z')
  jest.setSystemTime(start)
  const snapshot: TokenPlanUsageResponse = {
    keys: [],
    provider: 'minimax',
    provider_region: null,
  }
  fetchTokenPlanUsage.mockResolvedValueOnce(snapshot)

  await prefetchTokenPlanUsage('ep-stale')

  // Advance past the TTL but not so far that the next test's clock is
  // disturbed — restore to wall-clock at the end of the case.
  jest.setSystemTime(new Date(start.getTime() + TOKEN_PLAN_CACHE_TTL_MS + 1))

  expect(isTokenPlanUsageFresh('ep-stale')).toBe(false)
  // Stale value must still be readable; the previous implementation
  // dropped to `null` here, which blanked the row for one network RTT.
  expect(getCachedTokenPlanUsage('ep-stale')).toEqual(snapshot)

  jest.setSystemTime()
})

test('prefetchTokenPlanUsage returns the stale value and triggers a background refresh after the TTL window', async () => {
  const start = new Date('2026-09-10T00:00:00.000Z')
  jest.setSystemTime(start)
  const firstSnapshot: TokenPlanUsageResponse = {
    keys: [],
    provider: 'minimax',
    provider_region: null,
  }
  const secondSnapshot: TokenPlanUsageResponse = {
    keys: [],
    provider: 'command_code',
    provider_region: null,
  }
  fetchTokenPlanUsage.mockResolvedValueOnce(firstSnapshot)

  await prefetchTokenPlanUsage('ep-swr')

  jest.setSystemTime(new Date(start.getTime() + TOKEN_PLAN_CACHE_TTL_MS + 1))
  fetchTokenPlanUsage.mockResolvedValueOnce(secondSnapshot)

  // The prefetch must return the stale snapshot synchronously (no
  // `await` of the background fetch) so the caller doesn't stall while
  // the network resolves.
  const returned = await prefetchTokenPlanUsage('ep-swr')
  expect(returned).toEqual(firstSnapshot)

  // Background refresh: a follow-up call within the same tick must see
  // the in-flight promise and skip a duplicate network hit.
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(2)

  // Drain the background refresh so the cache settles to the second
  // snapshot, then verify the fresh payload is now visible.
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
  await prefetchTokenPlanUsage('ep-swr')
  expect(getCachedTokenPlanUsage('ep-swr')).toEqual(secondSnapshot)
  expect(isTokenPlanUsageFresh('ep-swr')).toBe(true)

  jest.setSystemTime()
})

test('prefetchTokenPlanUsage coalesces concurrent stale callers onto one background refresh', async () => {
  const start = new Date('2026-09-10T00:00:00.000Z')
  jest.setSystemTime(start)
  fetchTokenPlanUsage.mockResolvedValueOnce({
    keys: [],
    provider: 'minimax',
    provider_region: null,
  })

  await prefetchTokenPlanUsage('ep-swr-coalesce')

  jest.setSystemTime(new Date(start.getTime() + TOKEN_PLAN_CACHE_TTL_MS + 1))
  let release!: () => void
  const pending = new Promise<TokenPlanUsageResponse>((resolve) => {
    release = () =>
      resolve({
        keys: [],
        provider: 'minimax',
        provider_region: null,
      })
  })
  fetchTokenPlanUsage.mockImplementationOnce(() => pending)

  const first = prefetchTokenPlanUsage('ep-swr-coalesce')
  const second = prefetchTokenPlanUsage('ep-swr-coalesce')
  const third = prefetchTokenPlanUsage('ep-swr-coalesce')

  release()
  const results = await Promise.all([first, second, third])

  // All three callers must observe the stale snapshot immediately.
  for (const result of results) {
    expect(result?.provider).toBe('minimax')
  }
  // Only one background fetch should have been kicked off.
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(2)

  jest.setSystemTime()
})

// P2: when a background refresh fails after we already have a usable
// snapshot, the cache must NOT overwrite it with `null`. The table
// should keep rendering the last known numbers while the upstream
// recovers, and the next prefetch within the (refreshed) TTL window
// must NOT immediately re-hit the network — that's the back-off.
test('refresh failure preserves the prior snapshot and backs off (no null overwrite)', async () => {
  const start = new Date('2026-09-10T00:00:00.000Z')
  jest.setSystemTime(start)
  const snapshot: TokenPlanUsageResponse = {
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [
          {
            model_name: 'g',
            interval: { remaining_percent: 33 },
            weekly: { remaining_percent: 44 },
          },
        ],
      },
    ],
    provider: 'minimax',
    provider_region: null,
  }
  fetchTokenPlanUsage.mockResolvedValueOnce(snapshot)
  await prefetchTokenPlanUsage('ep-backoff')

  // Cross the TTL so the next prefetch triggers a background refresh.
  jest.setSystemTime(new Date(start.getTime() + TOKEN_PLAN_CACHE_TTL_MS + 1))
  fetchTokenPlanUsage.mockRejectedValueOnce(new Error('upstream 503'))

  const returned = await prefetchTokenPlanUsage('ep-backoff')
  // SWR: caller must see the stale snapshot synchronously.
  expect(returned).toEqual(snapshot)
  // Drain the in-flight background refresh.
  await new Promise<void>((resolve) => setTimeout(resolve, 0))

  // The cache must still hold the original snapshot, NOT a `null`
  // placeholder. The previous implementation wrote `{ usage: null }`
  // here, which blanked the row for the entire recovery window.
  expect(getCachedTokenPlanUsage('ep-backoff')).toEqual(snapshot)

  // Back-off: the failed refresh bumped `fetchedAt`, so the entry is
  // now "fresh" again and the next prefetch within the TTL window
  // must NOT immediately retry the network. The caller gets the
  // snapshot; only a follow-up past the TTL would kick another
  // background fetch.
  expect(isTokenPlanUsageFresh('ep-backoff')).toBe(true)
  await prefetchTokenPlanUsage('ep-backoff')
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(2)

  // Crossing the TTL again would re-attempt the refresh; wire the next
  // mock to a recovery payload so we observe the round-trip.
  jest.setSystemTime(
    new Date(start.getTime() + TOKEN_PLAN_CACHE_TTL_MS * 2 + 2),
  )
  const recovered: TokenPlanUsageResponse = {
    keys: [],
    provider: 'command_code',
    provider_region: null,
  }
  fetchTokenPlanUsage.mockResolvedValueOnce(recovered)
  await prefetchTokenPlanUsage('ep-backoff')
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
  expect(getCachedTokenPlanUsage('ep-backoff')).toEqual(recovered)

  jest.setSystemTime()
})
