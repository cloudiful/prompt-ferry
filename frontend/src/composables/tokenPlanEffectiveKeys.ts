import type {
  TokenPlanKeyUsage,
  TokenPlanQuotaState,
  TokenPlanQuotaStatus,
} from '@/generated/admin-api'
import type { TokenPlanFetchState } from '@/models/endpoints/quota'

// States produced by feedback rather than by a provider snapshot. They carry a
// retry deadline; once that deadline has passed the block is over and the badge
// must not keep claiming it until a refresh proves otherwise.
const FEEDBACK_STATES: ReadonlySet<TokenPlanQuotaState> = new Set([
  'cooling',
  'rate_limited',
  'upstream_unavailable',
])

export function isFeedbackState(
  state: TokenPlanQuotaState | undefined,
): boolean {
  return state != null && FEEDBACK_STATES.has(state)
}

// The status this key shows right now: an expired feedback deadline is demoted
// to unknown instead of a blocked state, so an elapsed backoff never looks like
// a live one.
export function effectiveQuotaStatus(
  key: TokenPlanKeyUsage,
  now: number = Date.now(),
): TokenPlanQuotaStatus | null {
  const status = key.quota_status
  if (status == null) return null
  if (!isFeedbackState(status.state)) return status
  const until = status.cooldown_until
  if (until == null) return status
  const deadline = Date.parse(until)
  if (!Number.isFinite(deadline) || deadline > now) return status
  return { ...status, state: 'unknown', cooldown_until: null }
}

export type EffectiveTokenPlanKey = {
  key: TokenPlanKeyUsage
  state: TokenPlanQuotaState
  usable: boolean
  legacy: boolean
  // Whether this key still carries provider capacity evidence for the badge
  // percentages. Throttling and an unavailable upstream block routing without
  // reducing the account's quota, so their windows stay visible; a confirmed
  // exhaustion does not.
  hasCapacity: boolean
}

export function tokenPlanEffectiveKeys(
  keys: readonly TokenPlanKeyUsage[],
  fetchState: TokenPlanFetchState,
  now: number = Date.now(),
): EffectiveTokenPlanKey[] {
  return keys.map((key) => {
    const legacy = key.quota_status == null
    const metadata = effectiveQuotaStatus(key, now)
    // Stale retains last-known numeric evidence with deadline-correct status
    // but never counts as usable/current: the summary stays unknown unless a
    // live feedback block is retained, and expired feedback stays demoted. A
    // stale non-feedback observation (exhausted/available/unknown) is not a
    // current claim, so it projects to unknown; exhausted, available, and
    // expired feedback keep their historical windows as evidence while
    // unknown keeps none. An absent window still yields null downstream.
    if (fetchState === 'stale') {
      if (metadata?.scope === 'text_inference') {
        const rawState = key.quota_status?.state
        const state = metadata.state
        if (
          rawState != null &&
          isFeedbackState(rawState) &&
          state === rawState
        ) {
          return {
            key,
            state,
            usable: false,
            legacy: false,
            hasCapacity:
              state === 'rate_limited' || state === 'upstream_unavailable',
          }
        }
        return {
          key,
          state: 'unknown',
          usable: false,
          legacy: false,
          hasCapacity: rawState != null && rawState !== 'unknown',
        }
      }
      return {
        key,
        state: 'unknown',
        usable: false,
        legacy,
        hasCapacity: legacy && key.ok,
      }
    }
    if (fetchState !== 'ready') {
      return {
        key,
        state: 'unknown',
        usable: false,
        legacy,
        hasCapacity: false,
      }
    }
    if (metadata?.scope === 'text_inference') {
      const state = metadata.state
      return {
        key,
        state,
        usable: state === 'available',
        legacy: false,
        hasCapacity:
          state === 'available' ||
          state === 'rate_limited' ||
          state === 'upstream_unavailable',
      }
    }
    return {
      key,
      state: 'unknown',
      usable: legacy && key.ok,
      legacy,
      hasCapacity: legacy && key.ok,
    }
  })
}
