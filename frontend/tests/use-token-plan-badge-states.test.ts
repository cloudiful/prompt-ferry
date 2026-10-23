import { afterEach, beforeEach, expect, jest, mock, test } from 'bun:test'
import type {
  DeepSeekBalance,
  TokenPlanQuotaState,
  TokenPlanUsageResponse,
} from '../src/generated/admin-api'
import {
  createEndpointsApiStub,
  createFetchTokenPlanUsageMock,
  createStorageStub,
  createTranslateStub,
  resetFetchTokenPlanUsageMock,
} from './helpers/token-plan-test-setup'

const { localStorage } = createStorageStub()
Object.defineProperty(globalThis, 'localStorage', {
  value: localStorage,
  configurable: true,
})

const fetchTokenPlanUsage = createFetchTokenPlanUsageMock()

mock.module('../src/stores/endpoints-api', () =>
  createEndpointsApiStub(fetchTokenPlanUsage),
)

const {
  __resetTokenPlanCacheForTests,
  getTokenPlanUsageSnapshot,
  prefetchTokenPlanUsage,
  TOKEN_PLAN_CACHE_TTL_MS,
} = await import('../src/composables/useTokenPlanUsageCache')
const { tokenPlanBadgePills, useTokenPlanBadges } =
  await import('../src/composables/useTokenPlanBadges')
const { useEndpointTokenPlanUsage } =
  await import('../src/composables/useEndpointTokenPlanUsage')

const t = createTranslateStub()

beforeEach(() => {
  __resetTokenPlanCacheForTests()
  resetFetchTokenPlanUsageMock(fetchTokenPlanUsage)
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

function windowUsage(
  state: TokenPlanQuotaState,
  cooldownUntil: string | null = null,
): TokenPlanUsageResponse {
  return {
    provider: 'minimax',
    provider_region: null,
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        quota_status: {
          state,
          scope: 'text_inference',
          cooldown_until: cooldownUntil,
        },
        model_remains: [
          {
            model_name: 'General',
            interval: { remaining_percent: 55 },
            weekly: { remaining_percent: 35 },
          },
        ],
      },
    ],
  }
}

test('a transient block keeps the window percentages and names its cause', async () => {
  for (const [state, label] of [
    ['rate_limited', 'tokenPlanRateLimited'],
    ['upstream_unavailable', 'tokenPlanUpstreamUnavailable'],
  ] as const) {
    const badges = await badgesFor(`ep-${state}`, windowUsage(state))
    expect(badges.quotaState).toBe(state)
    expect(tokenPlanBadgePills(badges, t).map((pill) => pill.label)).toEqual([
      'tokenPlanShortBadge 55%',
      'tokenPlanLongBadge 35%',
      label,
    ])
  }
})

test('a confirmed exhaustion still shows only the cooling pill', async () => {
  const badges = await badgesFor('ep-cooling', windowUsage('cooling'))
  expect(tokenPlanBadgePills(badges, t).map((pill) => pill.label)).toEqual([
    'tokenPlanQuotaCooling',
  ])
})

test('a blocked badge names when routing retries', async () => {
  const deadline = '2099-10-11T10:01:00.000Z'
  const badges = await badgesFor(
    'ep-deadline',
    windowUsage('rate_limited', deadline),
  )
  expect(badges.cooldownUntil).toBe(deadline)
  const pills = tokenPlanBadgePills(badges, t)
  expect(pills.at(-1)?.title).toContain('quotaRetryAt')
  // Without a deadline the pill keeps its own label as the whole tooltip.
  const bare = tokenPlanBadgePills(
    await badgesFor('ep-no-deadline', windowUsage('rate_limited')),
    t,
  )
  expect(bare.at(-1)?.title).toBe('tokenPlanRateLimited')
})

test('an elapsed deadline drops the transient badge until a refresh', async () => {
  const start = new Date('2026-10-08T00:00:00.000Z')
  jest.setSystemTime(start)
  const badges = await badgesFor(
    'ep-expired',
    windowUsage(
      'rate_limited',
      new Date(start.getTime() + 1_000).toISOString(),
    ),
  )
  expect(badges.quotaState).toBe('rate_limited')

  jest.setSystemTime(new Date(start.getTime() + 2_000))
  const elapsed = useTokenPlanBadges('ep-expired').value
  expect(elapsed.quotaState).toBe('unknown')
  expect(elapsed.short).toBeNull()
  expect(elapsed.cooldownUntil).toBeNull()
  jest.setSystemTime()
})

test('the detail view demotes an elapsed deadline even on a ready cache entry', async () => {
  const start = new Date('2026-10-08T00:00:00.000Z')
  jest.setSystemTime(start)
  const findEndpointById = () => ({
    provider: 'minimax',
    endpoint_key_lb_enabled: true,
  })
  const errors: unknown[] = []
  const detail = useEndpointTokenPlanUsage(
    findEndpointById as never,
    (cause: unknown) => errors.push(cause),
  )
  const feedbackKey = (
    state: 'cooling' | 'rate_limited' | 'upstream_unavailable',
    deadline: string,
  ) => ({
    key_id: `k-${state}`,
    key_label: state,
    ok: true,
    model_remains: [],
    quota_status: { state, scope: 'text_inference', cooldown_until: deadline },
  })

  // A ready cached payload whose feedback deadline has already elapsed. The
  // detail surface must not keep naming it as a live block.
  const elapsed = new Date(start.getTime() - 1_000).toISOString()
  const active = new Date(start.getTime() + 60_000).toISOString()
  fetchTokenPlanUsage.mockResolvedValue({
    provider: 'minimax',
    provider_region: null,
    keys: [
      feedbackKey('rate_limited', elapsed),
      feedbackKey('upstream_unavailable', elapsed),
      feedbackKey('cooling', elapsed),
    ],
  })
  await detail.openTokenPlanUsage('ep-detail-ready')
  const shown = detail.tokenPlanUsage.value!.keys.map(
    (key) => key.quota_status?.state,
  )
  expect(shown).toEqual(['unknown', 'unknown', 'unknown'])
  expect(
    detail.tokenPlanUsage.value!.keys.every(
      (key) => key.quota_status?.cooldown_until === null,
    ),
  ).toBe(true)

  // A deadline still in the future stays its true state in the same surface.
  fetchTokenPlanUsage.mockResolvedValue({
    provider: 'minimax',
    provider_region: null,
    keys: [
      feedbackKey('rate_limited', active),
      feedbackKey('upstream_unavailable', active),
      feedbackKey('cooling', active),
    ],
  })
  await detail.openTokenPlanUsage('ep-detail-active')
  expect(
    detail.tokenPlanUsage.value!.keys.map((key) => key.quota_status?.state),
  ).toEqual(['rate_limited', 'upstream_unavailable', 'cooling'])
  expect(
    detail.tokenPlanUsage.value!.keys.every(
      (key) => key.quota_status?.cooldown_until === active,
    ),
  ).toBe(true)
  expect(errors).toEqual([])
  jest.setSystemTime()
})

test('a stale detail payload keeps live feedback and demotes only expired feedback', async () => {
  const start = new Date('2026-10-08T00:00:00.000Z')
  jest.setSystemTime(start)
  const states = ['cooling', 'rate_limited', 'upstream_unavailable'] as const
  const usageWith = (deadline: string) => ({
    provider: 'minimax' as const,
    provider_region: null,
    keys: states.map((state) => ({
      key_id: state,
      key_label: state,
      ok: true,
      model_remains: [],
      quota_status: {
        state,
        scope: 'text_inference',
        cooldown_until: deadline,
      },
    })),
  })
  const errors: unknown[] = []
  const detail = useEndpointTokenPlanUsage(
    (() => ({ provider: 'minimax', endpoint_key_lb_enabled: true })) as never,
    (cause: unknown) => errors.push(cause),
  )
  const openStale = async (endpointId: string, deadline: string) => {
    // Warm a ready entry, let the TTL lapse, then make the revalidation fail:
    // the detail surface now serves the last good payload as `stale`.
    fetchTokenPlanUsage.mockResolvedValue(usageWith(deadline))
    await prefetchTokenPlanUsage(endpointId)
    jest.setSystemTime(new Date(start.getTime() + TOKEN_PLAN_CACHE_TTL_MS + 1))
    fetchTokenPlanUsage.mockRejectedValueOnce(new Error('usage 503'))
    await detail.openTokenPlanUsage(endpointId)
    // The payload really is stale: the revalidation failed and the cache
    // replays its last good entry.
    expect(getTokenPlanUsageSnapshot(endpointId).state).toBe('stale')
    jest.setSystemTime(start)
    return detail.tokenPlanUsage.value!.keys
  }

  const active = new Date(start.getTime() + 60 * 60_000).toISOString()
  const live = await openStale('ep-detail-stale-live', active)
  // A live feedback block is not a provider observation: a stale payload keeps
  // every cause and its deadline.
  expect(live.map((key) => key.quota_status?.state)).toEqual([
    'cooling',
    'rate_limited',
    'upstream_unavailable',
  ])
  expect(live.every((key) => key.quota_status?.cooldown_until === active)).toBe(
    true,
  )

  const elapsed = await openStale(
    'ep-detail-stale-expired',
    new Date(start.getTime() - 1_000).toISOString(),
  )
  // An elapsed deadline is demoted even on a stale payload.
  expect(elapsed.map((key) => key.quota_status?.state)).toEqual([
    'unknown',
    'unknown',
    'unknown',
  ])
  expect(
    elapsed.every((key) => key.quota_status?.cooldown_until === null),
  ).toBe(true)
  // A failed revalidation is replayed silently, so the badge is not the place
  // that reports it and neither open raised an error.
  expect(errors).toEqual([])
  jest.setSystemTime()
})

test('a refresh failure preserves raw data but marks the cached quota stale', async () => {
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
  // fail: preserve the raw snapshot without presenting its quota as current.
  jest.setSystemTime(new Date(start.getTime() + TOKEN_PLAN_CACHE_TTL_MS + 1))
  fetchTokenPlanUsage.mockRejectedValueOnce(new Error('usage 503'))
  await prefetchTokenPlanUsage('ep-swr-error')
  await new Promise<void>((resolve) => setTimeout(resolve, 0))

  const badges = useTokenPlanBadges('ep-swr-error').value
  expect(badges.status).toBe('stale')
  expect(badges.deepseekState).toBe('stale')
  // Stale retains the last-known balance alongside the explicit unknown
  // hint, never the amount as fresh capacity.
  expect(badges.deepseekBalances).toEqual([{ currency: 'CNY', total: 3 }])
  // Stale retains the last-known balance; the generic quota placeholder is
  // not shown next to it.
  expect(tokenPlanBadgePills(badges, t).map((pill) => pill.label)).toEqual([
    '¥3.00',
  ])
  expect(badges.usage).toEqual(usage)
  jest.setSystemTime()
})

test('a stale exhausted window keeps actual numbers with the unknown title', async () => {
  const { minimaxQuotaKey } =
    await import('./helpers/token-plan-quota-fixtures')
  const usage = {
    provider: 'minimax' as const,
    provider_region: null,
    keys: [minimaxQuotaKey('stale-exhausted', 'exhausted', 0, 40)],
  }
  const badges = await badgesFor('ep-stale-exhausted', usage)
  // Warm as ready first so the failure path below replays real cache state.
  expect(badges.status).toBe('ready')

  jest.setSystemTime(new Date(Date.now() + TOKEN_PLAN_CACHE_TTL_MS + 1))
  fetchTokenPlanUsage.mockRejectedValueOnce(new Error('usage 503'))
  await prefetchTokenPlanUsage('ep-stale-exhausted')
  await new Promise<void>((resolve) => setTimeout(resolve, 0))

  const stale = useTokenPlanBadges('ep-stale-exhausted').value
  expect(stale.status).toBe('stale')
  expect(stale.quotaState).toBe('unknown')
  // Historical 0/40 stays visible instead of a fabricated current 0/0.
  expect([stale.short, stale.long]).toEqual([0, 40])
  const pills = tokenPlanBadgePills(stale, t)
  expect(pills.map((pill) => pill.label)).toEqual([
    'tokenPlanShortBadge 0%',
    'tokenPlanLongBadge 40%',
  ])
  // Every retained percentage carries the unknown tooltip, never a fresh hint.
  expect(
    pills
      .filter((pill) => pill.label.includes('tokenPlanShortBadge'))
      .every((pill) => pill.title === 'tokenPlanQuotaUnknown'),
  ).toBe(true)
  jest.setSystemTime()
})

test('a ready exhausted window keeps the existing zero control', async () => {
  const { minimaxQuotaKey } =
    await import('./helpers/token-plan-quota-fixtures')
  const badges = await badgesFor('ep-ready-exhausted', {
    provider: 'minimax',
    provider_region: null,
    keys: [minimaxQuotaKey('ready-exhausted', 'exhausted', 0, 40)],
  })
  expect(badges.quotaState).toBe('exhausted')
  expect([badges.short, badges.long]).toEqual([0, 0])
})

test('stale expired feedback keeps numbers with the unknown title', async () => {
  const { minimaxQuotaKey } =
    await import('./helpers/token-plan-quota-fixtures')
  const { summarizeTokenPlanQuota } =
    await import('../src/composables/tokenPlanQuotaSummary')
  const expired = new Date(Date.now() - 60_000).toISOString()
  for (const state of [
    'cooling',
    'rate_limited',
    'upstream_unavailable',
  ] as const) {
    const key = minimaxQuotaKey(`stale-expired-${state}`, state, 60, 40)
    key.quota_status = {
      state,
      scope: 'text_inference',
      cooldown_until: expired,
    }
    const summary = summarizeTokenPlanQuota('minimax', [key], 'stale')
    expect(summary.state).toBe('unknown')
    expect([summary.short, summary.long]).toEqual([60, 40])
    const pills = tokenPlanBadgePills(
      {
        mode: 'window',
        status: 'stale',
        quotaState: summary.state,
        quotaPartial: summary.partial,
        quotaMetadataPresent: true,
        short: summary.short,
        long: summary.long,
        openrouterBalance: null,
        openrouterRemaining: null,
        openrouterDailySpend: null,
        deepseekBalances: [],
        deepseekAvailable: null,
        deepseekState: 'unknown',
        localTodayTokens: null,
        commandCodeExhausted: false,
        cooldownUntil: summary.cooldownUntil,
      },
      t,
    )
    expect(pills.map((pill) => pill.label)).toEqual([
      'tokenPlanShortBadge 60%',
      'tokenPlanLongBadge 40%',
    ])
  }
})
