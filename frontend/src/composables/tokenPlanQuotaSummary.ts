import type {
  EndpointProvider,
  TokenPlanKeyUsage,
  TokenPlanQuotaState,
  TokenPlanWindowUsage,
} from '@/generated/admin-api'
import type { TokenPlanFetchState } from '@/models/endpoints/quota'
import {
  ccAsWindow,
  glmAsWindow,
  opencodeGoAsWindow,
  knownRemainingPercent,
} from './useTokenPlanWindowEntries'
import { subscriptionQuotaMinimumForBand } from './subscriptionQuotaWindows'
import {
  isFeedbackState,
  tokenPlanEffectiveKeys,
  type EffectiveTokenPlanKey,
} from './tokenPlanEffectiveKeys'

export type TokenPlanQuotaSummaryState =
  | 'available'
  | 'exhausted'
  | 'unknown'
  | 'cooling'
  | 'rate_limited'
  | 'upstream_unavailable'
  | 'partial'

export type TokenPlanQuotaSummary = {
  state: TokenPlanQuotaSummaryState
  partial: boolean
  short: number | null
  long: number | null
  usableKeys: TokenPlanKeyUsage[]
  // Keys carrying numeric evidence (fresh capacity or stale historical
  // evidence). Balances/percentages stay visible from these while usability
  // stays false for stale, so stale never claims live capacity.
  capacityKeys: TokenPlanKeyUsage[]
  allExhausted: boolean
  legacyCommandCodeNoQuota: boolean
  // Latest retry deadline among the keys an active feedback block applies to.
  cooldownUntil: string | null
}

// States that mean routing refuses the key, whatever the reason. A usable or
// unknown key is excluded from this set on purpose: a pool that still has one
// is never summarized as blocked.
const BLOCKED_STATES: ReadonlySet<TokenPlanQuotaState> = new Set([
  'cooling',
  'exhausted',
  'rate_limited',
  'upstream_unavailable',
])

// Which cause names an entirely blocked pool. Exhaustion is handled by
// `allExhausted` first, so a mixed pool never claims every key is spent.
const BLOCKED_PRIORITY: readonly TokenPlanQuotaState[] = [
  'cooling',
  'upstream_unavailable',
  'rate_limited',
]

function defaultTextModels(key: TokenPlanKeyUsage, provider: EndpointProvider) {
  return provider === 'minimax'
    ? key.model_remains.filter(
        (model) => model.model_name.trim().toLowerCase() === 'general',
      )
    : []
}

function modelPercent(
  key: TokenPlanKeyUsage,
  provider: EndpointProvider,
  band: 'short' | 'long',
): number | null {
  const candidates: number[] = []
  for (const model of defaultTextModels(key, provider)) {
    if (model.windows != null) continue
    const window = (band === 'short' ? model.interval : model.weekly) as
      TokenPlanWindowUsage | null | undefined
    const remaining = window ? knownRemainingPercent(window) : null
    if (remaining !== null) candidates.push(remaining)
  }
  return candidates.length === 0 ? null : Math.min(...candidates)
}

export function keyShortPercent(
  key: TokenPlanKeyUsage,
  provider: EndpointProvider,
): number | null {
  const candidates: number[] = []
  const model = modelPercent(key, provider, 'short')
  if (model !== null) candidates.push(model)
  const subscription = subscriptionQuotaMinimumForBand(key, 'short')
  if (subscription !== null) candidates.push(subscription)
  const commandCode = ccAsWindow(key.five_hour)
  const commandRemaining = commandCode
    ? knownRemainingPercent(commandCode)
    : null
  if (commandRemaining !== null) candidates.push(commandRemaining)
  const opencodeGo = opencodeGoAsWindow(key.opencodego_rolling)
  const goRemaining = opencodeGo ? knownRemainingPercent(opencodeGo) : null
  if (goRemaining !== null) candidates.push(goRemaining)
  const glm = glmAsWindow(key.glm_five_hour)
  const glmRemaining = glm ? knownRemainingPercent(glm) : null
  if (glmRemaining !== null) candidates.push(glmRemaining)
  return candidates.length === 0 ? null : Math.min(...candidates)
}

export function keyLongPercent(
  key: TokenPlanKeyUsage,
  provider: EndpointProvider,
): number | null {
  const candidates: number[] = []
  const model = modelPercent(key, provider, 'long')
  if (model !== null) candidates.push(model)
  const subscription = subscriptionQuotaMinimumForBand(key, 'long')
  if (subscription !== null) candidates.push(subscription)
  const commandCode = ccAsWindow(key.weekly)
  const commandRemaining = commandCode
    ? knownRemainingPercent(commandCode)
    : null
  if (commandRemaining !== null) candidates.push(commandRemaining)
  for (const window of [
    opencodeGoAsWindow(key.opencodego_weekly),
    opencodeGoAsWindow(key.opencodego_monthly),
    glmAsWindow(key.glm_weekly),
  ]) {
    const remaining = window ? knownRemainingPercent(window) : null
    if (remaining !== null) candidates.push(remaining)
  }
  return candidates.length === 0 ? null : Math.min(...candidates)
}

function mean(values: number[]): number | null {
  return values.length === 0
    ? null
    : values.reduce((sum, value) => sum + value, 0) / values.length
}

function legacyCommandCodeBlocks(key: TokenPlanKeyUsage): boolean {
  const remaining = key.balances?.remaining_credits
  return (
    typeof remaining === 'number' &&
    Number.isFinite(remaining) &&
    remaining <= 0
  )
}

function legacyCommandCodeExhausted(
  keys: readonly TokenPlanKeyUsage[],
): boolean {
  const ok = keys.filter((key) => key.ok)
  return (
    ok.length > 0 &&
    ok.every((key) => key.quota_status == null && legacyCommandCodeBlocks(key))
  )
}

function quotaState(
  keys: EffectiveTokenPlanKey[],
  fetchState: TokenPlanFetchState,
): {
  state: TokenPlanQuotaSummaryState
  partial: boolean
  allExhausted: boolean
} {
  const usable = keys.filter((entry) => entry.usable)
  // A stale observation is historical evidence, never a current exhaustion
  // claim: allExhausted stays false outside ready even when every projected
  // state happens to read exhausted.
  const allExhausted =
    fetchState === 'ready' &&
    keys.length > 0 &&
    keys.every((entry) => entry.state === 'exhausted')
  if (usable.length > 0) {
    const hasConfirmedAvailable = usable.some((entry) => !entry.legacy)
    const partial = keys.some(
      (entry) => !entry.usable || entry.state === 'unknown',
    )
    if (!hasConfirmedAvailable)
      return { state: 'unknown', partial: false, allExhausted }
    return {
      state: partial ? 'partial' : 'available',
      partial,
      allExhausted: false,
    }
  }
  if (allExhausted) return { state: 'exhausted', partial: false, allExhausted }
  // No usable and no unknown key: every key is blocked. Name the strongest
  // cause present rather than flattening them into an exhausted pool.
  if (
    keys.length > 0 &&
    keys.every((entry) => BLOCKED_STATES.has(entry.state))
  ) {
    const state =
      BLOCKED_PRIORITY.find((candidate) =>
        keys.some((entry) => entry.state === candidate),
      ) ?? 'cooling'
    return { state, partial: false, allExhausted: false }
  }
  return { state: 'unknown', partial: false, allExhausted: false }
}

function latestCooldownUntil(keys: EffectiveTokenPlanKey[]): string | null {
  return (
    keys
      .filter((entry) => isFeedbackState(entry.state))
      .map((entry) => entry.key.quota_status?.cooldown_until ?? null)
      .filter((value): value is string => typeof value === 'string')
      .sort()
      .at(-1) ?? null
  )
}

export function summarizeTokenPlanQuota(
  provider: EndpointProvider,
  keys: readonly TokenPlanKeyUsage[],
  fetchState: TokenPlanFetchState,
): TokenPlanQuotaSummary {
  const effective = tokenPlanEffectiveKeys(keys, fetchState)
  if (provider === 'command_code') {
    for (const entry of effective) {
      // A blocked CommandCode key contributes no capacity either: its empty
      // balance must not inflate the window averages.
      if (entry.legacy && legacyCommandCodeBlocks(entry.key)) {
        entry.usable = false
        entry.hasCapacity = false
      }
    }
  }
  const state = quotaState(effective, fetchState)
  const usableKeys = effective
    .filter((entry) => entry.usable)
    .map((entry) => entry.key)
  // A throttled or temporarily unavailable key still reports its provider
  // windows, so those percentages stay visible next to the transient badge.
  // Stale historical evidence joins the same set while usability stays false.
  const capacityKeys = effective
    .filter((entry) => entry.hasCapacity)
    .map((entry) => entry.key)
  const shorts = capacityKeys
    .map((key) => keyShortPercent(key, provider))
    .filter((value): value is number => value !== null)
  const longs = capacityKeys
    .map((key) => keyLongPercent(key, provider))
    .filter((value): value is number => value !== null)
  let short = mean(shorts)
  let long = mean(longs)
  if (state.allExhausted) {
    if (keys.some((key) => keyShortPercent(key, provider) !== null)) short = 0
    if (keys.some((key) => keyLongPercent(key, provider) !== null)) long = 0
  }
  return {
    ...state,
    short,
    long,
    usableKeys,
    capacityKeys,
    legacyCommandCodeNoQuota:
      provider === 'command_code' &&
      fetchState === 'ready' &&
      legacyCommandCodeExhausted(keys),
    cooldownUntil: latestCooldownUntil(effective),
  }
}
