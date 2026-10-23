import { afterEach, beforeEach, expect, mock, test } from 'bun:test'
import { createSSRApp, h } from 'vue'
import { renderToString } from 'vue/server-renderer'
import type {
  TokenPlanKeyUsage,
  TokenPlanUsageResponse,
} from '../src/generated/admin-api'
import {
  createEndpointsApiStub,
  createFetchTokenPlanUsageMock,
  createStorageStub,
  createTranslateStub,
  resetFetchTokenPlanUsageMock,
} from './helpers/token-plan-test-setup'

// Issue #1049 P1: a subscription quota that is actually reported must be shown
// even when the coarse default-text status is unknown, and the generic
// "default text quota unknown" placeholder must never reach the endpoint
// surfaces. These regressions drive the real composables and the shared
// table/card renderer with realistic provider payloads.

const { localStorage } = createStorageStub()
Object.defineProperty(globalThis, 'localStorage', {
  value: localStorage,
  configurable: true,
})

const fetchTokenPlanUsage = createFetchTokenPlanUsageMock()

mock.module('../src/stores/endpoints-api', () =>
  createEndpointsApiStub(fetchTokenPlanUsage),
)

const { __resetTokenPlanCacheForTests, prefetchTokenPlanUsage } =
  await import('../src/composables/useTokenPlanUsageCache')
const { tokenPlanBadgePills, useTokenPlanBadges } =
  await import('../src/composables/useTokenPlanBadges')
const { summarizeTokenPlanQuota } =
  await import('../src/composables/tokenPlanQuotaSummary')
const { renderTokenPlanBadges } =
  await import('../src/components/endpoints/tokenPlanBadgeRender')

const t = createTranslateStub()

beforeEach(() => {
  __resetTokenPlanCacheForTests()
  resetFetchTokenPlanUsageMock(fetchTokenPlanUsage)
})

afterEach(() => {
  __resetTokenPlanCacheForTests()
})

// A ChatGPT subscription key whose provider observation is unknown while the
// reported windows still carry real numbers.
function openAiSubscriptionKey(
  id: string,
  windows: {
    source_window: string
    window_seconds: number | null
    remaining_percent: number | null
  }[],
  state: 'unknown' | 'available' = 'unknown',
): TokenPlanKeyUsage {
  return {
    key_id: id,
    key_label: id,
    ok: true,
    quota_status: { state, scope: 'text_inference' },
    model_remains: [
      {
        model_name: 'plus',
        windows: windows.map((window) => ({
          source_window: window.source_window,
          window_seconds: window.window_seconds,
          used_percent:
            window.remaining_percent === null
              ? null
              : 100 - window.remaining_percent,
          remaining_percent: window.remaining_percent,
          reset_at: new Date(Date.now() + 60_000).toISOString(),
          reset_after_seconds: null,
          availability: window.remaining_percent === null ? 'unknown' : 'known',
        })),
      },
    ],
  } as unknown as TokenPlanKeyUsage
}

function openCodeGoKey(
  id: string,
  windows: {
    rolling?: { percent: number | null } | null
    weekly?: { percent: number | null } | null
    monthly?: { percent: number | null } | null
  },
  state: 'unknown' | 'available' = 'unknown',
): TokenPlanKeyUsage {
  const window = (value: { percent: number | null } | null | undefined) =>
    value == null ? null : { percent: value.percent, resets_at: null }
  return {
    key_id: id,
    key_label: id,
    ok: true,
    quota_status: { state, scope: 'text_inference' },
    model_remains: [],
    opencodego_rolling: window(windows.rolling),
    opencodego_weekly: window(windows.weekly),
    opencodego_monthly: window(windows.monthly),
  } as unknown as TokenPlanKeyUsage
}

async function badgesFor(endpointId: string, usage: TokenPlanUsageResponse) {
  fetchTokenPlanUsage.mockResolvedValueOnce(usage)
  await prefetchTokenPlanUsage(endpointId)
  return useTokenPlanBadges(endpointId).value
}

function labels(source: Parameters<typeof tokenPlanBadgePills>[0]): string[] {
  return tokenPlanBadgePills(source, t).map((pill) => pill.label)
}

test('canonical unknown-status windows respect presence, availability and the supplied clock', () => {
  const now = Date.parse('2026-10-23T00:00:00Z')
  const key = openAiSubscriptionKey('canonical', [
    { source_window: 'primary', window_seconds: 18_000, remaining_percent: 99 },
  ])
  const row = {
    source_id: 'subscription:primary',
    kind: 'custom' as const,
    source_model: 'plus',
    duration_seconds: 18_000,
    is_default_scope: true,
    availability: 'known' as const,
    remaining_percent: 0,
    reset_at: new Date(now + 1).toISOString(),
  }
  key.quota_windows = [row]
  for (const fetchState of ['ready', 'stale'] as const) {
    const summary = summarizeTokenPlanQuota('openai', [key], fetchState, now)
    expect([summary.state, summary.short, summary.long]).toEqual([
      'unknown',
      0,
      null,
    ])
    expect(summary.usableKeys).toEqual([])
    expect(
      summarizeTokenPlanQuota('openai', [key], fetchState, now + 1).short,
    ).toBeNull()
    key.quota_windows = [{ ...row, availability: 'unknown' }]
    expect(
      summarizeTokenPlanQuota('openai', [key], fetchState, now).short,
    ).toBeNull()
    key.quota_windows = []
    expect(
      summarizeTokenPlanQuota('openai', [key], fetchState, now).short,
    ).toBeNull()
    key.quota_windows = [row]
  }
})

test('an unknown OpenAI subscription status still shows the reported windows', async () => {
  const usage: TokenPlanUsageResponse = {
    provider: 'openai',
    provider_region: null,
    keys: [
      openAiSubscriptionKey('subscription', [
        {
          source_window: 'primary',
          window_seconds: 18_000,
          remaining_percent: 25,
        },
        {
          source_window: 'secondary',
          window_seconds: 604_800,
          remaining_percent: 80,
        },
      ]),
    ],
  }

  const badges = await badgesFor('ep-openai-unknown', usage)
  // The coarse status stays unknown and the key stays unusable: the numbers
  // describe the account, they never claim routable capacity.
  expect(badges.quotaState).toBe('unknown')
  expect(badges.short).toBe(25)
  expect(badges.long).toBe(80)
  expect(labels(badges)).toEqual([
    'tokenPlanShortBadge 25%',
    'tokenPlanLongBadge 80%',
  ])
  // The generic placeholder never reaches the surface.
  expect(labels(badges)).not.toContain('tokenPlanQuotaUnknown')
})

test('an unknown subscription without any known window keeps the empty display', async () => {
  const usage: TokenPlanUsageResponse = {
    provider: 'openai',
    provider_region: null,
    keys: [
      openAiSubscriptionKey('subscription', [
        {
          source_window: 'primary',
          window_seconds: 18_000,
          remaining_percent: null,
        },
        {
          source_window: 'secondary',
          window_seconds: 604_800,
          remaining_percent: null,
        },
      ]),
    ],
  }

  const badges = await badgesFor('ep-openai-no-window', usage)
  expect(badges.quotaState).toBe('unknown')
  expect(badges.short).toBeNull()
  expect(badges.long).toBeNull()
  // Nothing is known, so nothing is claimed: no percentage and no pill.
  expect(labels(badges)).toEqual([])
})

test('an unknown subscription keeps the band whose window is actually known', async () => {
  const usage: TokenPlanUsageResponse = {
    provider: 'openai',
    provider_region: null,
    keys: [
      openAiSubscriptionKey('subscription', [
        {
          source_window: 'primary',
          window_seconds: 18_000,
          remaining_percent: null,
        },
        {
          source_window: 'secondary',
          window_seconds: 604_800,
          remaining_percent: 80,
        },
      ]),
    ],
  }

  const badges = await badgesFor('ep-openai-partial-window', usage)
  expect(badges.short).toBeNull()
  expect(badges.long).toBe(80)
  expect(labels(badges)).toEqual(['tokenPlanLongBadge 80%'])
})

test('a reported zero stays a real zero in an unknown status', async () => {
  const usage: TokenPlanUsageResponse = {
    provider: 'openai',
    provider_region: null,
    keys: [
      openAiSubscriptionKey('subscription', [
        {
          source_window: 'primary',
          window_seconds: 18_000,
          remaining_percent: 0,
        },
      ]),
    ],
  }

  const badges = await badgesFor('ep-openai-zero', usage)
  expect(badges.short).toBe(0)
  expect(badges.long).toBeNull()
  expect(labels(badges)).toEqual(['tokenPlanShortBadge 0%'])
})

test('a non-finite reported window contributes nothing', async () => {
  const key = openAiSubscriptionKey('subscription', [
    { source_window: 'primary', window_seconds: 18_000, remaining_percent: 25 },
  ])
  const windows = (
    key.model_remains[0] as unknown as {
      windows: { remaining_percent: number | null }[]
    }
  ).windows
  windows[0]!.remaining_percent = Number.NaN

  const badges = await badgesFor('ep-openai-non-finite', {
    provider: 'openai',
    provider_region: null,
    keys: [key],
  })
  expect(badges.short).toBeNull()
  expect(badges.long).toBeNull()
  expect(labels(badges)).toEqual([])
})

test('an unknown OpenCode Go status shows the reported rolling window', async () => {
  const usage: TokenPlanUsageResponse = {
    provider: 'opencode_go',
    provider_region: null,
    keys: [
      openCodeGoKey('go', {
        rolling: { percent: 30 },
        weekly: { percent: null },
        monthly: null,
      }),
    ],
  }

  const badges = await badgesFor('ep-go-unknown', usage)
  expect(badges.quotaState).toBe('unknown')
  // 100 - used, exactly like the available status reports it.
  expect(badges.short).toBe(70)
  expect(badges.long).toBeNull()
  expect(labels(badges)).toEqual(['tokenPlanShortBadge 70%'])
})

test('a mixed pool averages the known windows of the unknown key too', async () => {
  const keys = [
    openAiSubscriptionKey(
      'available',
      [
        {
          source_window: 'primary',
          window_seconds: 18_000,
          remaining_percent: 40,
        },
        {
          source_window: 'secondary',
          window_seconds: 604_800,
          remaining_percent: 60,
        },
      ],
      'available',
    ),
    openAiSubscriptionKey('unknown', [
      {
        source_window: 'primary',
        window_seconds: 18_000,
        remaining_percent: 20,
      },
      {
        source_window: 'secondary',
        window_seconds: 604_800,
        remaining_percent: 40,
      },
    ]),
  ]

  const summary = summarizeTokenPlanQuota('openai', keys, 'ready')
  expect(summary.state).toBe('partial')
  // Only the actually reported windows join the average.
  expect([summary.short, summary.long]).toEqual([30, 50])
  expect(summary.usableKeys.map((key) => key.key_id)).toEqual(['available'])
})

test('a stale unknown observation keeps the numbers visible but never usable', async () => {
  const usage: TokenPlanUsageResponse = {
    provider: 'openai',
    provider_region: null,
    keys: [
      openAiSubscriptionKey('subscription', [
        {
          source_window: 'primary',
          window_seconds: 18_000,
          remaining_percent: 25,
        },
        {
          source_window: 'secondary',
          window_seconds: 604_800,
          remaining_percent: 80,
        },
      ]),
    ],
  }

  const stale = summarizeTokenPlanQuota(
    'openai',
    usage.keys,
    'stale',
    Date.now(),
  )
  expect(stale.state).toBe('unknown')
  expect(stale.usableKeys).toEqual([])
  // The last-known numbers stay visible as historical evidence.
  expect([stale.short, stale.long]).toEqual([25, 80])

  const pills = tokenPlanBadgePills(
    {
      mode: 'window',
      status: 'stale',
      quotaState: stale.state,
      quotaPartial: stale.partial,
      quotaMetadataPresent: true,
      short: stale.short,
      long: stale.long,
      openrouterBalance: null,
      openrouterRemaining: null,
      openrouterDailySpend: null,
      deepseekBalances: [],
      deepseekAvailable: null,
      deepseekState: 'unknown',
      localTodayTokens: null,
      commandCodeExhausted: false,
      cooldownUntil: stale.cooldownUntil,
    },
    t,
  )
  expect(pills.map((pill) => pill.label)).toEqual([
    'tokenPlanShortBadge 25%',
    'tokenPlanLongBadge 80%',
  ])
  // The retained numbers are not certified current: the title keeps the
  // unknown marker instead of a visible placeholder pill.
  expect(pills.every((pill) => pill.title === 'tokenPlanQuotaUnknown')).toBe(
    true,
  )
})

test('a demoted feedback deadline keeps its key out of the displayed numbers', async () => {
  const key = openAiSubscriptionKey('feedback', [
    { source_window: 'primary', window_seconds: 18_000, remaining_percent: 25 },
  ])
  key.quota_status = {
    state: 'rate_limited',
    scope: 'text_inference',
    cooldown_until: new Date(Date.now() - 60_000).toISOString(),
  }

  const summary = summarizeTokenPlanQuota('openai', [key], 'ready')
  // An elapsed deadline is demoted to unknown and stays without capacity
  // until a refresh proves otherwise.
  expect(summary.state).toBe('unknown')
  expect([summary.short, summary.long]).toEqual([null, null])
})

test('the shared table and card surfaces show the same concrete quotas', async () => {
  const usage: TokenPlanUsageResponse = {
    provider: 'openai',
    provider_region: null,
    keys: [
      openAiSubscriptionKey('subscription', [
        {
          source_window: 'primary',
          window_seconds: 18_000,
          remaining_percent: 25,
        },
        {
          source_window: 'secondary',
          window_seconds: 604_800,
          remaining_percent: 80,
        },
      ]),
    ],
  }
  const badges = await badgesFor('ep-surface', usage)

  for (const density of ['table', 'card'] as const) {
    const html = await renderToString(
      createSSRApp({
        render: () => h(renderTokenPlanBadges(badges, t, density)),
      }),
    )
    expect(html).toContain('tokenPlanShortBadge 25%')
    expect(html).toContain('tokenPlanLongBadge 80%')
    // No visible placeholder pill; the unknown marker stays in the tooltip.
    expect(html).not.toContain('>tokenPlanQuotaUnknown<')
    expect(html).toContain('title="tokenPlanQuotaUnknown"')
  }
})

test('an OpenRouter balance never hides behind the generic quota placeholder', async () => {
  const badges = await badgesFor('ep-openrouter', {
    provider: 'openrouter',
    provider_region: null,
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [],
        quota_status: { state: 'available', scope: 'text_inference' },
        openrouter_balance: {
          limit: 100,
          limit_remaining: 42.5,
          limit_reset: null,
          is_free_tier: false,
          total_credits: null,
          total_usage: null,
        },
      },
    ],
  })

  expect(labels(badges)).toEqual(['tokenPlanOpenRouterRemaining $42.50'])
  expect(labels(badges)).not.toContain('tokenPlanQuotaUnknown')
})

test('an OpenRouter account without a credit signal names the balance', async () => {
  const badges = await badgesFor('ep-openrouter-empty', {
    provider: 'openrouter',
    provider_region: null,
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [],
        quota_status: { state: 'unknown', scope: 'text_inference' },
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

  expect(labels(badges)).toEqual(['tokenPlanBalanceUnknown'])
  expect(labels(badges)).not.toContain('tokenPlanQuotaUnknown')
})

test('a DeepSeek account without a balance names the balance, not the quota', async () => {
  const badges = await badgesFor('ep-deepseek', {
    provider: 'deepseek',
    provider_region: null,
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [],
        quota_status: { state: 'unknown', scope: 'text_inference' },
      },
    ],
  })

  expect(labels(badges)).toEqual(['tokenPlanBalanceUnknown'])
  expect(labels(badges)).not.toContain('tokenPlanQuotaUnknown')
})
