import { expect, test } from 'bun:test'
import { createSSRApp, h } from 'vue'
import { renderToString } from 'vue/server-renderer'
import {
  TOKEN_PLAN_BADGE_LAYOUT,
  deepseekBalanceRows,
  renderTokenPlanBadges,
  type TokenPlanBadgeDensity,
} from '../src/components/endpoints/tokenPlanBadgeRender'
import type { DeepSeekBalance } from '../src/generated/admin-api'
import type { TokenPlanPillSource } from '../src/composables/useTokenPlanBadges'

// P3: the desktop table and the mobile card must render one shared
// density-aware surface. These cases pin the shared behavior (same pills, same
// order, per-density layout) rather than either view's markup.

const DENSITIES: TokenPlanBadgeDensity[] = ['table', 'card']

function render(source: TokenPlanPillSource, density: TokenPlanBadgeDensity) {
  return renderToString(
    createSSRApp({
      render: () => h(renderTokenPlanBadges(source, t, density)),
    }),
  )
}

const t = ((key: string, params?: Record<string, unknown>) =>
  params ? `${key}(${JSON.stringify(params)})` : key) as unknown as TranslateFn

const windowSource = (
  short: number | null,
  long: number | null,
): TokenPlanPillSource => ({
  mode: 'window',
  status: 'ready',
  short,
  long,
  openrouterBalance: null,
  openrouterRemaining: null,
  openrouterDailySpend: null,
  deepseekBalances: [],
  deepseekAvailable: null,
  deepseekState: 'ready',
  localTodayTokens: null,
  commandCodeExhausted: false,
})

test('both densities render the same pills in the same order', async () => {
  const source = windowSource(42, 73)
  const table = await render(source, 'table')
  const card = await render(source, 'card')

  for (const html of [table, card]) {
    expect(html).toContain('tokenPlanShortBadge 42%')
    expect(html).toContain('tokenPlanLongBadge 73%')
  }
  // Same set, same order: the labels are not density-dependent.
  const labels = (html: string) =>
    [...html.matchAll(/>(tokenPlan(?:Short|Long)Badge[^<]*)</g)].map(
      (m) => m[1],
    )
  expect(labels(table)).toEqual(labels(card))
  expect(labels(table)).toEqual([
    'tokenPlanShortBadge 42%',
    'tokenPlanLongBadge 73%',
  ])
})

test('each density keeps its own responsive layout classes', async () => {
  const source = windowSource(42, 73)
  const table = await render(source, 'table')
  const card = await render(source, 'card')

  expect(table).toContain(TOKEN_PLAN_BADGE_LAYOUT.table.wrapper)
  expect(table).toContain(TOKEN_PLAN_BADGE_LAYOUT.table.pill)
  expect(table).not.toContain(TOKEN_PLAN_BADGE_LAYOUT.card.wrapper)
  expect(table).not.toContain(TOKEN_PLAN_BADGE_LAYOUT.card.pill)

  expect(card).toContain(TOKEN_PLAN_BADGE_LAYOUT.card.wrapper)
  expect(card).toContain(TOKEN_PLAN_BADGE_LAYOUT.card.pill)
  expect(card).not.toContain(TOKEN_PLAN_BADGE_LAYOUT.table.wrapper)
  expect(card).not.toContain(TOKEN_PLAN_BADGE_LAYOUT.table.pill)

  // The responsive difference that motivated the density input: the card row
  // wraps, the table row does not.
  expect(TOKEN_PLAN_BADGE_LAYOUT.card.wrapper).toContain('flex-wrap')
  expect(TOKEN_PLAN_BADGE_LAYOUT.table.wrapper).not.toContain('flex-wrap')
})

test('an endpoint with nothing to report renders the density placeholder at both densities', async () => {
  const source = windowSource(null, null)
  for (const density of DENSITIES) {
    const html = await render(source, density)
    expect(html).toContain(TOKEN_PLAN_BADGE_LAYOUT[density].empty)
    expect(html).toContain('—')
  }
})

test('the DeepSeek state pills survive the shared surface without a fabricated amount', async () => {
  const allUnknown: TokenPlanPillSource = {
    ...windowSource(null, null),
    mode: 'deepseek',
    deepseekState: 'unknown',
  }
  for (const density of DENSITIES) {
    const html = await render(allUnknown, density)
    expect(html).toContain('tokenPlanBalanceUnknown')
    // No currency amount is invented for an account that reports none.
    expect(html).not.toContain('$0.00')
  }

  const unavailable: TokenPlanPillSource = {
    ...allUnknown,
    deepseekState: 'unavailable',
  }
  const html = await render(unavailable, 'table')
  expect(html).toContain('tokenPlanUnavailable')
  expect(html).not.toContain('$0.00')
})

test('deepseekBalanceRows lists every currency in payload order, never summed', () => {
  const balance: DeepSeekBalance = {
    balances: [
      {
        currency: 'CNY',
        total_balance: 12.5,
        granted_balance: 2,
        topped_up_balance: 10.5,
      },
      {
        currency: 'USD',
        total_balance: 3,
        granted_balance: null,
        topped_up_balance: 3,
      },
    ],
    is_available: true,
  }
  const rows = deepseekBalanceRows(balance, t)
  expect(rows).toEqual([
    { currency: 'CNY', total: '¥12.50', granted: '¥2.00', toppedUp: '¥10.50' },
    {
      currency: 'USD',
      total: '$3.00',
      granted: 'tokenPlanBalanceUnknown',
      toppedUp: '$3.00',
    },
  ])
})

test('deepseekBalanceRows keeps a real zero and names an unknown amount', () => {
  const rows = deepseekBalanceRows(
    {
      balances: [
        {
          currency: 'CNY',
          total_balance: 0,
          granted_balance: 0,
          topped_up_balance: 0,
        },
        {
          currency: 'USD',
          total_balance: null,
          granted_balance: null,
          topped_up_balance: null,
        },
      ],
      is_available: true,
    },
    t,
  )
  // A reported zero is a real amount, an omitted one is unknown — the two must
  // never collapse into the same rendering.
  expect(rows[0]).toEqual({
    currency: 'CNY',
    total: '¥0.00',
    granted: '¥0.00',
    toppedUp: '¥0.00',
  })
  expect(rows[1]).toEqual({
    currency: 'USD',
    total: 'tokenPlanBalanceUnknown',
    granted: 'tokenPlanBalanceUnknown',
    toppedUp: 'tokenPlanBalanceUnknown',
  })
})

test('deepseekBalanceRows reports an account with no parseable entry as no rows', () => {
  expect(deepseekBalanceRows({ balances: [], is_available: false }, t)).toEqual(
    [],
  )
  expect(deepseekBalanceRows(null, t)).toEqual([])
})
