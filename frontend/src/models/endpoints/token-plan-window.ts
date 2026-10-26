import type { TokenPlanWindowUsage } from '@/generated/admin-api'

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

export function formatOpenRouterCredits(
  value: number | null | undefined,
  t: TranslateFn,
): string {
  if (value == null || !Number.isFinite(value)) return t('tokenPlanNoLimit')
  return value.toFixed(2)
}
