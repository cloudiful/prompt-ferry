import { h, type VNode } from 'vue'
import type { DeepSeekBalance } from '@/generated/admin-api'
import {
  tokenPlanBadgePills,
  type TokenPlanPillSource,
} from '@/composables/useTokenPlanBadges'
import { formatMoney } from '@/composables/useTokenPlanWindowEntries'

// Issue #715 P3: the shared render helpers for the token-plan surfaces. The
// desktop table and the mobile card no longer carry their own copy of the pill
// markup — they pass a density and get the row back. Only the layout differs
// between the two (tight single-line column vs. wrapping card surface); which
// pills an endpoint renders is decided once, by `tokenPlanBadgePills`. The
// multi-currency DeepSeek rows below serve the same contract for the usage
// dialog.
export type TokenPlanBadgeDensity = 'table' | 'card'

export type TokenPlanBadgeLayout = {
  // Wrapper around the pill set. The card wraps across its wider surface, the
  // table keeps the pills on one line so the row height stays stable.
  wrapper: string
  pill: string
  // Shown while the shared cache has no snapshot yet, so neither view blinks
  // a fabricated zero before the lazy prefetch lands.
  empty: string
}

export const TOKEN_PLAN_BADGE_LAYOUT: Record<
  TokenPlanBadgeDensity,
  TokenPlanBadgeLayout
> = {
  table: {
    wrapper: 'inline-flex items-center gap-1',
    pill: 'inline-flex items-center rounded-full border border-default bg-elevated px-1.5 py-px text-[0.7rem] font-semibold whitespace-nowrap',
    empty: 'text-xs text-muted',
  },
  card: {
    wrapper: 'flex flex-wrap items-center gap-1',
    pill: 'inline-flex items-center rounded-full border border-default bg-elevated px-2 py-px text-[0.74rem] font-semibold whitespace-nowrap',
    empty: 'text-[0.74rem] text-muted',
  },
}

// One DeepSeek currency entry as a display surface lists it. Every amount is
// pre-formatted so a template never has to decide what an absent amount means:
// `null` is unknown and says so, a reported `0` stays a real `$0.00`, and
// entries are never summed across currencies.
export type DeepSeekBalanceRow = {
  currency: string
  total: string
  granted: string
  toppedUp: string
}

function amountOrUnknown(
  t: TranslateFn,
  currency: string,
  value: number | null | undefined,
): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return t('tokenPlanBalanceUnknown')
  }
  return formatMoney(currency, value)
}

// `balances[]` arrives in the contract's deterministic order (by currency,
// then amount) and is carried through as reported: no FX conversion, no
// cross-currency total, no dropped currency. The dialog uses this so the
// multi-currency shape and its unknown/real-zero rule are stated once,
// alongside the badge pills that render the same entries.
export function deepseekBalanceRows(
  balance: DeepSeekBalance | null | undefined,
  t: TranslateFn,
): DeepSeekBalanceRow[] {
  if (!balance) return []
  return balance.balances.map((entry) => ({
    currency: entry.currency,
    total: amountOrUnknown(t, entry.currency, entry.total_balance),
    granted: amountOrUnknown(t, entry.currency, entry.granted_balance),
    toppedUp: amountOrUnknown(t, entry.currency, entry.topped_up_balance),
  }))
}

// Render one endpoint's badges at the requested density. An empty pill set
// (cold cache, or a provider with nothing to report) falls back to the dash
// placeholder, which is the pre-P3 behavior of both views.
export function renderTokenPlanBadges(
  source: TokenPlanPillSource,
  t: TranslateFn,
  density: TokenPlanBadgeDensity,
): VNode {
  const layout = TOKEN_PLAN_BADGE_LAYOUT[density]
  const pills = tokenPlanBadgePills(source, t)
  if (pills.length === 0) {
    return h('span', { class: layout.empty }, '—')
  }
  return h(
    'span',
    { class: layout.wrapper },
    pills.map((pill) =>
      h(
        'span',
        { class: layout.pill, style: { color: pill.color }, title: pill.title },
        pill.label,
      ),
    ),
  )
}
