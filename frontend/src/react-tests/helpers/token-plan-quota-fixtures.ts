import type {
  SubscriptionWindowUsage,
  TokenPlanKeyUsage,
  TokenPlanQuotaState,
  TokenPlanUsageResponse,
  TokenPlanWindowUsage,
} from '@/generated/admin-api'

export function quotaStatus(state: TokenPlanQuotaState) {
  return { state, scope: 'text_inference' }
}

export function minimaxQuotaKey(
  id: string,
  state: TokenPlanQuotaState | null,
  interval: number | null,
  weekly: number | null,
  extraModels: TokenPlanKeyUsage['model_remains'] = [],
): TokenPlanKeyUsage {
  const window = (remaining: number | null): TokenPlanWindowUsage | null =>
    remaining === null ? null : { remaining_percent: remaining }
  return {
    key_id: id,
    key_label: id,
    ok: true,
    quota_status: state === null ? null : quotaStatus(state),
    model_remains: [
      {
        model_name: 'General',
        interval: window(interval),
        weekly: window(weekly),
      },
      ...extraModels,
    ],
  }
}

export function usage(
  provider: TokenPlanUsageResponse['provider'],
  keys: TokenPlanKeyUsage[],
): TokenPlanUsageResponse {
  return { provider, provider_region: null, keys }
}

export function subscriptionWindow(
  seconds: number,
  remaining: number | null,
): SubscriptionWindowUsage {
  return {
    source_window: seconds < 86_400 ? 'primary' : 'secondary',
    window_seconds: seconds,
    remaining_percent: remaining,
    used_percent: remaining === null ? null : 100 - remaining,
    reset_at: new Date(Date.now() + 60_000).toISOString(),
    reset_after_seconds: null,
    availability: remaining === null ? 'unknown' : 'known',
  }
}
