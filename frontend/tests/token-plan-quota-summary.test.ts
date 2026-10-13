import { expect, test } from 'bun:test'
import { summarizeTokenPlanQuota } from '../src/composables/tokenPlanQuotaSummary'
import type { TokenPlanKeyUsage } from '../src/generated/admin-api'
import {
  minimaxQuotaKey,
  quotaStatus,
  subscriptionWindow,
} from './helpers/token-plan-quota-fixtures'

test('MiniMax averages only usable default-text keys and keeps other models raw', () => {
  const keys = [
    minimaxQuotaKey('available', 'available', 1, 35, [
      { model_name: 'video', interval: { remaining_percent: 100 } },
    ]),
    minimaxQuotaKey('short-zero', 'exhausted', 0, 11),
    minimaxQuotaKey('weekly-zero', 'exhausted', 100, 0, [
      { model_name: 'video', interval: { remaining_percent: 100 } },
    ]),
  ]
  const summary = summarizeTokenPlanQuota('minimax', keys, 'ready')

  expect([summary.short, summary.long]).toEqual([1, 35])
  expect(summary.state).toBe('partial')
  expect(summary.usableKeys.map((key) => key.key_id)).toEqual(['available'])
  expect(keys[1]?.model_remains[0]?.interval?.remaining_percent).toBe(0)
  expect(keys[2]?.model_remains[1]?.interval?.remaining_percent).toBe(100)
})

test('all confirmed exhausted keys report zero without rewriting raw windows', () => {
  const keys = [
    minimaxQuotaKey('a', 'exhausted', 0, 40),
    minimaxQuotaKey('b', 'exhausted', 60, 0),
  ]
  const summary = summarizeTokenPlanQuota('minimax', keys, 'ready')
  expect([summary.state, summary.short, summary.long]).toEqual([
    'exhausted',
    0,
    0,
  ])
  expect(
    keys.map((key) => key.model_remains[0]?.interval?.remaining_percent),
  ).toEqual([0, 60])
})

test('fresh relative-reset zero metadata keeps the effective summary at zero', () => {
  const key = minimaxQuotaKey('relative-zero', 'exhausted', 0, 40)
  key.model_remains[0]!.interval = {
    remaining_percent: 0,
    remains_time_ms: 0,
  }

  const summary = summarizeTokenPlanQuota('minimax', [key], 'ready')
  expect([summary.state, summary.short, summary.long]).toEqual([
    'exhausted',
    0,
    0,
  ])
})

test('stale observations stay unknown without usable capacity but retain numbers', () => {
  const stale = minimaxQuotaKey('stale', 'available', 90, 95)
  const unknown = minimaxQuotaKey('unknown', 'unknown', 80, 80)
  const summary = summarizeTokenPlanQuota('minimax', [stale, unknown], 'stale')
  expect(summary.state).toBe('unknown')
  // Historical numeric evidence stays visible; the unknown key contributes
  // nothing, and neither observation counts as usable/current capacity.
  expect(summary.short).toBe(90)
  expect(summary.long).toBe(95)
  expect(summary.usableKeys).toEqual([])

  const partial = summarizeTokenPlanQuota(
    'minimax',
    [minimaxQuotaKey('fresh', 'available', 40, 60), unknown],
    'ready',
  )
  expect(partial.state).toBe('partial')
  expect(partial.short).toBe(40)
  expect(partial.long).toBe(60)
})

test('legacy responses keep numeric detail but do not claim availability', () => {
  const legacy = minimaxQuotaKey('legacy', null, 25, 75)
  const summary = summarizeTokenPlanQuota('minimax', [legacy], 'ready')
  expect(summary.state).toBe('unknown')
  expect([summary.short, summary.long]).toEqual([25, 75])
})

test('ChatGPT uses reported short and long durations and preserves unknown windows', () => {
  const key = {
    key_id: 'subscription',
    key_label: 'ChatGPT Plus',
    ok: true,
    quota_status: quotaStatus('available'),
    model_remains: [
      {
        model_name: 'plus',
        windows: [
          subscriptionWindow(18_000, 3),
          subscriptionWindow(604_800, 35),
        ],
      },
    ],
  } as TokenPlanKeyUsage
  const summary = summarizeTokenPlanQuota('openai', [key], 'ready')
  expect([summary.short, summary.long]).toEqual([3, 35])

  key.quota_status = quotaStatus('unknown')
  key.model_remains[0]!.windows = [
    subscriptionWindow(18_000, null),
    subscriptionWindow(604_800, 35),
  ]
  const unknown = summarizeTokenPlanQuota('openai', [key], 'ready')
  expect(unknown.state).toBe('unknown')
  expect([unknown.short, unknown.long]).toEqual([null, null])
})

test('a transient block keeps the provider percentages and names its cause', () => {
  for (const state of ['rate_limited', 'upstream_unavailable'] as const) {
    const key = minimaxQuotaKey(state, state, 70, 55)
    const summary = summarizeTokenPlanQuota('minimax', [key], 'ready')
    expect(summary.state).toBe(state)
    // The account's quota is untouched, so its windows stay visible.
    expect([summary.short, summary.long]).toEqual([70, 55])
    expect(summary.usableKeys).toEqual([])
  }
  // A confirmed exhaustion keeps the cooling presentation and no percentages.
  const exhausted = summarizeTokenPlanQuota(
    'minimax',
    [minimaxQuotaKey('exhausted', 'cooling', 70, 55)],
    'ready',
  )
  expect(exhausted.state).toBe('cooling')
  expect([exhausted.short, exhausted.long]).toEqual([null, null])
})

test('an entirely blocked pool rolls up by cause priority, never as exhausted', () => {
  const cases = [
    [['cooling', 'rate_limited'], 'cooling'],
    [['upstream_unavailable', 'rate_limited'], 'upstream_unavailable'],
    [['rate_limited'], 'rate_limited'],
    [['rate_limited', 'exhausted'], 'rate_limited'],
  ] as const
  for (const [states, expected] of cases) {
    const keys = states.map((state) =>
      minimaxQuotaKey(`${state}-key`, state, 40, 40),
    )
    const summary = summarizeTokenPlanQuota('minimax', keys, 'ready')
    expect(summary.state).toBe(expected)
    expect(summary.allExhausted).toBe(false)
  }
  // Every key genuinely spent is still the exhausted summary.
  const spent = summarizeTokenPlanQuota(
    'minimax',
    ['a', 'b'].map((id) => minimaxQuotaKey(id, 'exhausted', 30, 30)),
    'ready',
  )
  expect([spent.state, spent.allExhausted]).toEqual(['exhausted', true])
})

test('one usable key keeps a mixed pool out of the blocked rollup', () => {
  const summary = summarizeTokenPlanQuota(
    'minimax',
    [
      minimaxQuotaKey('available', 'available', 40, 60),
      minimaxQuotaKey('throttled', 'rate_limited', 10, 10),
      minimaxQuotaKey('unknown', 'unknown', 10, 10),
    ],
    'ready',
  )
  expect(summary.state).toBe('partial')
  expect(summary.allExhausted).toBe(false)
  expect(summary.usableKeys.map((key) => key.key_id)).toEqual(['available'])
  // The throttled key still has capacity, so its windows join the average; the
  // unknown one contributes nothing.
  expect([summary.short, summary.long]).toEqual([25, 35])
})

test('an expired retry deadline is demoted to unknown in both surfaces', () => {
  const expired = new Date(Date.now() - 60_000).toISOString()
  const active = new Date(Date.now() + 60_000).toISOString()
  const key = (state: 'cooling' | 'rate_limited', until: string) => {
    const built = minimaxQuotaKey(`${state}-${until}`, state, 60, 60)
    built.quota_status = {
      state,
      scope: 'text_inference',
      cooldown_until: until,
    }
    return built
  }

  const live = summarizeTokenPlanQuota(
    'minimax',
    [key('rate_limited', active)],
    'ready',
  )
  expect(live.state).toBe('rate_limited')
  expect(live.cooldownUntil).toBe(active)

  const elapsed = summarizeTokenPlanQuota(
    'minimax',
    [key('rate_limited', expired)],
    'ready',
  )
  expect(elapsed.state).toBe('unknown')
  expect([elapsed.short, elapsed.long]).toEqual([null, null])
  expect(elapsed.cooldownUntil).toBeNull()

  // One demoted key leaves the pool unknown rather than blocked, and the
  // still-running deadline of the other key is the one the badge reports.
  const mixed = summarizeTokenPlanQuota(
    'minimax',
    [key('rate_limited', active), key('cooling', expired)],
    'ready',
  )
  expect(mixed.state).toBe('unknown')
  expect(mixed.cooldownUntil).toBe(active)
})

test('stale exhausted observations stay unknown and retain actual windows', () => {
  const cases = [
    { id: 'stale-0-40', interval: 0, weekly: 40 },
    { id: 'stale-40-0', interval: 40, weekly: 0 },
    { id: 'stale-0-0', interval: 0, weekly: 0 },
  ] as const
  for (const { id, interval, weekly } of cases) {
    const summary = summarizeTokenPlanQuota(
      'minimax',
      [minimaxQuotaKey(id, 'exhausted', interval, weekly)],
      'stale',
    )
    expect(summary.state).toBe('unknown')
    expect(summary.usableKeys).toEqual([])
    expect(summary.allExhausted).toBe(false)
    // Historical evidence stays visible; a 0/40 window is never flattened to
    // a current 0/0 exhaustion claim.
    expect([summary.short, summary.long]).toEqual([interval, weekly])
  }
  // An absent window stays null, never a fabricated zero.
  const absent = summarizeTokenPlanQuota(
    'minimax',
    [minimaxQuotaKey('stale-absent', 'exhausted', null, null)],
    'stale',
  )
  expect(absent.state).toBe('unknown')
  expect([absent.short, absent.long]).toEqual([null, null])
  expect(absent.allExhausted).toBe(false)
})

test('a stale window with expired feedback stays unknown but retains numbers', () => {
  const expired = new Date(Date.now() - 60_000).toISOString()
  for (const state of [
    'cooling',
    'rate_limited',
    'upstream_unavailable',
  ] as const) {
    const built = minimaxQuotaKey(`stale-expired-${state}`, state, 60, 40)
    built.quota_status = {
      state,
      scope: 'text_inference',
      cooldown_until: expired,
    }
    const summary = summarizeTokenPlanQuota('minimax', [built], 'stale')
    // The label demotes without deleting successful historical windows, and
    // the elapsed deadline is never reported as a live retry.
    expect(summary.state).toBe('unknown')
    expect(summary.usableKeys).toEqual([])
    expect([summary.short, summary.long]).toEqual([60, 40])
    expect(summary.cooldownUntil).toBeNull()
    expect(summary.allExhausted).toBe(false)
  }
  // An absent default-text window stays null, never a fabricated zero.
  const missing = minimaxQuotaKey(
    'stale-expired-missing',
    'rate_limited',
    null,
    null,
  )
  missing.quota_status = {
    state: 'rate_limited',
    scope: 'text_inference',
    cooldown_until: expired,
  }
  const absent = summarizeTokenPlanQuota('minimax', [missing], 'stale')
  expect(absent.state).toBe('unknown')
  expect([absent.short, absent.long]).toEqual([null, null])
  expect(absent.cooldownUntil).toBeNull()
})

test('a stale window with active feedback keeps its live label', () => {
  const active = new Date(Date.now() + 60_000).toISOString()
  const built = minimaxQuotaKey('stale-active', 'rate_limited', 70, 55)
  built.quota_status = {
    state: 'rate_limited',
    scope: 'text_inference',
    cooldown_until: active,
  }
  const summary = summarizeTokenPlanQuota('minimax', [built], 'stale')
  expect(summary.state).toBe('rate_limited')
  expect(summary.usableKeys).toEqual([])
  expect([summary.short, summary.long]).toEqual([70, 55])
  expect(summary.cooldownUntil).toBe(active)
})

test('a ready exhausted observation keeps the existing zero summary', () => {
  const summary = summarizeTokenPlanQuota(
    'minimax',
    [minimaxQuotaKey('ready-exhausted', 'exhausted', 0, 40)],
    'ready',
  )
  expect([summary.state, summary.short, summary.long]).toEqual([
    'exhausted',
    0,
    0,
  ])
  expect(summary.allExhausted).toBe(true)
})

function glmKey(
  id: string,
  fiveHour: {
    current_value: number
    limit: number
    remaining: number
    percentage?: number | null
  } | null,
  weekly: {
    current_value: number
    limit: number
    remaining: number
    percentage?: number | null
  } | null,
): TokenPlanKeyUsage {
  return {
    key_id: id,
    key_label: id,
    ok: true,
    model_remains: [],
    glm_five_hour: fiveHour,
    glm_weekly: weekly,
  } as TokenPlanKeyUsage
}

test('GLM weekly exhaustion and fresh 5h window keep honest percentages', () => {
  const summary = summarizeTokenPlanQuota(
    'glm',
    [
      glmKey(
        'glm-exhausted-weekly',
        { current_value: 0, limit: 10000, remaining: 10000, percentage: 0 },
        { current_value: 10000, limit: 10000, remaining: 0, percentage: 100 },
      ),
    ],
    'ready',
  )
  // Legacy key without quota_status: numeric evidence stays visible without
  // claiming availability; no weekly exhaustion is invented or hidden.
  expect(summary.state).toBe('unknown')
  expect([summary.short, summary.long]).toEqual([100, 0])
})

test('GLM missing weekly window leaves long null', () => {
  const summary = summarizeTokenPlanQuota(
    'glm',
    [
      glmKey(
        'glm-no-weekly',
        {
          current_value: 500,
          limit: 2000,
          remaining: 1500,
          percentage: 25,
        },
        null,
      ),
    ],
    'ready',
  )
  expect(summary.state).toBe('unknown')
  expect([summary.short, summary.long]).toEqual([75, null])
})
