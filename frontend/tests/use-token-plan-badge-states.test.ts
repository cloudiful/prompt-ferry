import { afterEach, beforeEach, expect, jest, mock, test } from 'bun:test'
import type {
  DeepSeekBalance,
  TokenPlanUsageResponse,
} from '../src/generated/admin-api'

const storage = new Map<string, string>()
Object.defineProperty(globalThis, 'localStorage', {
  value: {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  },
  configurable: true,
})

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
  fetchOrganizationUsage: mock(async () => undefined),
  fetchTokenPlanUsage,
  listEndpoints: mock(async () => undefined),
  listModelRoutes: mock(async () => undefined),
  modelRouteFormToRequest: mock((route: unknown) => route),
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
  getTokenPlanUsageSnapshot,
  prefetchTokenPlanUsage,
  TOKEN_PLAN_CACHE_TTL_MS,
} = await import('../src/composables/useTokenPlanUsageCache')
const { tokenPlanBadgePills, useTokenPlanBadges } =
  await import('../src/composables/useTokenPlanBadges')

const t = ((key: string) => key) as unknown as TranslateFn

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

function deepseekUsage(
  deepseekBalance: DeepSeekBalance,
  localTodayTokens?: number,
): TokenPlanUsageResponse {
  return {
    provider: 'deepseek',
    provider_region: null,
    ...(localTodayTokens === undefined
      ? {}
      : { local_today_tokens: localTodayTokens }),
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [],
        deepseek_balance: deepseekBalance,
      },
    ],
  }
}

async function badgesFor(endpointId: string, usage: TokenPlanUsageResponse) {
  fetchTokenPlanUsage.mockResolvedValueOnce(usage)
  await prefetchTokenPlanUsage(endpointId)
  return useTokenPlanBadges(endpointId).value
}

test('every known currency gets its own pill in the reported order', async () => {
  const badges = await badgesFor(
    'ep-ds-multi',
    deepseekUsage({
      is_available: true,
      balances: [
        { currency: 'CNY', total_balance: 110.5 },
        { currency: 'USD', total_balance: 12.34 },
        { currency: 'EUR', total_balance: null },
      ],
    }),
  )

  expect(badges.deepseekBalances).toEqual([
    { currency: 'CNY', total: 110.5 },
    { currency: 'USD', total: 12.34 },
    { currency: 'EUR', total: null },
  ])
  expect(badges.deepseekState).toBe('ready')
  // Two currencies, two pills: no FX conversion, no cross-currency sum, and
  // the unknown entry is named rather than dropped or shown as zero.
  expect(tokenPlanBadgePills(badges, t).map((pill) => pill.label)).toEqual([
    '¥110.50',
    '$12.34',
    'tokenPlanBalanceUnknown',
  ])
})

test('a real zero stays a zero next to the other currencies', async () => {
  const badges = await badgesFor(
    'ep-ds-zero',
    deepseekUsage({
      is_available: true,
      balances: [
        { currency: 'CNY', total_balance: 0 },
        { currency: 'USD', total_balance: 8 },
      ],
    }),
  )

  expect(tokenPlanBadgePills(badges, t).map((pill) => pill.label)).toEqual([
    '¥0.00',
    '$8.00',
  ])
  // A reported zero is known data, so no unknown pill is appended.
  expect(tokenPlanBadgePills(badges, t)).toHaveLength(2)
})

test('an all-unknown balance names the state instead of rendering an amount', async () => {
  const badges = await badgesFor(
    'ep-ds-unknown',
    deepseekUsage({
      is_available: true,
      balances: [
        { currency: 'CNY', total_balance: null },
        { currency: 'USD', granted_balance: 5, total_balance: null },
      ],
    }),
  )

  expect(badges.deepseekState).toBe('unknown')
  const labels = tokenPlanBadgePills(badges, t).map((pill) => pill.label)
  expect(labels).toEqual(['tokenPlanBalanceUnknown'])
  expect(labels.some((label) => label.includes('0.00'))).toBe(false)
})

test('an account that reports no balance entry at all stays unknown', async () => {
  const badges = await badgesFor(
    'ep-ds-absent',
    deepseekUsage({ is_available: true, balances: [] }, 42),
  )

  expect(badges.deepseekAvailable).toBe(true)
  expect(badges.deepseekState).toBe('unknown')
  expect(tokenPlanBadgePills(badges, t).map((pill) => pill.label)).toEqual([
    'tokenPlanBalanceUnknown',
    'tokenPlanLocalTodayTokens 42',
  ])
})

test('is_available colors the pills without hiding a reported amount', async () => {
  const badges = await badgesFor(
    'ep-ds-unavailable',
    deepseekUsage({
      is_available: false,
      balances: [{ currency: 'USD', total_balance: 0 }],
    }),
  )

  expect(badges.deepseekState).toBe('unavailable')
  const pills = tokenPlanBadgePills(badges, t)
  expect(pills.map((pill) => pill.label)).toEqual(['$0.00'])
  expect(pills[0]?.color).toBe('hsl(0 80% 45%)')
})

test('an unavailable account with no known amount shows the unavailable state', async () => {
  const badges = await badgesFor(
    'ep-ds-unavailable-unknown',
    deepseekUsage({
      is_available: false,
      balances: [{ currency: 'USD', total_balance: null }],
    }),
  )

  expect(badges.deepseekState).toBe('unavailable')
  expect(tokenPlanBadgePills(badges, t).map((pill) => pill.label)).toEqual([
    'tokenPlanUnavailable',
  ])
})

test('a key reporting a known amount wins over one whose amounts are unknown', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce({
    provider: 'deepseek',
    provider_region: null,
    keys: [
      {
        key_id: 'k-unknown',
        key_label: 'k-unknown',
        ok: true,
        model_remains: [],
        deepseek_balance: {
          is_available: true,
          balances: [{ currency: 'CNY', total_balance: null }],
        },
      },
      {
        key_id: 'k-known',
        key_label: 'k-known',
        ok: true,
        model_remains: [],
        deepseek_balance: {
          is_available: true,
          balances: [{ currency: 'CNY', total_balance: 7 }],
        },
      },
    ],
  })
  await prefetchTokenPlanUsage('ep-ds-keys')
  const badges = useTokenPlanBadges('ep-ds-keys').value

  expect(badges.deepseekBalances).toEqual([{ currency: 'CNY', total: 7 }])
  expect(tokenPlanBadgePills(badges, t).map((pill) => pill.label)).toEqual([
    '¥7.00',
  ])
})

test('a cold cache reads as loading, not as ready data', () => {
  let release!: (usage: TokenPlanUsageResponse) => void
  fetchTokenPlanUsage.mockImplementationOnce(
    () =>
      new Promise<TokenPlanUsageResponse>((resolve) => {
        release = resolve
      }),
  )

  // The composable kicks the prefetch on setup, so the snapshot is still cold
  // until that request settles.
  const badges = useTokenPlanBadges('ep-cold')
  expect(getTokenPlanUsageSnapshot('ep-cold')).toEqual({
    state: 'loading',
    usage: null,
  })
  expect(badges.value.status).toBe('loading')
  expect(badges.value.deepseekState).toBe('loading')
  // A payload-less badge has no provider yet, so it emits no pill at all and
  // the row keeps its placeholder instead of a fabricated amount.
  expect(tokenPlanBadgePills(badges.value, t)).toEqual([])
  release({
    keys: [],
    provider: 'minimax',
    provider_region: null,
  })
})

test('a failed cold fetch reads as an error instead of a ready zero', async () => {
  fetchTokenPlanUsage.mockRejectedValueOnce(new Error('usage 503'))

  await prefetchTokenPlanUsage('ep-error')

  expect(getTokenPlanUsageSnapshot('ep-error')).toEqual({
    state: 'error',
    usage: null,
  })
  const badges = useTokenPlanBadges('ep-error').value
  expect(badges.status).toBe('error')
  expect(badges.deepseekState).toBe('error')
  expect(badges.usage).toBeNull()
  const labels = tokenPlanBadgePills(badges, t).map((pill) => pill.label)
  expect(labels).toEqual([])
  expect(labels.some((label) => label.includes('0.00'))).toBe(false)
})

test('a refresh failure keeps the last ready snapshot ready', async () => {
  const start = new Date('2026-10-08T00:00:00.000Z')
  jest.setSystemTime(start)
  const usage = deepseekUsage({
    is_available: true,
    balances: [{ currency: 'CNY', total_balance: 3 }],
  })
  fetchTokenPlanUsage.mockResolvedValueOnce(usage)
  await prefetchTokenPlanUsage('ep-swr-error')
  expect(getTokenPlanUsageSnapshot('ep-swr-error').state).toBe('ready')

  // Cross the TTL so the prefetch really re-fetches, then let that refresh
  // fail: the entry keeps its snapshot, so the badge must stay ready instead
  // of dropping into an error state that hides real numbers.
  jest.setSystemTime(new Date(start.getTime() + TOKEN_PLAN_CACHE_TTL_MS + 1))
  fetchTokenPlanUsage.mockRejectedValueOnce(new Error('usage 503'))
  await prefetchTokenPlanUsage('ep-swr-error')
  await new Promise<void>((resolve) => setTimeout(resolve, 0))

  const badges = useTokenPlanBadges('ep-swr-error').value
  expect(badges.status).toBe('ready')
  expect(badges.deepseekState).toBe('ready')
  expect(tokenPlanBadgePills(badges, t).map((pill) => pill.label)).toEqual([
    '¥3.00',
  ])
  jest.setSystemTime()
})
