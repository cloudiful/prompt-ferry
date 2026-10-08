import { afterEach, beforeEach, expect, jest, mock, test } from 'bun:test'
import type {
  ProviderEndpoint,
  TokenPlanUsageResponse,
} from '../src/generated/admin-api'

// This file owns the API mock for the shared usage cache, so it can count the
// requests the list badges and the usage dialog issue together. The rule under
// test: the dialog consumes the same cache as the list, so opening it after the
// badges warmed the endpoint must add no request at all.
const fetchTokenPlanUsage = mock(
  async (_endpointId: string): Promise<TokenPlanUsageResponse> =>
    ({
      keys: [],
      provider: 'minimax',
      provider_region: null,
    }) as TokenPlanUsageResponse,
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
  fetchModelRoutePage: mock(async () => ({
    first: 0,
    routes: [],
    rows: 10,
    total: 0,
  })),
  fetchOrganizationUsage: mock(async () => undefined),
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
    model_name: '',
    model_pattern: '',
    ok: true,
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
  prefetchTokenPlanUsage,
  requestTokenPlanUsage,
  TOKEN_PLAN_CACHE_TTL_MS,
} = await import('../src/composables/useTokenPlanUsageCache')
const { useEndpointTokenPlanUsage } =
  await import('../src/composables/useEndpointTokenPlanUsage')

const endpoint = (id: string, provider = 'deepseek') =>
  ({
    endpoint_id: id,
    has_oauth_token: null,
    name: id,
    provider,
  }) as unknown as ProviderEndpoint

function snapshot(provider = 'minimax'): TokenPlanUsageResponse {
  return {
    keys: [],
    provider,
    provider_region: null,
  } as TokenPlanUsageResponse
}

beforeEach(() => {
  __resetTokenPlanCacheForTests()
  fetchTokenPlanUsage.mockReset()
  fetchTokenPlanUsage.mockImplementation(async () => snapshot())
})

afterEach(() => {
  __resetTokenPlanCacheForTests()
})

test('a fresh entry is read from cache without reaching the network', async () => {
  fetchTokenPlanUsage.mockResolvedValue(snapshot('deepseek'))
  await prefetchTokenPlanUsage('ep-warm')
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)

  const result = await requestTokenPlanUsage('ep-warm')

  expect(result.state).toBe('ready')
  expect(result.usage?.provider).toBe('deepseek')
  // The signal the dialog uses to prove it added no duplicate request.
  expect(result.requested).toBe(false)
  expect(result.failed).toBe(false)
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
})

test('a read with no endpoint id stays a cold, network-free answer', async () => {
  const result = await requestTokenPlanUsage('')

  // The empty id has no entry and must never reach the network. It is the cold
  // cache shape, not an error: nothing was requested and nothing failed.
  expect(result.state).toBe('loading')
  expect(result.usage).toBeNull()
  expect(result.requested).toBe(false)
  expect(result.failed).toBe(false)
  expect(result.cause).toBeNull()
  expect(fetchTokenPlanUsage).not.toHaveBeenCalled()
})

test('a cold entry pays for exactly one request and reports it as requested', async () => {
  const result = await requestTokenPlanUsage('ep-cold')

  expect(result.state).toBe('ready')
  expect(result.requested).toBe(true)
  expect(result.failed).toBe(false)
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
})

test('a warm cache read and an in-flight badge prefetch share one request', async () => {
  let release!: () => void
  const pending = new Promise<TokenPlanUsageResponse>((resolve) => {
    release = () => resolve(snapshot())
  })
  fetchTokenPlanUsage.mockImplementationOnce(() => pending)

  // The list badges prefetch while the user opens the dialog in the same tick.
  const prefetched = prefetchTokenPlanUsage('ep-race')
  const requested = requestTokenPlanUsage('ep-race')
  release()

  const [, result] = await Promise.all([prefetched, requested])

  expect(result.requested).toBe(true)
  expect(result.usage).not.toBeNull()
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
})

test('a failed cold fetch is reported once, then replayed silently inside the TTL', async () => {
  const cause = new Error('usage upstream down')
  fetchTokenPlanUsage.mockRejectedValueOnce(cause)

  const first = await requestTokenPlanUsage('ep-fail')

  expect(first.state).toBe('error')
  expect(first.usage).toBeNull()
  expect(first.requested).toBe(true)
  // The cause rides along so the caller can report it on its own channel
  // instead of the cache throwing.
  expect(first.failed).toBe(true)
  expect(first.cause).toBe(cause)

  // Inside the TTL the negative entry is served without a retry, and the
  // already-reported failure is not replayed.
  const second = await requestTokenPlanUsage('ep-fail')
  expect(second.state).toBe('error')
  expect(second.requested).toBe(false)
  expect(second.failed).toBe(false)
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
})

test('a stale entry serves the previous payload and revalidates once in the background', async () => {
  const start = new Date('2026-10-08T00:00:00.000Z')
  jest.setSystemTime(start)
  const original = snapshot('deepseek')
  fetchTokenPlanUsage.mockResolvedValueOnce(original)
  await prefetchTokenPlanUsage('ep-stale-read')

  // Advance past the TTL so the next read revalidates.
  jest.setSystemTime(new Date(start.getTime() + TOKEN_PLAN_CACHE_TTL_MS + 1))
  const refreshed = snapshot('command_code')
  fetchTokenPlanUsage.mockResolvedValueOnce(refreshed)

  const result = await requestTokenPlanUsage('ep-stale-read')

  // The caller never stalls on the revalidation.
  expect(result.usage).toEqual(original)
  expect(result.requested).toBe(true)
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(2)

  await new Promise<void>((resolve) => setTimeout(resolve, 0))
  expect(getCachedTokenPlanUsage('ep-stale-read')).toEqual(refreshed)
  jest.setSystemTime()
})

test('a deliberate refresh bypasses the TTL with exactly one request', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce(snapshot('deepseek'))
  await prefetchTokenPlanUsage('ep-force')

  fetchTokenPlanUsage.mockResolvedValueOnce(snapshot('command_code'))
  const result = await requestTokenPlanUsage('ep-force', { refresh: true })

  expect(result.state).toBe('ready')
  expect(result.requested).toBe(true)
  expect(result.usage?.provider).toBe('command_code')
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(2)
  expect(getCachedTokenPlanUsage('ep-force')?.provider).toBe('command_code')
})

test('the dialog opens on a warm snapshot without a second request', async () => {
  fetchTokenPlanUsage.mockResolvedValue(snapshot('deepseek'))
  // The list prefetched the visible rows; the dialog then opens on that row.
  await prefetchTokenPlanUsage('ep-dialog')
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)

  const errors: unknown[] = []
  const dialog = useEndpointTokenPlanUsage(
    () => endpoint('ep-dialog'),
    (cause) => errors.push(cause),
  )
  await dialog.openTokenPlanUsage('ep-dialog')

  expect(dialog.tokenPlanUsageVisible.value).toBe(true)
  expect(dialog.tokenPlanUsage.value?.provider).toBe('deepseek')
  expect(dialog.tokenPlanUsageLoading.value).toBe(false)
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
  expect(errors).toEqual([])
})

test('the dialog reopens a stale snapshot through the shared TTL/SWR policy', async () => {
  const start = new Date('2026-10-08T00:00:00.000Z')
  jest.setSystemTime(start)
  const original = snapshot('deepseek')
  fetchTokenPlanUsage.mockResolvedValueOnce(original)
  // The list badges warmed the endpoint while the dialog stayed closed.
  await prefetchTokenPlanUsage('ep-dialog-stale')
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)

  const errors: unknown[] = []
  const dialog = useEndpointTokenPlanUsage(
    () => endpoint('ep-dialog-stale'),
    (cause) => errors.push(cause),
  )

  // Past the TTL the cached snapshot is still `ready`, which must not make the
  // dialog return forever: opening it serves the snapshot and kicks the SWR
  // revalidate through the shared request policy.
  jest.setSystemTime(new Date(start.getTime() + TOKEN_PLAN_CACHE_TTL_MS + 1))
  const refreshed = snapshot('command_code')
  fetchTokenPlanUsage.mockResolvedValueOnce(refreshed)

  await dialog.openTokenPlanUsage('ep-dialog-stale')

  // The stale snapshot stays on screen (no spinner) while the refresh is in
  // flight, and the revalidation request was actually issued.
  expect(dialog.tokenPlanUsage.value).toEqual(original)
  expect(dialog.tokenPlanUsageLoading.value).toBe(false)
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(2)

  // The background revalidate lands on the shared cache.
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
  expect(getCachedTokenPlanUsage('ep-dialog-stale')).toEqual(refreshed)
  expect(errors).toEqual([])
  jest.setSystemTime()
})

test('the dialog shows the loading state for a cold endpoint and then the payload', async () => {
  let release!: () => void
  const pending = new Promise<TokenPlanUsageResponse>((resolve) => {
    release = () => resolve(snapshot('deepseek'))
  })
  fetchTokenPlanUsage.mockImplementationOnce(() => pending)

  const dialog = useEndpointTokenPlanUsage(
    () => endpoint('ep-dialog-cold'),
    () => {},
  )
  const opening = dialog.openTokenPlanUsage('ep-dialog-cold')

  expect(dialog.tokenPlanUsageVisible.value).toBe(true)
  expect(dialog.tokenPlanUsageLoading.value).toBe(true)
  release()
  await opening

  expect(dialog.tokenPlanUsageLoading.value).toBe(false)
  expect(dialog.tokenPlanUsage.value?.provider).toBe('deepseek')
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
})

test('the dialog reports a failed request once and stays silent on a reopen', async () => {
  const cause = new Error('usage upstream down')
  fetchTokenPlanUsage.mockRejectedValueOnce(cause)
  const errors: unknown[] = []
  const dialog = useEndpointTokenPlanUsage(
    () => endpoint('ep-dialog-fail'),
    (error) => errors.push(error),
  )

  await dialog.openTokenPlanUsage('ep-dialog-fail')
  expect(errors).toEqual([cause])
  expect(dialog.tokenPlanUsage.value).toBeNull()
  expect(dialog.tokenPlanUsageLoading.value).toBe(false)

  // Reopening inside the TTL replays the negative cache entry without a retry
  // and without a second report.
  await dialog.openTokenPlanUsage('ep-dialog-fail')
  expect(errors).toEqual([cause])
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
})

test('the dialog re-reads the endpoint on demand while it is open', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce(snapshot('deepseek'))
  const dialog = useEndpointTokenPlanUsage(
    () => endpoint('ep-dialog-refresh'),
    () => {},
  )
  await dialog.openTokenPlanUsage('ep-dialog-refresh')
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)

  // Inside the TTL the open was a pure cache read, so the user-visible numbers
  // only move when a refresh is asked for.
  fetchTokenPlanUsage.mockResolvedValueOnce(snapshot('command_code'))
  await dialog.refreshTokenPlanUsage()

  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(2)
  expect(dialog.tokenPlanUsage.value?.provider).toBe('command_code')
})

test('an ineligible or unknown endpoint never opens the dialog or fetches', async () => {
  const errors: unknown[] = []
  const dialog = useEndpointTokenPlanUsage(
    (id: string) =>
      id === 'ep-plain' ? endpoint('ep-plain', 'generic') : null,
    (cause) => errors.push(cause),
  )

  await dialog.openTokenPlanUsage('ep-plain')
  await dialog.openTokenPlanUsage('ep-missing')
  // A deliberate refresh with nothing open is a no-op too.
  await dialog.refreshTokenPlanUsage()

  expect(dialog.tokenPlanUsageVisible.value).toBe(false)
  expect(dialog.tokenPlanUsage.value).toBeNull()
  expect(errors).toEqual([])
  expect(fetchTokenPlanUsage).not.toHaveBeenCalled()
})
