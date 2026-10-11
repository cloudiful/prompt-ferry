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
  expect(keys.map((key) => key.model_remains[0]?.interval?.remaining_percent)).toEqual([0, 60])
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

test('stale and unknown observations never become confirmed capacity', () => {
  const stale = minimaxQuotaKey('stale', 'available', 90, 95)
  const unknown = minimaxQuotaKey('unknown', 'unknown', 80, 80)
  const summary = summarizeTokenPlanQuota('minimax', [stale, unknown], 'stale')
  expect(summary.state).toBe('unknown')
  expect(summary.short).toBeNull()
  expect(summary.long).toBeNull()

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
        windows: [subscriptionWindow(18_000, 3), subscriptionWindow(604_800, 35)],
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
