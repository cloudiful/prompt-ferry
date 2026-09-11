import { computed, type ComputedRef, type Ref } from 'vue'
import type {
  TokenPlanKeyUsage,
  TokenPlanUsageResponse,
  TokenPlanWindowUsage,
} from '@/generated/admin-api'
import { formatTokenQuantity } from './useUsageFormatting'
import {
  ccAsWindow,
  formatMoney,
  glmAsWindow,
  opencodeGoAsWindow,
  progressColor,
  remainingPercent,
} from './useTokenPlanWindowEntries'
import {
  getCachedTokenPlanUsage,
  prefetchTokenPlanUsage,
} from './useTokenPlanUsageCache'

// Badge rendering family: window providers average their per-key remaining
// windows, balance providers pair an account balance with a today-usage
// figure. Derived from the endpoint provider so the pill builder never has
// to guess from numeric nullability.
export type TokenPlanBadgeMode = 'window' | 'openrouter' | 'deepseek'

// Fields the pill builder needs. Kept separate from `TokenPlanBadges` so the
// aggregate and the tests can construct a source without the raw payload.
export type TokenPlanPillSource = {
  mode: TokenPlanBadgeMode
  // Arithmetic mean across the ok keys' short-window remaining percent
  // (0..100). `null` when no key reports a short window.
  short: number | null
  // Arithmetic mean across the ok keys' long-window remaining percent.
  long: number | null
  // OpenRouter remaining credit in USD. `null` when neither the key limit
  // nor the account credit totals are reported.
  openrouterBalance: number | null
  // OpenRouter remaining-limit ratio (0..100) used only for the pill color.
  openrouterRemaining: number | null
  // OpenRouter provider-reported spend today (USD).
  openrouterDailySpend: number | null
  // DeepSeek account balance and its `is_available` routing flag.
  deepseekTotal: number | null
  deepseekCurrency: string | null
  deepseekAvailable: boolean | null
  // Locally aggregated AI tokens for the endpoint since UTC midnight.
  localTodayTokens: number | null
}

export type TokenPlanBadges = TokenPlanPillSource & {
  // Raw cached payload — exposed so the popover can re-render the
  // breakdown without re-fetching.
  usage: TokenPlanUsageResponse | null
}

// One key's derived values. Not exported: with the P3 rotation gone the
// per-key breakdown is only an intermediate of the aggregate.
type KeyBadges = {
  ok: boolean
  short: number | null
  long: number | null
  openrouterRemaining: number | null
  openrouterBalance: number | null
  openrouterDailySpend: number | null
  deepseekTotal: number | null
  deepseekCurrency: string | null
  deepseekAvailable: boolean | null
}

const EMPTY_SOURCE: TokenPlanPillSource = {
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
}

const EMPTY_BADGES: TokenPlanBadges = { ...EMPTY_SOURCE, usage: null }

function finiteNumber(value: number | null | undefined): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function mean(values: number[]): number | null {
  if (values.length === 0) return null
  return values.reduce((sum, value) => sum + value, 0) / values.length
}

function badgeMode(provider: string): TokenPlanBadgeMode {
  if (provider === 'openrouter') return 'openrouter'
  if (provider === 'deepseek') return 'deepseek'
  return 'window'
}

// Short window: min(5h / rolling / interval) within one key. MiniMax
// surfaces each model_remains entry separately so we iterate them;
// CommandCode / OpencodeGo / GLM each carry a single short-window slot.
// We deliberately exclude OpenRouter (credit-balance-only, handled
// separately) and any empty `remaining_percent`.
function keyShortPercent(key: TokenPlanKeyUsage): number | null {
  const candidates: number[] = []
  for (const model of key.model_remains) {
    const window = model.interval as TokenPlanWindowUsage | null | undefined
    if (window && window.remaining_percent != null) {
      candidates.push(remainingPercent(window))
    }
  }
  const five = ccAsWindow(key.five_hour)
  if (five && five.remaining_percent != null) {
    candidates.push(remainingPercent(five))
  }
  const rolling = opencodeGoAsWindow(key.opencodego_rolling)
  if (rolling && rolling.remaining_percent != null) {
    candidates.push(remainingPercent(rolling))
  }
  const glmFive = glmAsWindow(key.glm_five_hour)
  if (glmFive && glmFive.remaining_percent != null) {
    candidates.push(remainingPercent(glmFive))
  }
  return candidates.length === 0 ? null : Math.min(...candidates)
}

// Long window: min(weekly, monthly) within one key. MiniMax weekly,
// CommandCode weekly (USD), OpencodeGo weekly+monthly, GLM weekly.
function keyLongPercent(key: TokenPlanKeyUsage): number | null {
  const candidates: number[] = []
  for (const model of key.model_remains) {
    const window = model.weekly as TokenPlanWindowUsage | null | undefined
    if (window && window.remaining_percent != null) {
      candidates.push(remainingPercent(window))
    }
  }
  const weekly = ccAsWindow(key.weekly)
  if (weekly && weekly.remaining_percent != null) {
    candidates.push(remainingPercent(weekly))
  }
  for (const window of [
    opencodeGoAsWindow(key.opencodego_weekly),
    opencodeGoAsWindow(key.opencodego_monthly),
    glmAsWindow(key.glm_weekly),
  ]) {
    if (window && window.remaining_percent != null) {
      candidates.push(remainingPercent(window))
    }
  }
  return candidates.length === 0 ? null : Math.min(...candidates)
}

// OpenRouter's remaining credit and today spend. `limit_remaining` is the
// remaining credit on the key cap; when the key is unlimited (limit null)
// we fall back to the management-key credit totals difference.
function openrouterSignal(key: TokenPlanKeyUsage): {
  remaining: number | null
  balance: number | null
  daily: number | null
} {
  const bal = key.openrouter_balance
  const spend = key.openrouter_spend
  if (!bal && !spend) {
    return { remaining: null, balance: null, daily: null }
  }
  const limit = finiteNumber(bal?.limit)
  const limitRemaining = finiteNumber(bal?.limit_remaining)
  const totalCredits = finiteNumber(bal?.total_credits)
  const totalUsage = finiteNumber(bal?.total_usage)
  const balance =
    limitRemaining ??
    (totalCredits !== null && totalUsage !== null
      ? totalCredits - totalUsage
      : null)
  const remaining =
    limit !== null && limitRemaining !== null && limit > 0
      ? Math.max(0, Math.min(100, (limitRemaining / limit) * 100))
      : null
  return { remaining, balance, daily: finiteNumber(spend?.daily) }
}

function computeKeyBadges(key: TokenPlanKeyUsage): KeyBadges {
  if (!key.ok) {
    return {
      ok: false,
      short: null,
      long: null,
      openrouterRemaining: null,
      openrouterBalance: null,
      openrouterDailySpend: null,
      deepseekTotal: null,
      deepseekCurrency: null,
      deepseekAvailable: null,
    }
  }
  const or = openrouterSignal(key)
  const bal = key.deepseek_balance
  return {
    ok: true,
    short: keyShortPercent(key),
    long: keyLongPercent(key),
    openrouterRemaining: or.remaining,
    openrouterBalance: or.balance,
    openrouterDailySpend: or.daily,
    deepseekTotal: finiteNumber(bal?.total_balance),
    deepseekCurrency: bal?.currency ?? null,
    deepseekAvailable: bal ? bal.is_available : null,
  }
}

function computeBadges(usage: TokenPlanUsageResponse | null): TokenPlanBadges {
  if (!usage) return EMPTY_BADGES
  const mode = badgeMode(usage.provider)
  const shorts: number[] = []
  const longs: number[] = []
  let openrouter: KeyBadges | null = null
  let deepseek: KeyBadges | null = null
  for (const key of usage.keys.map(computeKeyBadges)) {
    if (!key.ok) continue
    // Window providers: arithmetic mean across every key that reports the
    // window (missing windows are skipped, not treated as zero).
    if (key.short !== null) shorts.push(key.short)
    if (key.long !== null) longs.push(key.long)
    // Balance providers keep the first ok key that carries a signal, matching
    // the pre-P2 single-pill behavior (an account balance is not averaged).
    if (
      openrouter === null &&
      (key.openrouterBalance !== null ||
        key.openrouterRemaining !== null ||
        key.openrouterDailySpend !== null)
    ) {
      openrouter = key
    }
    if (deepseek === null && key.deepseekTotal !== null) deepseek = key
  }
  return {
    mode,
    short: mean(shorts),
    long: mean(longs),
    openrouterBalance: openrouter?.openrouterBalance ?? null,
    openrouterRemaining: openrouter?.openrouterRemaining ?? null,
    openrouterDailySpend: openrouter?.openrouterDailySpend ?? null,
    deepseekTotal: deepseek?.deepseekTotal ?? null,
    deepseekCurrency: deepseek?.deepseekCurrency ?? null,
    deepseekAvailable: deepseek?.deepseekAvailable ?? null,
    localTodayTokens: finiteNumber(usage.local_today_tokens),
    usage,
  }
}

// Reactive accessor for a single endpoint's badges. Reads the module
// cache synchronously and triggers a prefetch on the first observation
// so the badges become non-empty within one network round-trip after
// the table mounts. Accepts either a static string or a ref so the
// caller can pick whichever shape is convenient.
export function useTokenPlanBadges(
  endpointId: string | Ref<string>,
): ComputedRef<TokenPlanBadges> {
  const idRef = computed(() =>
    typeof endpointId === 'string' ? endpointId : endpointId.value,
  )
  // Kick off the prefetch eagerly so consumers that only read the ref
  // (no `watch`) still get fresh data on the next tick. The cache is
  // idempotent — re-mounting is a no-op for fresh entries.
  void prefetchTokenPlanUsage(idRef.value)
  return computed(() => computeBadges(getCachedTokenPlanUsage(idRef.value)))
}

// Re-export the adapt helpers so the badge template can color its pills
// the same way the dialog colors its progress bars.
export { progressColor, remainingPercent }

// `progressColor` expects a TokenPlanWindowUsage; the badge already holds
// the remaining percent, so we synthesize one with
// `remaining_percent = percent` and let progressColor fold used =
// 100 - remaining internally to drive the hue ramp. Shared by the desktop
// table and the mobile card.
export function badgeColorForPercent(percent: number): string {
  return progressColor({ end_at: null, remaining_percent: percent })
}

// Display descriptor for one badge pill. Keeping the label/color/title
// construction here means the desktop table and the mobile card only own
// their markup, not the provider-specific folding.
export type TokenPlanBadgePill = {
  label: string
  color: string
  title: string
}

// Build the pill descriptors for one endpoint. Window providers emit a
// short/long pair (each slot omitted when no key reports that window);
// balance providers emit a balance + today-usage pair.
export function tokenPlanBadgePills(
  source: TokenPlanPillSource,
  t: TranslateFn,
): TokenPlanBadgePill[] {
  if (source.mode === 'openrouter') {
    const pills: TokenPlanBadgePill[] = [
      {
        label:
          source.openrouterBalance !== null
            ? `${t('tokenPlanOpenRouterRemaining')} ${formatMoney('USD', source.openrouterBalance)}`
            : `${t('tokenPlanOpenRouterRemaining')} ${t('tokenPlanNoQuota')}`,
        color:
          source.openrouterRemaining !== null
            ? badgeColorForPercent(source.openrouterRemaining)
            : '',
        title: t('tokenPlanOpenRouterBalanceHint'),
      },
    ]
    // Provider-reported spend wins; the local aggregate is the fallback.
    if (source.openrouterDailySpend !== null) {
      pills.push({
        label: `${t('tokenPlanSpendDaily')} ${formatMoney('USD', source.openrouterDailySpend)}`,
        color: '',
        title: t('tokenPlanOpenRouterSpendHint'),
      })
    } else if (source.localTodayTokens !== null) {
      pills.push(localTodayPill(source.localTodayTokens, t))
    }
    return pills
  }
  if (source.mode === 'deepseek') {
    const pills: TokenPlanBadgePill[] = []
    if (source.deepseekTotal !== null) {
      pills.push({
        label: `${t('tokenPlanDeepSeekBalance')} ${formatMoney(source.deepseekCurrency, source.deepseekTotal)}`,
        color: badgeColorForPercent(
          source.deepseekAvailable === false ? 0 : 100,
        ),
        title: t('tokenPlanDeepSeekBalanceHint'),
      })
    }
    if (source.localTodayTokens !== null) {
      pills.push(localTodayPill(source.localTodayTokens, t))
    }
    return pills
  }
  const pills: TokenPlanBadgePill[] = []
  if (source.short !== null) {
    pills.push({
      label: `${t('tokenPlanShortBadge')} ${source.short.toFixed(0)}%`,
      color: badgeColorForPercent(source.short),
      title: t('tokenPlanShortBadgeHint'),
    })
  }
  if (source.long !== null) {
    pills.push({
      label: `${t('tokenPlanLongBadge')} ${source.long.toFixed(0)}%`,
      color: badgeColorForPercent(source.long),
      title: t('tokenPlanLongBadgeHint'),
    })
  }
  return pills
}

function localTodayPill(tokens: number, t: TranslateFn): TokenPlanBadgePill {
  return {
    label: `${t('tokenPlanLocalTodayTokens')} ${formatTokenQuantity(tokens)}`,
    color: '',
    title: t('tokenPlanLocalTodayTokensHint'),
  }
}
