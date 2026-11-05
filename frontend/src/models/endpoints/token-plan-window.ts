import type {
  CanonicalQuotaWindowKind,
  TokenPlanWindowUsage,
} from '@/generated/admin-api'

export function currencySymbol(currency: string | null | undefined): string {
  switch ((currency ?? '').toUpperCase()) {
    case 'CNY':
      return '¥'
    case 'USD':
      return '$'
    case '':
      return ''
    default:
      return `${currency} `
  }
}

export function formatMoney(
  currency: string | null | undefined,
  value: number,
): string {
  return `${currencySymbol(currency)}${value.toFixed(2)}`
}

export function knownRemainingPercent(
  window: TokenPlanWindowUsage,
): number | null {
  const value = window.remaining_percent
  return typeof value === 'number' &&
    Number.isFinite(value) &&
    value >= 0 &&
    value <= 100
    ? value
    : null
}

export function remainingPercent(window: TokenPlanWindowUsage): number {
  return knownRemainingPercent(window) ?? 0
}

export function usedPercent(window: TokenPlanWindowUsage): number {
  return 100 - remainingPercent(window)
}

export function progressColor(window: TokenPlanWindowUsage): string {
  const used = usedPercent(window)
  const hue = 120 - used * 1.2
  return `hsl(${hue} 80% 45%)`
}

export function formatResetDuration(ms: number | null, t: TranslateFn): string {
  if (ms === null) return '-'
  if (ms <= 0) return t('tokenPlanExpired')
  const totalSeconds = Math.floor(ms / 1000)
  const days = Math.floor(totalSeconds / 86400)
  const hours = Math.floor((totalSeconds % 86400) / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  if (days > 0) {
    return t('tokenPlanResetExpiresDaysHoursMinutes', { days, hours, minutes })
  }
  if (hours > 0) {
    return t('tokenPlanResetExpiresHoursMinutes', { hours, minutes })
  }
  if (minutes > 0) {
    return t('tokenPlanResetExpiresMinutes', { minutes })
  }
  return t('tokenPlanResetExpiresSeconds', { seconds: totalSeconds })
}

function compactDurationUnit(seconds: number): string {
  const day = 24 * 60 * 60
  if (seconds % day === 0) return `${seconds / day}d`
  if (seconds % 3600 === 0) return `${seconds / 3600}h`
  if (seconds % 60 === 0) return `${seconds / 60}m`
  return `${seconds}s`
}

// Short duration label derived from an observed window length. A missing or
// non-positive duration never invents a slot.
export function compactWindowDuration(seconds: number | null): string | null {
  if (seconds === null || !Number.isFinite(seconds) || seconds <= 0) {
    return null
  }
  return compactDurationUnit(seconds)
}

// Compact label for one quota window. Known kinds supply their own slot only
// when no real duration was observed; custom windows keep their measured
// duration and stay unknown without one.
export function shortWindowLabel(
  kind: CanonicalQuotaWindowKind,
  durationSeconds: number | null,
): string | null {
  const duration = compactWindowDuration(durationSeconds)
  if (duration !== null) return duration
  switch (kind) {
    case 'five_hour':
      return '5h'
    case 'weekly':
      return '7d'
    case 'monthly':
      return '30d'
    default:
      return null
  }
}

// Compact countdown for the current-detail bar: duration units only, with the
// shared expired token when the reset already passed and no countdown when the
// anchor is missing.
export function formatCompactResetCountdown(
  ms: number | null,
  t: TranslateFn,
): string {
  if (ms === null || !Number.isFinite(ms)) return '-'
  if (ms <= 0) return t('tokenPlanExpired')
  const totalSeconds = Math.floor(ms / 1000)
  const days = Math.floor(totalSeconds / 86400)
  const hours = Math.floor((totalSeconds % 86400) / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  if (days > 0) return hours > 0 ? `${days}d ${hours}h` : `${days}d`
  if (hours > 0) return minutes > 0 ? `${hours}h ${minutes}m` : `${hours}h`
  if (minutes > 0) return `${minutes}m`
  return `${totalSeconds}s`
}

export function formatOpenRouterCredits(
  value: number | null | undefined,
  t: TranslateFn,
): string {
  if (value == null || !Number.isFinite(value)) return t('tokenPlanNoLimit')
  return value.toFixed(2)
}
