import type {
  SubscriptionWindowUsage,
  TokenPlanKeyUsage,
} from '@/generated/admin-api'

const FIVE_HOURS_SECONDS = 5 * 60 * 60
const DAY_SECONDS = 24 * 60 * 60
const WEEK_SECONDS = 7 * DAY_SECONDS

export type SubscriptionWindowBand = 'short' | 'long'

export function subscriptionQuotaWindows(
  key: TokenPlanKeyUsage,
): SubscriptionWindowUsage[] {
  return key.model_remains.flatMap((model) => model.windows ?? [])
}

export function subscriptionQuotaWindowCount(
  key: TokenPlanKeyUsage,
  legacyCount: number,
): number {
  return key.model_remains.reduce(
    (count, model) =>
      count + (model.windows == null ? 0 : model.windows.length),
    legacyCount,
  )
}

export function subscriptionQuotaMinimumRemainingPercent(
  key: TokenPlanKeyUsage,
  legacyValues: number[],
): number | null {
  const values = [
    ...legacyValues,
    ...subscriptionQuotaWindows(key).flatMap((window) => {
      const remaining = subscriptionWindowRemainingPercent(window)
      return remaining === null ? [] : [remaining]
    }),
  ]
  return values.length === 0 ? null : Math.min(...values)
}

export function subscriptionQuotaMinimumForBand(
  key: TokenPlanKeyUsage,
  band: SubscriptionWindowBand,
): number | null {
  const values = subscriptionQuotaWindows(key).flatMap((window) => {
    if (subscriptionWindowBand(window) !== band) return []
    const remaining = subscriptionWindowRemainingPercent(window)
    return remaining === null ? [] : [remaining]
  })
  return values.length === 0 ? null : Math.min(...values)
}

export function subscriptionWindowRemainingPercent(
  window: SubscriptionWindowUsage,
): number | null {
  if (window.availability !== 'known') return null
  const remaining = window.remaining_percent
  if (
    typeof remaining !== 'number' ||
    !Number.isFinite(remaining) ||
    remaining < 0 ||
    remaining > 100
  ) {
    return null
  }
  return remaining
}

export function subscriptionWindowBand(
  window: SubscriptionWindowUsage,
): SubscriptionWindowBand | null {
  const duration = window.window_seconds
  if (
    typeof duration !== 'number' ||
    !Number.isFinite(duration) ||
    duration <= 0
  ) {
    return null
  }
  return duration < DAY_SECONDS ? 'short' : 'long'
}

function formatDuration(seconds: number): string {
  if (seconds % WEEK_SECONDS === 0) return `${seconds / WEEK_SECONDS}w`
  if (seconds % DAY_SECONDS === 0) return `${seconds / DAY_SECONDS}d`
  if (seconds % 3600 === 0) return `${seconds / 3600}h`
  if (seconds % 60 === 0) return `${seconds / 60}m`
  return `${seconds}s`
}

export function subscriptionWindowLabel(
  window: SubscriptionWindowUsage,
  t: TranslateFn,
): string {
  const duration = window.window_seconds
  if (duration === FIVE_HOURS_SECONDS) return t('tokenPlanInterval')
  if (duration === WEEK_SECONDS) return t('tokenPlanWeekly')
  if (
    typeof duration !== 'number' ||
    !Number.isFinite(duration) ||
    duration <= 0
  ) {
    return t('tokenPlanWindowDurationUnknown')
  }
  return t('tokenPlanDurationWindow', { duration: formatDuration(duration) })
}

export function subscriptionWindowResetMs(
  window: SubscriptionWindowUsage,
  nowMs: number,
  resetBaseMs?: number,
): number | null {
  if (typeof window.reset_at === 'string' && window.reset_at.length > 0) {
    const resetAt = Date.parse(window.reset_at)
    if (!Number.isNaN(resetAt)) return resetAt - nowMs
  }
  const countdown = window.reset_after_seconds
  if (
    typeof countdown !== 'number' ||
    !Number.isFinite(countdown) ||
    countdown < 0
  ) {
    return null
  }
  return typeof resetBaseMs === 'number' && Number.isFinite(resetBaseMs)
    ? resetBaseMs + countdown * 1000 - nowMs
    : countdown * 1000
}

export function formatSubscriptionQuotaDateTime(value: string): string {
  const timestamp = Date.parse(value)
  if (!Number.isFinite(timestamp)) return value
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(timestamp)
}

export function subscriptionWindowProgressColor(
  remainingPercent: number,
): string {
  const usedPercent = 100 - remainingPercent
  return `hsl(${120 - usedPercent * 1.2} 80% 45%)`
}
