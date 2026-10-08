import { computed, type ComputedRef, type Ref } from 'vue'
import type {
  TokenPlanKeyUsage,
  TokenPlanUsageResponse,
  TokenPlanWindowUsage,
} from '@/generated/admin-api'
import {
  deepseekBalanceAmounts,
  deepseekBalanceState,
  type DeepSeekBalanceAmount,
  type DeepSeekBalanceState,
  type TokenPlanFetchState,
} from '@/models/endpoints/quota'
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
  getTokenPlanUsageSnapshot,
  prefetchTokenPlanUsage,
  type TokenPlanUsageSnapshot,
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
  // Fetch state of the shared usage snapshot: `loading` until the first
  // payload lands, `ready` with data, `error` when the cold fetch failed and
  // the negative-cache entry is all the cache holds. Explicit so a failed
  // request is never mistaken for a provider that reports nothing.
  status: TokenPlanFetchState
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
  // Every DeepSeek currency entry the account reports, in the payload's
  // deterministic order and never merged across currencies. An entry whose
  // `total` is `null` is unknown and renders no amount; a reported `0` stays
  // a real zero.
  deepseekBalances: DeepSeekBalanceAmount[]
  // Provider-reported DeepSeek availability, read from `is_available`
  // independently of the amounts above.
  deepseekAvailable: boolean | null
  // Display state of the DeepSeek balance surface: the fetch state first,
  // then provider availability, then whether any amount is known.
  deepseekState: DeepSeekBalanceState
  // Locally aggregated AI tokens for the endpoint since UTC midnight.
  localTodayTokens: number | null
  // CommandCode only (issue #656): `true` when every ok key on the endpoint
  // reports an empty effective credit balance (`balances.remaining_credits <=
  // 0`), i.e. routing has excluded the whole endpoint. The window averages
  // then omit those keys and a no-quota pill replaces the percentages, so the
  // badge never advertises usable 5h/weekly quota that routing will not use.
  commandCodeExhausted: boolean
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
  deepseekBalances: DeepSeekBalanceAmount[]
  // `true` when the key reports a `deepseek_balance` at all, so an
  // all-unknown balance still surfaces the unknown state instead of looking
  // like a key that never reported one.
  deepseekReported: boolean
  deepseekAvailable: boolean | null
  // CommandCode equivalent of the backend `command_code_balance_exhausted`
  // signal: a reported balance with no credits left. `false` when no balances
  // are reported (older/PAYG payloads) so routing-compatible windows survive.
  exhausted: boolean
}

const EMPTY_SOURCE: TokenPlanPillSource = {
  mode: 'window',
  status: 'loading',
  short: null,
  long: null,
  openrouterBalance: null,
  openrouterRemaining: null,
  openrouterDailySpend: null,
  deepseekBalances: [],
  deepseekAvailable: null,
  deepseekState: 'loading',
  localTodayTokens: null,
  commandCodeExhausted: false,
}

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
      deepseekBalances: [],
      deepseekReported: false,
      deepseekAvailable: null,
      exhausted: false,
    }
  }
  const or = openrouterSignal(key)
  // Only CommandCode reports credit balances; a non-positive remaining is the
  // effective exhaustion the backend uses to drop the key from routing.
  const remainingCredits = finiteNumber(key.balances?.remaining_credits)
  return {
    ok: true,
    short: keyShortPercent(key),
    long: keyLongPercent(key),
    openrouterRemaining: or.remaining,
    openrouterBalance: or.balance,
    openrouterDailySpend: or.daily,
    // DeepSeek carries every currency entry through as reported: no FX
    // conversion, no cross-currency sum, an unknown amount left `null`, and
    // `is_available` read independently of the amounts.
    deepseekBalances: deepseekBalanceAmounts(key.deepseek_balance),
    deepseekReported: key.deepseek_balance != null,
    deepseekAvailable: key.deepseek_balance?.is_available ?? null,
    exhausted: remainingCredits !== null && remainingCredits <= 0,
  }
}

function computeBadges(snapshot: TokenPlanUsageSnapshot): TokenPlanBadges {
  const status = snapshot.state
  if (snapshot.usage === null) {
    // No payload: the badge state can only describe the request. Window and
    // OpenRouter pills stay empty exactly as before, so a row that has not
    // loaded yet still renders its placeholder instead of a fabricated zero.
    return {
      ...EMPTY_SOURCE,
      status,
      deepseekState: deepseekBalanceState(status, [], null),
      usage: null,
    }
  }
  const usage = snapshot.usage
  const mode = badgeMode(usage.provider)
  const shorts: number[] = []
  const longs: number[] = []
  let openrouter: KeyBadges | null = null
  let deepseek: KeyBadges | null = null
  let deepseekUnknown: KeyBadges | null = null
  let okKeys = 0
  let exhaustedKeys = 0
  for (const key of usage.keys.map(computeKeyBadges)) {
    if (!key.ok) continue
    okKeys += 1
    // Window providers: arithmetic mean across every key that reports the
    // window (missing windows are skipped, not treated as zero). Exhausted
    // CommandCode keys are routing-excluded, so their windows must not raise
    // the average; when every ok key is exhausted the no-quota pill below
    // takes over instead of a percentage.
    if (key.exhausted) {
      exhaustedKeys += 1
    } else {
      if (key.short !== null) shorts.push(key.short)
      if (key.long !== null) longs.push(key.long)
    }
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
    // The DeepSeek account balance is never summed across keys: a key that
    // reports a known amount wins, and an account whose amounts are all unknown
    // still reports its availability instead of looking absent.
    const knownTotal = key.deepseekBalances.some(
      (amount) => amount.total !== null,
    )
    if (knownTotal && deepseek === null) deepseek = key
    if (!knownTotal && deepseekUnknown === null && key.deepseekReported) {
      deepseekUnknown = key
    }
  }
  const deepseekKey = deepseek ?? deepseekUnknown
  const deepseekBalances = deepseekKey?.deepseekBalances ?? []
  const deepseekAvailable = deepseekKey?.deepseekAvailable ?? null
  return {
    mode,
    status,
    short: mean(shorts),
    long: mean(longs),
    openrouterBalance: openrouter?.openrouterBalance ?? null,
    openrouterRemaining: openrouter?.openrouterRemaining ?? null,
    openrouterDailySpend: openrouter?.openrouterDailySpend ?? null,
    deepseekBalances,
    deepseekAvailable,
    deepseekState: deepseekBalanceState(
      status,
      deepseekBalances,
      deepseekAvailable,
    ),
    localTodayTokens: finiteNumber(usage.local_today_tokens),
    commandCodeExhausted: okKeys > 0 && exhaustedKeys === okKeys,
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
  return computed(() => computeBadges(getTokenPlanUsageSnapshot(idRef.value)))
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
// balance providers emit a balance + today-usage pair. A DeepSeek balance that
// reports no known amount names its state instead of showing an amount.
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
    // One pill per known currency, in the payload's deterministic order and
    // never summed across currencies. An unknown amount contributes no amount;
    // `is_available` only colors the pills, so a real zero under an
    // unavailable account stays visible.
    const color = badgeColorForPercent(
      source.deepseekAvailable === false ? 0 : 100,
    )
    const known = source.deepseekBalances.filter(
      (amount) => amount.total !== null,
    ).length
    for (const amount of source.deepseekBalances) {
      if (amount.total === null) continue
      pills.push({
        label: formatMoney(amount.currency, amount.total),
        color,
        title: t('tokenPlanDeepSeekBalanceHint'),
      })
    }
    // Name the missing information instead of inventing an amount: an account
    // with nothing known says so once, and a mixed payload names the unknown
    // entries alongside the known currencies instead of dropping them.
    // `loading`/`error` need a provider this payload-less badge does not have
    // yet, so they stay with the shared surface that knows the provider.
    if (known === 0) {
      pills.push(
        source.deepseekState === 'unavailable'
          ? namedPill(t('tokenPlanUnavailable'), badgeColorForPercent(0))
          : namedPill(t('tokenPlanBalanceUnknown')),
      )
    } else if (known < source.deepseekBalances.length) {
      pills.push(namedPill(t('tokenPlanBalanceUnknown')))
    }
    if (source.localTodayTokens !== null) {
      pills.push(localTodayPill(source.localTodayTokens, t))
    }
    return pills
  }
  const pills: TokenPlanBadgePill[] = []
  // Issue #656: an endpoint whose CommandCode keys are all out of effective
  // monthly credits is excluded from routing, so a 0%-colored no-quota pill
  // replaces the window percentages instead of advertising unusable quota.
  if (source.commandCodeExhausted) {
    return [
      {
        label: t('tokenPlanNoQuota'),
        color: badgeColorForPercent(0),
        title: t('tokenPlanUnavailable'),
      },
    ]
  }
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

// State pill whose own label is also the tooltip: the balance surface says
// what it knows without inventing an amount to explain.
function namedPill(label: string, color = ''): TokenPlanBadgePill {
  return { label, color, title: label }
}
