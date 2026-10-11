import type {
  EndpointProvider,
  TokenPlanKeyUsage,
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
  tokenPlanEffectiveKeys,
  type EffectiveTokenPlanKey,
} from './tokenPlanEffectiveKeys'

export type TokenPlanQuotaSummaryState =
  | 'available'
  | 'exhausted'
  | 'unknown'
  | 'cooling'
  | 'partial'

export type TokenPlanQuotaSummary = {
  state: TokenPlanQuotaSummaryState
  partial: boolean
  short: number | null
  long: number | null
  usableKeys: TokenPlanKeyUsage[]
  allExhausted: boolean
  legacyCommandCodeNoQuota: boolean
}

function defaultTextModels(
  key: TokenPlanKeyUsage,
  provider: EndpointProvider,
) {
  return provider === 'minimax'
    ? key.model_remains.filter((model) =>
        model.model_name.trim().toLowerCase() === 'general',
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
      | TokenPlanWindowUsage
      | null
      | undefined
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
  const commandRemaining = commandCode ? knownRemainingPercent(commandCode) : null
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
  const commandRemaining = commandCode ? knownRemainingPercent(commandCode) : null
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
  return typeof remaining === 'number' && Number.isFinite(remaining) && remaining <= 0
}

function legacyCommandCodeExhausted(keys: readonly TokenPlanKeyUsage[]): boolean {
  const ok = keys.filter((key) => key.ok)
  return (
    ok.length > 0 &&
    ok.every((key) => key.quota_status == null && legacyCommandCodeBlocks(key))
  )
}

function quotaState(keys: EffectiveTokenPlanKey[]): {
  state: TokenPlanQuotaSummaryState
  partial: boolean
  allExhausted: boolean
} {
  const usable = keys.filter((entry) => entry.usable)
  const allExhausted = keys.length > 0 && keys.every((entry) => entry.state === 'exhausted')
  if (usable.length > 0) {
    const hasConfirmedAvailable = usable.some((entry) => !entry.legacy)
    const partial = keys.some(
      (entry) => !entry.usable || entry.state === 'unknown',
    )
    if (!hasConfirmedAvailable) return { state: 'unknown', partial: false, allExhausted }
    return {
      state: partial ? 'partial' : 'available',
      partial,
      allExhausted: false,
    }
  }
  if (allExhausted) return { state: 'exhausted', partial: false, allExhausted }
  if (
    keys.length > 0 &&
    keys.some((entry) => entry.state === 'cooling') &&
    keys.every((entry) => entry.state === 'cooling' || entry.state === 'exhausted')
  ) {
    return { state: 'cooling', partial: false, allExhausted: false }
  }
  return { state: 'unknown', partial: false, allExhausted: false }
}

export function summarizeTokenPlanQuota(
  provider: EndpointProvider,
  keys: readonly TokenPlanKeyUsage[],
  fetchState: TokenPlanFetchState,
): TokenPlanQuotaSummary {
  const effective = tokenPlanEffectiveKeys(keys, fetchState)
  if (provider === 'command_code') {
    for (const entry of effective) {
      if (entry.legacy && legacyCommandCodeBlocks(entry.key)) entry.usable = false
    }
  }
  const state = quotaState(effective)
  const usableKeys = effective.filter((entry) => entry.usable).map((entry) => entry.key)
  const shorts = usableKeys
    .map((key) => keyShortPercent(key, provider))
    .filter((value): value is number => value !== null)
  const longs = usableKeys
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
    legacyCommandCodeNoQuota:
      provider === 'command_code' &&
      fetchState === 'ready' &&
      legacyCommandCodeExhausted(keys),
  }
}
