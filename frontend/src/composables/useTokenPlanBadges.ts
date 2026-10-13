import { computed, type ComputedRef, type Ref } from 'vue'
import type {
  TokenPlanKeyUsage,
  TokenPlanUsageResponse,
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
  formatMoney,
  progressColor,
  remainingPercent,
} from './useTokenPlanWindowEntries'
import {
  keyLongPercent,
  keyShortPercent,
  summarizeTokenPlanQuota,
  type TokenPlanQuotaSummaryState,
} from './tokenPlanQuotaSummary'
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
  quotaState?: TokenPlanQuotaSummaryState
  quotaPartial?: boolean
  quotaMetadataPresent?: boolean
  // Arithmetic mean across the effective keys' short-window remaining percent
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
  // Latest retry deadline of the active feedback block, shown in the tooltip
  // of a transient badge. Optional so a source without it still renders.
  cooldownUntil?: string | null
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
  openrouterRemaining: number | null
  openrouterBalance: number | null
  openrouterDailySpend: number | null
  deepseekBalances: DeepSeekBalanceAmount[]
  // `true` when the key reports a `deepseek_balance` at all, so an
  // all-unknown balance still surfaces the unknown state instead of looking
  // like a key that never reported one.
  deepseekReported: boolean
  deepseekAvailable: boolean | null
}

const EMPTY_SOURCE: TokenPlanPillSource = {
  mode: 'window',
  status: 'loading',
  quotaState: 'unknown',
  quotaPartial: false,
  quotaMetadataPresent: false,
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
  cooldownUntil: null,
}

function finiteNumber(value: number | null | undefined): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function badgeMode(provider: string): TokenPlanBadgeMode {
  if (provider === 'openrouter') return 'openrouter'
  if (provider === 'deepseek') return 'deepseek'
  return 'window'
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
      openrouterRemaining: null,
      openrouterBalance: null,
      openrouterDailySpend: null,
      deepseekBalances: [],
      deepseekReported: false,
      deepseekAvailable: null,
    }
  }
  const or = openrouterSignal(key)
  return {
    ok: true,
    openrouterRemaining: or.remaining,
    openrouterBalance: or.balance,
    openrouterDailySpend: or.daily,
    // DeepSeek carries every currency entry through as reported: no FX
    // conversion, no cross-currency sum, an unknown amount left `null`, and
    // `is_available` read independently of the amounts.
    deepseekBalances: deepseekBalanceAmounts(key.deepseek_balance),
    deepseekReported: key.deepseek_balance != null,
    deepseekAvailable: key.deepseek_balance?.is_available ?? null,
  }
}

function computeBadges(snapshot: TokenPlanUsageSnapshot): TokenPlanBadges {
  const status = snapshot.state
  if (snapshot.usage === null) {
    return {
      ...EMPTY_SOURCE,
      status,
      quotaState: 'unknown',
      quotaPartial: false,
      deepseekState: deepseekBalanceState(status, [], null),
      usage: null,
    }
  }
  const usage = snapshot.usage
  const summary = summarizeTokenPlanQuota(usage.provider, usage.keys, status)
  const mode = badgeMode(usage.provider)
  // Stale keeps last-known balances from capacity evidence (usable is empty
  // by definition); fresh keeps the usable-only behavior unchanged.
  const balanceKeys =
    status === 'stale' ? summary.capacityKeys : summary.usableKeys
  let openrouter: KeyBadges | null = null
  let deepseek: KeyBadges | null = null
  let deepseekUnknown: KeyBadges | null = null
  for (const key of balanceKeys.map(computeKeyBadges)) {
    if (!key.ok) continue
    if (
      openrouter === null &&
      (key.openrouterBalance !== null ||
        key.openrouterRemaining !== null ||
        key.openrouterDailySpend !== null)
    ) {
      openrouter = key
    }
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
  const deepseekAvailable =
    deepseekKey?.deepseekAvailable ??
    (summary.state === 'exhausted' ? false : null)
  return {
    mode,
    status,
    quotaState: summary.state,
    quotaPartial: summary.partial,
    quotaMetadataPresent: usage.keys.some((key) => key.quota_status != null),
    short: summary.short,
    long: summary.long,
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
    commandCodeExhausted:
      usage.provider === 'command_code' &&
      (summary.allExhausted || summary.legacyCommandCodeNoQuota),
    cooldownUntil: summary.cooldownUntil,
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
  if (source.status === 'loading' || source.status === 'error') return []
  const state = source.quotaState ?? 'unknown'
  if (source.mode === 'openrouter') {
    const pills: TokenPlanBadgePill[] = []
    const hasBalance =
      source.openrouterBalance !== null || source.openrouterRemaining !== null
    if (state === 'cooling' || state === 'exhausted') {
      pills.push(quotaStatePill(state, t, source.cooldownUntil))
    } else if (state === 'unknown' && !hasBalance) {
      pills.push(quotaStatePill('unknown', t))
    } else if (state === 'available' && !hasBalance) {
      pills.push(quotaStatePill('available', t))
    } else {
      pills.push({
        label:
          source.openrouterBalance !== null
            ? `${t('tokenPlanOpenRouterRemaining')} ${formatMoney('USD', source.openrouterBalance)}`
            : `${t('tokenPlanOpenRouterRemaining')} ${t('tokenPlanQuotaUnknown')}`,
        color:
          source.openrouterRemaining !== null
            ? badgeColorForPercent(source.openrouterRemaining)
            : '',
        title: t('tokenPlanOpenRouterBalanceHint'),
      })
      if (
        state === 'unknown' &&
        (source.quotaMetadataPresent || source.status === 'stale')
      ) {
        pills.push(quotaStatePill('unknown', t))
      }
    }
    if (isTransientState(state)) {
      pills.push(quotaStatePill(state, t, source.cooldownUntil))
    }
    if (source.openrouterDailySpend !== null) {
      pills.push({
        label: `${t('tokenPlanSpendDaily')} ${formatMoney('USD', source.openrouterDailySpend)}`,
        color: '',
        title: t('tokenPlanOpenRouterSpendHint'),
      })
    } else if (source.localTodayTokens !== null) {
      pills.push(localTodayPill(source.localTodayTokens, t))
    }
    if (state === 'partial') pills.push(quotaStatePill('partial', t))
    return pills
  }
  if (source.mode === 'deepseek') {
    const pills: TokenPlanBadgePill[] = []
    // Stale with known amounts shows both: the last-known balance plus an
    // explicit unknown/stale hint, never the amount as fresh capacity.
    const hasKnownDeepseek = source.deepseekBalances.some(
      (amount) => amount.total !== null,
    )
    if (state === 'cooling' || state === 'exhausted') {
      pills.push(quotaStatePill(state, t, source.cooldownUntil))
    } else if (
      state === 'unknown' &&
      (source.quotaMetadataPresent || source.status === 'stale') &&
      !(source.status === 'stale' && hasKnownDeepseek)
    ) {
      pills.push(quotaStatePill('unknown', t))
    } else {
      const legacyUnavailable =
        !source.quotaMetadataPresent && source.deepseekState === 'unavailable'
      const color = legacyUnavailable
        ? badgeColorForPercent(0)
        : state === 'unknown'
          ? ''
          : badgeColorForPercent(100)
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
      if (known === 0 && legacyUnavailable) {
        pills.push(
          namedPill(t('tokenPlanUnavailable'), badgeColorForPercent(0)),
        )
      } else if (known === 0 || known < source.deepseekBalances.length) {
        pills.push(namedPill(t('tokenPlanBalanceUnknown')))
      }
      if (state === 'available' && known === 0) {
        pills.push(quotaStatePill('available', t))
      }
      if (state === 'partial') pills.push(quotaStatePill('partial', t))
      if (isTransientState(state)) {
        pills.push(quotaStatePill(state, t, source.cooldownUntil))
      }
      // Nonexclusive stale adjunct: retained amounts stay visible alongside
      // the explicit unknown hint, never as fresh capacity.
      if (source.status === 'stale' && state === 'unknown') {
        pills.push(quotaStatePill('unknown', t))
      }
    }
    if (source.localTodayTokens !== null) {
      pills.push(localTodayPill(source.localTodayTokens, t))
    }
    return pills
  }
  if (state === 'cooling')
    return [quotaStatePill('cooling', t, source.cooldownUntil)]
  if (state === 'available' && source.short === null && source.long === null) {
    return [quotaStatePill('available', t)]
  }
  const pills: TokenPlanBadgePill[] = []
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
      title:
        state === 'unknown'
          ? t('tokenPlanQuotaUnknown')
          : t('tokenPlanShortBadgeHint'),
    })
  }
  if (source.long !== null) {
    pills.push({
      label: `${t('tokenPlanLongBadge')} ${source.long.toFixed(0)}%`,
      color: badgeColorForPercent(source.long),
      title:
        state === 'unknown'
          ? t('tokenPlanQuotaUnknown')
          : t('tokenPlanLongBadgeHint'),
    })
  }
  if (
    state === 'unknown' &&
    pills.length === 0 &&
    (source.quotaMetadataPresent || source.status === 'stale')
  ) {
    pills.push(quotaStatePill('unknown', t))
  } else if (
    state === 'unknown' &&
    source.status === 'stale' &&
    pills.length > 0
  ) {
    // Stale historical numbers stay visible but never as current capacity:
    // name the unknown observation alongside the retained percentages.
    pills.push(quotaStatePill('unknown', t))
  } else if (state === 'partial') {
    pills.push(quotaStatePill('partial', t))
  } else if (isTransientState(state)) {
    // Throttling and an unavailable upstream never hide the provider
    // percentages: the pill names the block, the windows still describe the
    // account's actual capacity.
    pills.push(quotaStatePill(state, t, source.cooldownUntil))
  } else if (state === 'exhausted' && pills.length === 0) {
    pills.push(quotaStatePill('exhausted', t))
  }
  return pills
}

// A transient feedback state: the key is blocked right now without its quota
// being spent.
function isTransientState(
  state: TokenPlanQuotaSummaryState,
): state is 'rate_limited' | 'upstream_unavailable' {
  return state === 'rate_limited' || state === 'upstream_unavailable'
}

function localTodayPill(tokens: number, t: TranslateFn): TokenPlanBadgePill {
  return {
    label: `${t('tokenPlanLocalTodayTokens')} ${formatTokenQuantity(tokens)}`,
    color: '',
    title: t('tokenPlanLocalTodayTokensHint'),
  }
}

function quotaStatePill(
  state: TokenPlanQuotaSummaryState,
  t: TranslateFn,
  cooldownUntil?: string | null,
): TokenPlanBadgePill {
  const label =
    state === 'available'
      ? t('tokenPlanAvailable')
      : state === 'cooling'
        ? t('tokenPlanQuotaCooling')
        : state === 'rate_limited'
          ? t('tokenPlanRateLimited')
          : state === 'upstream_unavailable'
            ? t('tokenPlanUpstreamUnavailable')
            : state === 'exhausted'
              ? t('tokenPlanUnavailable')
              : state === 'partial'
                ? t('tokenPlanQuotaPartial')
                : t('tokenPlanQuotaUnknown')
  const pill = namedPill(
    label,
    state === 'exhausted'
      ? badgeColorForPercent(0)
      : state === 'available'
        ? badgeColorForPercent(100)
        : '',
  )
  // A blocked key says when routing will try again, using the same deadline
  // template the detail view uses.
  return cooldownUntil
    ? {
        ...pill,
        title: `${pill.title} · ${t('quotaRetryAt', {
          time: new Date(cooldownUntil).toLocaleString(),
        })}`,
      }
    : pill
}

// State pill whose own label is also the tooltip: the balance surface says
// what it knows without inventing an amount to explain.
function namedPill(label: string, color = ''): TokenPlanBadgePill {
  return { label, color, title: label }
}
