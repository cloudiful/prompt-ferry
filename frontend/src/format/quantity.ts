import type { Locale } from '@/i18n'
import { formatNumber } from './datetime-core'

export function formatTokenQuantity(
  value: number | null | undefined,
  locale: Locale,
): string {
  if (value == null || !Number.isFinite(value)) return '-'
  const abs = Math.abs(value)
  if (abs < 1000) return formatNumber(Math.round(value), locale)

  let scaled = value
  let unit = ''
  if (abs >= 1_000_000_000_000) {
    scaled = value / 1_000_000_000_000
    unit = 'T'
  } else if (abs >= 1_000_000_000) {
    scaled = value / 1_000_000_000
    unit = 'B'
  } else if (abs >= 1_000_000) {
    scaled = value / 1_000_000
    unit = 'M'
  } else {
    scaled = value / 1_000
    unit = 'K'
  }
  const rounded = Math.round(scaled * 10) / 10
  const fixed = rounded.toFixed(1)
  const trimmed = fixed.endsWith('.0') ? fixed.slice(0, -2) : fixed
  const localized = formatNumber(Number(trimmed), locale, {
    minimumFractionDigits: 0,
    maximumFractionDigits: 1,
  })
  return `${localized}${unit}`
}
