import { expect, test } from 'bun:test'
import type {
  CanonicalQuotaWindow,
  TokenPlanKeyUsage,
} from '../src/generated/admin-api'
import type { CanonicalQuotaRow } from '../src/composables/useCanonicalQuotaWindows'
import {
  canonicalBandPercent,
  canonicalQuantityCaption,
  keyBandPercent,
  toCanonicalQuotaRows,
} from '../src/composables/useCanonicalQuotaWindows'

const NOW = Date.parse('2026-10-11T00:00:00.000Z')
const FUTURE = new Date(NOW + 3_600_000).toISOString()
const PAST = new Date(NOW - 3_600_000).toISOString()

function key(overrides: Partial<TokenPlanKeyUsage> = {}): TokenPlanKeyUsage {
  return {
    key_id: 'k',
    key_label: 'k',
    ok: true,
    model_remains: [],
    ...overrides,
  } as TokenPlanKeyUsage
}

function observation(
  overrides: Partial<CanonicalQuotaWindow> & {
    source_id: string
    kind: CanonicalQuotaWindow['kind']
  },
): CanonicalQuotaWindow {
  return {
    source_model: null,
    unit: null,
    duration_seconds: null,
    is_default_scope: true,
    availability: 'known',
    remaining_percent: 80,
    current: null,
    limit: null,
    remaining: null,
    reset_at: null,
    ...overrides,
  }
}

test('canonical observations win over legacy fields', () => {
  const rows = toCanonicalQuotaRows(
    key({
      glm_five_hour: {
        current_value: 0,
        limit: 2000,
        remaining: 2000,
        percentage: 0,
      },
      quota_windows: [
        observation({
          source_id: 'glm:five_hour',
          kind: 'five_hour',
          unit: 'tokens',
          remaining_percent: 25,
          current: 75,
          limit: 100,
          remaining: 25,
          reset_at: FUTURE,
        }),
      ],
    }),
    NOW,
  )
  expect(rows).toHaveLength(1)
  expect(rows[0]?.remainingPercent).toBe(25)
  expect(rows[0]?.current).toBe(75)
  expect(canonicalBandPercent(rows, 'short')).toBe(25)
})

test('an explicit empty vector is authoritative and never falls back', () => {
  const rows = toCanonicalQuotaRows(
    key({
      glm_five_hour: {
        current_value: 0,
        limit: 2000,
        remaining: 2000,
        percentage: 0,
      },
      quota_windows: [],
    }),
    NOW,
  )
  expect(rows).toEqual([])
  // A missing field falls back; an explicit empty does not.
  const fallback = toCanonicalQuotaRows(
    key({
      glm_five_hour: {
        current_value: 0,
        limit: 2000,
        remaining: 2000,
        percentage: 0,
      },
    }),
    NOW,
  )
  expect(fallback).toHaveLength(1)
})

test('failed keys carry no rows', () => {
  const rows = toCanonicalQuotaRows(
    key({
      ok: false,
      glm_five_hour: {
        current_value: 0,
        limit: 2000,
        remaining: 2000,
        percentage: 0,
      },
      quota_windows: [
        observation({ source_id: 'glm:five_hour', kind: 'five_hour' }),
      ],
    }),
    NOW,
  )
  expect(rows).toEqual([])
})

test('legacy minimax counts only the default-scope model', () => {
  const rows = toCanonicalQuotaRows(
    key({
      model_remains: [
        {
          model_name: 'General',
          interval: { remaining_percent: 35 },
          weekly: { remaining_percent: 60 },
        },
        {
          model_name: 'video',
          interval: { remaining_percent: 99 },
          weekly: { remaining_percent: 99 },
        },
      ],
    }),
    NOW,
  )
  expect(canonicalBandPercent(rows, 'short')).toBe(35)
  expect(canonicalBandPercent(rows, 'long')).toBe(60)
  const general = rows.filter((row) => row.sourceModel === 'General')
  expect(general.every((row) => row.isDefaultScope)).toBe(true)
  const video = rows.filter((row) => row.sourceModel === 'video')
  expect(video.every((row) => !row.isDefaultScope)).toBe(true)
})

test('a real zero stays known and distinct from unknown', () => {
  const rows = toCanonicalQuotaRows(
    key({
      model_remains: [
        {
          model_name: 'general',
          interval: { remaining_percent: 0 },
          weekly: null,
        },
      ],
    }),
    NOW,
  )
  expect(canonicalBandPercent(rows, 'short')).toBe(0)
  expect(keyBandPercent(key(), 'short', NOW)).toBe(null)
})

test('legacy GLM keeps honest percents with unknown quantities', () => {
  const rows = toCanonicalQuotaRows(
    key({
      glm_five_hour: {
        current_value: 0,
        limit: 2000,
        remaining: 2000,
        percentage: 0,
      },
      glm_weekly: {
        current_value: 10000,
        limit: 10000,
        remaining: 0,
        percentage: 100,
      },
    }),
    NOW,
  )
  expect(canonicalBandPercent(rows, 'short')).toBe(100)
  expect(canonicalBandPercent(rows, 'long')).toBe(0)
  // Ambiguous legacy amounts never become captions.
  for (const row of rows) {
    expect(row.current).toBe(null)
    expect(canonicalQuantityCaption(row)).toBe(null)
  }
})

test('canonical GLM captions only observed positive-cap amounts', () => {
  const rows = toCanonicalQuotaRows(
    key({
      quota_windows: [
        observation({
          source_id: 'glm:five_hour',
          kind: 'five_hour',
          unit: 'tokens',
          remaining_percent: 100,
          current: 0,
          limit: 2000,
        }),
        observation({
          source_id: 'glm:weekly',
          kind: 'weekly',
          unit: 'credits',
          remaining_percent: null,
          availability: 'unknown',
          current: 0,
          limit: 0,
        }),
      ],
    }),
    NOW,
  )
  expect(canonicalQuantityCaption(rows[0]!)).toBe('0.00 / 2000.00')
  // Zero cap: known amount, but no caption may fake a total.
  expect(canonicalQuantityCaption(rows[1]!)).toBe(null)
})

function balanceRow(
  overrides: Partial<CanonicalQuotaRow> = {},
): CanonicalQuotaRow {
  return {
    sourceId: 'deepseek:CNY',
    kind: 'balance',
    sourceModel: null,
    unit: 'CNY',
    durationSeconds: null,
    isDefaultScope: true,
    availability: 'known',
    observedRemainingPercent: null,
    remainingPercent: null,
    current: null,
    limit: null,
    remaining: null,
    resetAt: null,
    expired: false,
    ...overrides,
  }
}

test('balance captions render an observed total even without a cap', () => {
  // Capless observed amount (DeepSeek CNY 9.91 / null limit): the total alone.
  expect(canonicalQuantityCaption(balanceRow({ current: 9.91 }))).toBe('9.91')
  // A real zero is a real amount, not a dash.
  expect(
    canonicalQuantityCaption(
      balanceRow({ sourceId: 'deepseek:USD', unit: 'USD', current: 0 }),
    ),
  ).toBe('0.00')
  // A negative balance is neither clamped nor hidden.
  expect(canonicalQuantityCaption(balanceRow({ current: -3.5 }))).toBe('-3.50')
  // A positive cap keeps the existing remaining / cap shape.
  expect(canonicalQuantityCaption(balanceRow({ current: 5, limit: 10 }))).toBe(
    '5.00 / 10.00',
  )
  // A nonpositive cap is capless in effect, so the amount stays visible.
  expect(canonicalQuantityCaption(balanceRow({ current: 2, limit: 0 }))).toBe(
    '2.00',
  )
  // An absent amount stays unknown; no zero is inferred.
  expect(canonicalQuantityCaption(balanceRow())).toBe(null)
  // An unlimited row never invents an amount.
  expect(
    canonicalQuantityCaption(
      balanceRow({
        sourceId: 'openrouter:unlimited',
        kind: 'unlimited',
        unit: 'credits',
      }),
    ),
  ).toBe(null)
})

test('subscription rows band by their real duration', () => {
  const rows = toCanonicalQuotaRows(
    key({
      model_remains: [
        {
          model_name: 'chatgpt',
          interval: null,
          weekly: null,
          windows: [
            {
              source_window: 'primary',
              window_seconds: 18_000,
              used_percent: 20,
              remaining_percent: 80,
              reset_at: FUTURE,
              reset_after_seconds: null,
              availability: 'known',
            },
            {
              source_window: 'secondary',
              window_seconds: 604_800,
              used_percent: 10,
              remaining_percent: 90,
              reset_at: FUTURE,
              reset_after_seconds: null,
              availability: 'known',
            },
          ],
        },
      ],
    }),
    NOW,
  )
  expect(canonicalBandPercent(rows, 'short')).toBe(80)
  expect(canonicalBandPercent(rows, 'long')).toBe(90)
  expect(rows[0]?.durationSeconds).toBe(18_000)
})

test('subscription account windows bind the default pool', () => {
  const rows = toCanonicalQuotaRows(
    key({
      model_remains: [
        {
          model_name: 'chatgpt',
          interval: null,
          weekly: null,
          windows: [
            {
              source_window: 'primary',
              window_seconds: 18_000,
              used_percent: 20,
              remaining_percent: 80,
              reset_at: FUTURE,
              reset_after_seconds: null,
              availability: 'known',
            },
          ],
        },
      ],
    }),
    NOW,
  )
  expect(rows).toHaveLength(1)
  expect(rows[0]?.isDefaultScope).toBe(true)
  expect(rows[0]?.sourceModel).toBe('chatgpt')
  expect(canonicalBandPercent(rows, 'short')).toBe(80)
})

test('detail custom rows never bind the default pool minimum', () => {
  const rows = toCanonicalQuotaRows(
    key({
      quota_windows: [
        observation({
          source_id: 'subscription:primary',
          kind: 'custom',
          source_model: 'chatgpt',
          duration_seconds: 18_000,
          is_default_scope: true,
          remaining_percent: 80,
          reset_at: FUTURE,
        }),
        observation({
          source_id: 'subscription:detail',
          kind: 'custom',
          source_model: 'detail-model',
          duration_seconds: 18_000,
          is_default_scope: false,
          remaining_percent: 0,
          reset_at: FUTURE,
        }),
      ],
    }),
    NOW,
  )
  // Detail exhaustion stays visible in its own group but the pool keeps 80.
  expect(canonicalBandPercent(rows, 'short')).toBe(80)
  const detailOnly = rows.filter((row) => !row.isDefaultScope)
  expect(detailOnly).toHaveLength(1)
  expect(detailOnly[0]?.remainingPercent).toBe(0)
})

test('non-finite percents and amounts degrade to unknown', () => {
  const rows = toCanonicalQuotaRows(
    key({
      quota_windows: [
        observation({
          source_id: 'opencode_go:rolling',
          kind: 'rolling',
          remaining_percent: Number.POSITIVE_INFINITY,
          current: Number.NaN,
          limit: Number.NEGATIVE_INFINITY,
        }),
      ],
    }),
    NOW,
  )
  expect(rows[0]?.remainingPercent).toBe(null)
  expect(rows[0]?.observedRemainingPercent).toBe(null)
  expect(rows[0]?.current).toBe(null)
  expect(canonicalBandPercent(rows, 'short')).toBe(null)
})

test('an expired reset hides the bar but keeps the amounts', () => {
  const rows = toCanonicalQuotaRows(
    key({
      quota_windows: [
        observation({
          source_id: 'glm:five_hour',
          kind: 'five_hour',
          unit: 'tokens',
          remaining_percent: 75,
          current: 25,
          limit: 100,
          reset_at: PAST,
        }),
      ],
    }),
    NOW,
  )
  expect(rows[0]?.expired).toBe(true)
  expect(rows[0]?.remainingPercent).toBe(null)
  expect(rows[0]?.observedRemainingPercent).toBe(75)
  expect(rows[0]?.current).toBe(25)
  expect(canonicalBandPercent(rows, 'short')).toBe(null)
})

test('a missing reset anchor is not expiry', () => {
  const rows = toCanonicalQuotaRows(
    key({
      quota_windows: [
        observation({
          source_id: 'glm:five_hour',
          kind: 'five_hour',
          reset_at: null,
        }),
      ],
    }),
    NOW,
  )
  expect(rows[0]?.expired).toBe(false)
  expect(rows[0]?.remainingPercent).toBe(80)
})

test('balances and unlimited rows never report a percentage', () => {
  const rows = toCanonicalQuotaRows(
    key({
      openrouter_balance: {
        limit: null,
        limit_remaining: null,
        limit_reset: null,
        is_free_tier: true,
        total_credits: null,
        total_usage: null,
      },
      openrouter_spend: { usage: 1, daily: 1, weekly: 1, monthly: 1 },
      deepseek_balance: {
        is_available: false,
        balances: [
          {
            currency: 'cny',
            total_balance: 5,
            granted_balance: null,
            topped_up_balance: null,
          },
        ],
      },
    }),
    NOW,
  )
  const unlimited = rows.find((row) => row.kind === 'unlimited')
  expect(unlimited?.availability).toBe('known')
  expect(unlimited?.remainingPercent).toBe(null)
  const cny = rows.find((row) => row.sourceId === 'deepseek:CNY')
  expect(cny?.unit).toBe('CNY')
  expect(cny?.current).toBe(5)
  expect(cny?.remainingPercent).toBe(null)
  expect(canonicalBandPercent(rows, 'short')).toBe(null)
  expect(canonicalBandPercent(rows, 'long')).toBe(null)
})

test('a generic key with no rows reports no bands', () => {
  expect(keyBandPercent(key(), 'short', NOW)).toBe(null)
  expect(keyBandPercent(key(), 'long', NOW)).toBe(null)
})
