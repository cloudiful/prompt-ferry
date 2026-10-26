import type {
  EndpointProvider,
  TokenPlanKeyUsage,
  TokenPlanUsageResponse,
} from '@/generated/admin-api'
import { formatDateTime } from '@/format/datetime-core'
import { formatTokenQuantity } from '@/format/quantity'
import type { Locale } from '@/i18n'
import {
  deepseekBalanceAmounts,
  deepseekBalanceState,
  type DeepSeekBalanceAmount,
  type DeepSeekBalanceState,
  type TokenPlanFetchState,
} from '@/models/endpoints/quota'
import {
  formatMoney,
  progressColor,
} from '@/models/endpoints/token-plan-window'
import {
  summarizeTokenPlanQuota,
  type TokenPlanQuotaSummaryState,
} from '@/composables/tokenPlanQuotaSummary'

export type TokenPlanBadgeMode = 'window' | 'openrouter' | 'deepseek'

export type TokenPlanPillSource = {
  mode: TokenPlanBadgeMode
  status: TokenPlanFetchState
  quotaState?: TokenPlanQuotaSummaryState
  quotaPartial?: boolean
  quotaMetadataPresent?: boolean
  short: number | null
  long: number | null
  openrouterBalance: number | null
  openrouterRemaining: number | null
  openrouterDailySpend: number | null
  deepseekBalances: DeepSeekBalanceAmount[]
  deepseekAvailable: boolean | null
  deepseekState: DeepSeekBalanceState
  localTodayTokens: number | null
  commandCodeExhausted: boolean
  cooldownUntil?: string | null
}

export type TokenPlanBadges = TokenPlanPillSource & {
  usage: TokenPlanUsageResponse | null
}

export type QuotaBadgeSnapshot = {
  state: TokenPlanFetchState
  usage: TokenPlanUsageResponse | null
}

export type TokenPlanBadgePill = {
  label: string
  color: string
  title: string
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

function badgeMode(provider: EndpointProvider): TokenPlanBadgeMode {
  if (provider === 'openrouter') return 'openrouter'
  if (provider === 'deepseek') return 'deepseek'
  return 'window'
}

function openrouterSignal(key: TokenPlanKeyUsage): {
  remaining: number | null
  balance: number | null
  daily: number | null
} {
  const balance = key.openrouter_balance
  const spend = key.openrouter_spend
  if (!balance && !spend) {
    return { remaining: null, balance: null, daily: null }
  }
  const limit = finiteNumber(balance?.limit)
  const limitRemaining = finiteNumber(balance?.limit_remaining)
  const totalCredits = finiteNumber(balance?.total_credits)
  const totalUsage = finiteNumber(balance?.total_usage)
  const remainingBalance =
    limitRemaining ??
    (totalCredits !== null && totalUsage !== null
      ? totalCredits - totalUsage
      : null)
  const remaining =
    limit !== null && limitRemaining !== null && limit > 0
      ? Math.max(0, Math.min(100, (limitRemaining / limit) * 100))
      : null
  return {
    remaining,
    balance: remainingBalance,
    daily: finiteNumber(spend?.daily),
  }
}

type KeyBadges = {
  ok: boolean
  openrouterRemaining: number | null
  openrouterBalance: number | null
  openrouterDailySpend: number | null
  deepseekBalances: DeepSeekBalanceAmount[]
  deepseekReported: boolean
  deepseekAvailable: boolean | null
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
  const openrouter = openrouterSignal(key)
  return {
    ok: true,
    openrouterRemaining: openrouter.remaining,
    openrouterBalance: openrouter.balance,
    openrouterDailySpend: openrouter.daily,
    deepseekBalances: deepseekBalanceAmounts(key.deepseek_balance),
    deepseekReported: key.deepseek_balance != null,
    deepseekAvailable: key.deepseek_balance?.is_available ?? null,
  }
}

export function computeTokenPlanBadges(
  snapshot: QuotaBadgeSnapshot,
): TokenPlanBadges {
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

export function badgeColorForPercent(percent: number): string {
  return progressColor({ end_at: null, remaining_percent: percent })
}

export function tokenPlanBadgePills(
  source: TokenPlanPillSource,
  t: TranslateFn,
  locale: Locale,
  timeZone: string,
): TokenPlanBadgePill[] {
  if (source.status === 'loading' || source.status === 'error') return []
  const state = source.quotaState ?? 'unknown'
  if (source.mode === 'openrouter') {
    const pills: TokenPlanBadgePill[] = []
    const hasBalance =
      source.openrouterBalance !== null || source.openrouterRemaining !== null
    if (state === 'cooling' || state === 'exhausted') {
      pills.push(
        quotaStatePill(state, t, locale, timeZone, source.cooldownUntil),
      )
    } else if (state === 'available' && !hasBalance) {
      pills.push(quotaStatePill('available', t, locale, timeZone))
    } else if (hasBalance) {
      pills.push({
        label:
          source.openrouterBalance !== null
            ? `${t('tokenPlanOpenRouterRemaining')} ${formatMoney('USD', source.openrouterBalance)}`
            : t('tokenPlanBalanceUnknown'),
        color:
          source.openrouterRemaining !== null
            ? badgeColorForPercent(source.openrouterRemaining)
            : '',
        title: t('tokenPlanOpenRouterBalanceHint'),
      })
    } else {
      pills.push(namedPill(t('tokenPlanBalanceUnknown')))
    }
    if (isTransientState(state)) {
      pills.push(
        quotaStatePill(state, t, locale, timeZone, source.cooldownUntil),
      )
    }
    if (source.openrouterDailySpend !== null) {
      pills.push({
        label: `${t('tokenPlanSpendDaily')} ${formatMoney('USD', source.openrouterDailySpend)}`,
        color: '',
        title: t('tokenPlanOpenRouterSpendHint'),
      })
    } else if (source.localTodayTokens !== null) {
      pills.push(localTodayPill(source.localTodayTokens, t, locale))
    }
    if (state === 'partial') {
      pills.push(quotaStatePill('partial', t, locale, timeZone))
    }
    return pills
  }
  if (source.mode === 'deepseek') {
    const pills: TokenPlanBadgePill[] = []
    if (state === 'cooling' || state === 'exhausted') {
      pills.push(
        quotaStatePill(state, t, locale, timeZone, source.cooldownUntil),
      )
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
        pills.push(quotaStatePill('available', t, locale, timeZone))
      }
      if (state === 'partial') {
        pills.push(quotaStatePill('partial', t, locale, timeZone))
      }
      if (isTransientState(state)) {
        pills.push(
          quotaStatePill(state, t, locale, timeZone, source.cooldownUntil),
        )
      }
    }
    if (source.localTodayTokens !== null) {
      pills.push(localTodayPill(source.localTodayTokens, t, locale))
    }
    return pills
  }
  if (state === 'cooling') {
    return [
      quotaStatePill('cooling', t, locale, timeZone, source.cooldownUntil),
    ]
  }
  if (state === 'available' && source.short === null && source.long === null) {
    return [quotaStatePill('available', t, locale, timeZone)]
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
  if (state === 'partial') {
    pills.push(quotaStatePill('partial', t, locale, timeZone))
  } else if (isTransientState(state)) {
    pills.push(quotaStatePill(state, t, locale, timeZone, source.cooldownUntil))
  } else if (state === 'exhausted' && pills.length === 0) {
    pills.push(quotaStatePill('exhausted', t, locale, timeZone))
  }
  return pills
}

function isTransientState(
  state: TokenPlanQuotaSummaryState,
): state is 'rate_limited' | 'upstream_unavailable' {
  return state === 'rate_limited' || state === 'upstream_unavailable'
}

function localTodayPill(
  tokens: number,
  t: TranslateFn,
  locale: Locale,
): TokenPlanBadgePill {
  return {
    label: `${t('tokenPlanLocalTodayTokens')} ${formatTokenQuantity(tokens, locale)}`,
    color: '',
    title: t('tokenPlanLocalTodayTokensHint'),
  }
}

type TokenPlanBadgeState = Exclude<TokenPlanQuotaSummaryState, 'unknown'>

function quotaStatePill(
  state: TokenPlanBadgeState,
  t: TranslateFn,
  locale: Locale,
  timeZone: string,
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
              : t('tokenPlanQuotaPartial')
  const pill = namedPill(
    label,
    state === 'exhausted'
      ? badgeColorForPercent(0)
      : state === 'available'
        ? badgeColorForPercent(100)
        : '',
  )
  return cooldownUntil
    ? {
        ...pill,
        title: `${pill.title} · ${t('quotaRetryAt', {
          time: formatDateTime(cooldownUntil, locale, undefined, timeZone),
        })}`,
      }
    : pill
}

function namedPill(label: string, color = ''): TokenPlanBadgePill {
  return { label, color, title: label }
}
