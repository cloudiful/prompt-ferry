import { afterEach, beforeEach, expect, mock, test } from 'bun:test'
import { computed, nextTick, ref } from 'vue'
import type { TokenPlanUsageResponse } from '../src/generated/admin-api'

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

const { __resetTokenPlanCacheForTests, prefetchTokenPlanUsage } =
  await import('../src/composables/useTokenPlanUsageCache')
const { tokenPlanBadgePills, useTokenPlanBadges } =
  await import('../src/composables/useTokenPlanBadges')

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

test('empty usage renders empty badges', () => {
  const badges = useTokenPlanBadges('ep-empty')
  expect(badges.value).toEqual({
    mode: 'window',
    short: null,
    long: null,
    openrouterBalance: null,
    openrouterRemaining: null,
    openrouterDailySpend: null,
    deepseekTotal: null,
    deepseekCurrency: null,
    deepseekAvailable: null,
    localTodayTokens: null,
    usage: null,
  })
})

test('MiniMax windows: short/long are the arithmetic mean across keys', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce({
    provider: 'minimax',
    provider_region: null,
    keys: [
      {
        key_id: 'k1',
        key_label: 'k1',
        ok: true,
        model_remains: [
          {
            model_name: 'g',
            interval: { remaining_percent: 50 },
            weekly: { remaining_percent: 80 },
          },
        ],
      },
      {
        key_id: 'k2',
        key_label: 'k2',
        ok: true,
        model_remains: [
          {
            model_name: 'g',
            interval: { remaining_percent: 30 },
            weekly: { remaining_percent: 60 },
          },
        ],
      },
    ],
  })
  await prefetchTokenPlanUsage('ep-mm')
  const badges = useTokenPlanBadges('ep-mm')
  // mean(50, 30) = 40, mean(80, 60) = 70 — no longer the worst-case min.
  expect(badges.value.short).toBe(40)
  expect(badges.value.long).toBe(70)
  expect(badges.value.mode).toBe('window')
  expect(badges.value.openrouterRemaining).toBeNull()
})

test('keys without a window are skipped, not averaged as zero', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce({
    provider: 'minimax',
    provider_region: null,
    keys: [
      {
        key_id: 'k1',
        key_label: 'k1',
        ok: true,
        model_remains: [
          {
            model_name: 'g',
            interval: { remaining_percent: 60 },
            weekly: { remaining_percent: 90 },
          },
        ],
      },
      {
        key_id: 'k2',
        key_label: 'k2',
        ok: true,
        model_remains: [],
      },
    ],
  })
  await prefetchTokenPlanUsage('ep-mm-skip')
  const badges = useTokenPlanBadges('ep-mm-skip')
  expect(badges.value.short).toBe(60)
  expect(badges.value.long).toBe(90)
})

test('CommandCode USD: short = five_hour, long = weekly', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce({
    provider: 'command_code',
    provider_region: null,
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [],
        five_hour: { remaining_percent: 75, reset_at: null },
        weekly: { remaining_percent: 40, reset_at: null },
      },
    ],
  })
  await prefetchTokenPlanUsage('ep-cc')
  const badges = useTokenPlanBadges('ep-cc')
  expect(badges.value.short).toBe(75)
  expect(badges.value.long).toBe(40)
})

test('OpencodeGo uses USED percent: short = 100 - rolling.used, long = min(100 - weekly.used, 100 - monthly.used)', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce({
    provider: 'opencode_go',
    provider_region: null,
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [],
        opencodego_rolling: { percent: 30, resets_at: null },
        opencodego_weekly: { percent: 40, resets_at: null },
        opencodego_monthly: { percent: 20, resets_at: null },
      },
    ],
  })
  await prefetchTokenPlanUsage('ep-ocg')
  const badges = useTokenPlanBadges('ep-ocg')
  expect(badges.value.short).toBe(70)
  expect(badges.value.long).toBe(60) // min(60, 80)
})

test('GLM Coding Plan uses USED percent: short = 100 - glm_five_hour.percentage, long = 100 - glm_weekly.percentage', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce({
    provider: 'glm',
    provider_region: null,
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [],
        glm_five_hour: {
          percentage: 25,
          current_value: 1,
          limit: 4,
          next_reset_at: null,
        },
        glm_weekly: {
          percentage: 60,
          current_value: 6,
          limit: 10,
          next_reset_at: null,
        },
      },
    ],
  })
  await prefetchTokenPlanUsage('ep-glm')
  const badges = useTokenPlanBadges('ep-glm')
  expect(badges.value.short).toBe(75)
  expect(badges.value.long).toBe(40)
})

test('OpenRouter: balance + remaining ratio + provider daily spend', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce({
    provider: 'openrouter',
    provider_region: null,
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [],
        openrouter_balance: {
          limit: 100,
          limit_remaining: 42.5,
          limit_reset: null,
          is_free_tier: false,
          total_credits: null,
          total_usage: null,
        },
        openrouter_spend: {
          usage: 10,
          daily: 1.5,
          weekly: 6,
          monthly: 9,
        },
      },
    ],
  })
  await prefetchTokenPlanUsage('ep-or')
  const badges = useTokenPlanBadges('ep-or')
  expect(badges.value.mode).toBe('openrouter')
  expect(badges.value.short).toBeNull()
  expect(badges.value.long).toBeNull()
  expect(badges.value.openrouterBalance).toBe(42.5)
  expect(badges.value.openrouterRemaining).toBeCloseTo(42.5, 5)
  expect(badges.value.openrouterDailySpend).toBe(1.5)
})

test('OpenRouter without a limit falls back to the credit totals difference', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce({
    provider: 'openrouter',
    provider_region: null,
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [],
        openrouter_balance: {
          limit: null,
          limit_remaining: null,
          limit_reset: null,
          is_free_tier: false,
          total_credits: 100,
          total_usage: 25.5,
        },
      },
    ],
  })
  await prefetchTokenPlanUsage('ep-or-credits')
  const badges = useTokenPlanBadges('ep-or-credits')
  expect(badges.value.openrouterBalance).toBe(74.5)
  expect(badges.value.openrouterRemaining).toBeNull()
})

test('OpenRouter with no credit signal keeps null remaining and balance', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce({
    provider: 'openrouter',
    provider_region: null,
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [],
        openrouter_balance: {
          limit: null,
          limit_remaining: null,
          limit_reset: null,
          is_free_tier: true,
          total_credits: null,
          total_usage: null,
        },
      },
    ],
  })
  await prefetchTokenPlanUsage('ep-or-free')
  const badges = useTokenPlanBadges('ep-or-free')
  expect(badges.value.short).toBeNull()
  expect(badges.value.long).toBeNull()
  expect(badges.value.openrouterRemaining).toBeNull()
  expect(badges.value.openrouterBalance).toBeNull()
})

test('DeepSeek exposes the balance and the backend local today tokens', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce({
    provider: 'deepseek',
    provider_region: null,
    local_today_tokens: 1234,
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [],
        deepseek_balance: {
          is_available: true,
          currency: 'CNY',
          total_balance: 110,
          granted_balance: 10,
          topped_up_balance: 100,
        },
      },
    ],
  })
  await prefetchTokenPlanUsage('ep-ds')
  const badges = useTokenPlanBadges('ep-ds')
  expect(badges.value.mode).toBe('deepseek')
  expect(badges.value.deepseekTotal).toBe(110)
  expect(badges.value.deepseekCurrency).toBe('CNY')
  expect(badges.value.deepseekAvailable).toBe(true)
  expect(badges.value.localTodayTokens).toBe(1234)
})

test('non-ok keys are skipped when aggregating', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce({
    provider: 'minimax',
    provider_region: null,
    keys: [
      {
        key_id: 'k1',
        key_label: 'k1',
        ok: false,
        error_message: 'rate limited',
        model_remains: [
          {
            model_name: 'g',
            interval: { remaining_percent: 5 },
            weekly: { remaining_percent: 5 },
          },
        ],
      },
    ],
  })
  await prefetchTokenPlanUsage('ep-err')
  const badges = useTokenPlanBadges('ep-err')
  expect(badges.value.short).toBeNull()
  expect(badges.value.long).toBeNull()
})

test('useTokenPlanBadges reacts when the cache is populated after mount', async () => {
  // The composable auto-prefetches on setup, so configure the mock
  // *before* invoking it. We then explicitly await it so the test
  // doesn't race the auto-prefetch (it would coalesce onto the same
  // in-flight promise and return only after resolution).
  const populated = {
    provider: 'minimax' as const,
    provider_region: null,
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [
          {
            model_name: 'g',
            interval: { remaining_percent: 91 },
            weekly: { remaining_percent: 92 },
          },
        ],
      },
    ],
  }
  fetchTokenPlanUsage.mockReset()
  fetchTokenPlanUsage.mockImplementation(async () => populated)

  const badges = useTokenPlanBadges('ep-late')
  // Await the in-flight prefetch so the reactive cache update has
  // landed before the first computed read.
  await prefetchTokenPlanUsage('ep-late')
  expect(badges.value.short).toBe(91)
  expect(badges.value.long).toBe(92)
})

// The inline badge subcomponents (`EndpointUsageBadges`,
// `EndpointMobileUsageBadges`) wrap `props.endpointId` in a `computed`
// before handing it to `useTokenPlanBadges`. The composable must track
// that ref so the badges re-evaluate when the row's endpoint id swaps
// underneath the same mounted component (e.g. after an inline edit,
// when UTable's `getRowId` keeps the component instance alive).
test('useTokenPlanBadges tracks a computed ref of endpointId across swaps', async () => {
  const epA: TokenPlanUsageResponse = {
    provider: 'minimax',
    provider_region: null,
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [
          {
            model_name: 'g',
            interval: { remaining_percent: 50 },
            weekly: { remaining_percent: 60 },
          },
        ],
      },
    ],
  }
  const epB: TokenPlanUsageResponse = {
    provider: 'command_code',
    provider_region: null,
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [],
        five_hour: { remaining_percent: 11, reset_at: null },
        weekly: { remaining_percent: 22, reset_at: null },
      },
    ],
  }
  fetchTokenPlanUsage.mockImplementation(async (id: string) => {
    if (id === 'ep-a') return epA
    if (id === 'ep-b') return epB
    throw new Error(`unexpected id ${id}`)
  })

  const idRef = ref('ep-a')
  const badges = useTokenPlanBadges(computed(() => idRef.value))
  // Drain the auto-prefetch so the first endpoint is cached.
  await prefetchTokenPlanUsage('ep-a')

  expect(badges.value.short).toBe(50)
  expect(badges.value.long).toBe(60)
  expect(badges.value.usage?.provider).toBe('minimax')

  // Swap to a different endpoint without remounting. A reactive `props`
  // proxy on the consumer side drives this — the composable must pick
  // up the change because it received a computed ref, not a static
  // string snapshot.
  idRef.value = 'ep-b'
  await prefetchTokenPlanUsage('ep-b')
  await nextTick()

  expect(badges.value.short).toBe(11)
  expect(badges.value.long).toBe(22)
  expect(badges.value.usage?.provider).toBe('command_code')
})

test('tokenPlanBadgePills derives the static quota pill pair', () => {
  const t = ((key: string) => key) as unknown as TranslateFn
  const pills = tokenPlanBadgePills(
    {
      mode: 'window',
      short: 40,
      long: 70,
      openrouterBalance: null,
      openrouterRemaining: null,
      openrouterDailySpend: null,
      deepseekTotal: null,
      deepseekCurrency: null,
      deepseekAvailable: null,
      localTodayTokens: null,
    },
    t,
  )
  expect(pills.map((pill) => pill.label)).toEqual([
    'tokenPlanShortBadge 40%',
    'tokenPlanLongBadge 70%',
  ])
})

test('tokenPlanBadgePills pairs the OpenRouter balance with provider spend', () => {
  const t = ((key: string) => key) as unknown as TranslateFn
  const pills = tokenPlanBadgePills(
    {
      mode: 'openrouter',
      short: null,
      long: null,
      openrouterBalance: 42.5,
      openrouterRemaining: 42.5,
      openrouterDailySpend: 1.5,
      deepseekTotal: null,
      deepseekCurrency: null,
      deepseekAvailable: null,
      localTodayTokens: null,
    },
    t,
  )
  expect(pills.map((pill) => pill.label)).toEqual([
    'tokenPlanOpenRouterRemaining $42.50',
    'tokenPlanSpendDaily $1.50',
  ])
})

test('tokenPlanBadgePills falls back to local tokens when OpenRouter has no spend', () => {
  const t = ((key: string) => key) as unknown as TranslateFn
  const pills = tokenPlanBadgePills(
    {
      mode: 'openrouter',
      short: null,
      long: null,
      openrouterBalance: null,
      openrouterRemaining: null,
      openrouterDailySpend: null,
      deepseekTotal: null,
      deepseekCurrency: null,
      deepseekAvailable: null,
      localTodayTokens: 0,
    },
    t,
  )
  expect(pills.map((pill) => pill.label)).toEqual([
    'tokenPlanOpenRouterRemaining tokenPlanNoQuota',
    'tokenPlanLocalTodayTokens 0',
  ])
})

test('tokenPlanBadgePills pairs the DeepSeek balance with local today tokens', () => {
  const t = ((key: string) => key) as unknown as TranslateFn
  const available = tokenPlanBadgePills(
    {
      mode: 'deepseek',
      short: null,
      long: null,
      openrouterBalance: null,
      openrouterRemaining: null,
      openrouterDailySpend: null,
      deepseekTotal: 110,
      deepseekCurrency: 'CNY',
      deepseekAvailable: true,
      localTodayTokens: 1234,
    },
    t,
  )
  expect(available.map((pill) => pill.label)).toEqual([
    'tokenPlanDeepSeekBalance ¥110.00',
    'tokenPlanLocalTodayTokens 1.2K',
  ])
  const unavailable = tokenPlanBadgePills(
    {
      mode: 'deepseek',
      short: null,
      long: null,
      openrouterBalance: null,
      openrouterRemaining: null,
      openrouterDailySpend: null,
      deepseekTotal: 0,
      deepseekCurrency: 'USD',
      deepseekAvailable: false,
      localTodayTokens: 0,
    },
    t,
  )
  expect(unavailable.map((pill) => pill.label)).toEqual([
    'tokenPlanDeepSeekBalance $0.00',
    'tokenPlanLocalTodayTokens 0',
  ])
})
