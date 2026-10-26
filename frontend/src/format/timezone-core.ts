export const AUTO_DISPLAY_TIME_ZONE = 'auto'

export const FALLBACK_TIME_ZONES = [
  'UTC',
  'Asia/Shanghai',
  'Asia/Hong_Kong',
  'Asia/Tokyo',
  'Asia/Singapore',
  'Australia/Sydney',
  'Europe/Berlin',
  'Europe/London',
  'America/Los_Angeles',
  'America/New_York',
]

export type DisplayTimeZoneOption = { label: string; value: string }

export function isValidTimeZone(zone: string): boolean {
  if (!zone) return false
  try {
    new Intl.DateTimeFormat('en-US', { timeZone: zone })
    return true
  } catch {
    return false
  }
}

export function browserTimeZone(): string {
  try {
    const reported = Intl.DateTimeFormat().resolvedOptions().timeZone
    if (reported && isValidTimeZone(reported)) return reported
  } catch {
    // A runtime without Intl zone support falls through to UTC.
  }
  return 'UTC'
}

export function resolveDisplayTimeZone(stored: string | null): string {
  const value = (stored ?? '').trim()
  if (!value || value === AUTO_DISPLAY_TIME_ZONE) return AUTO_DISPLAY_TIME_ZONE
  return isValidTimeZone(value) ? value : AUTO_DISPLAY_TIME_ZONE
}

export function effectiveTimeZoneFor(selected: string): string {
  if (selected !== AUTO_DISPLAY_TIME_ZONE && isValidTimeZone(selected)) {
    return selected
  }
  return browserTimeZone()
}

export function supportedTimeZones(): string[] {
  try {
    const intl = Intl as typeof Intl & {
      supportedValuesOf?: (key: string) => string[]
    }
    const values = intl.supportedValuesOf?.('timeZone')
    if (values && values.length > 0) return values
  } catch {
    // A runtime without the enumeration keeps the curated list.
  }
  return FALLBACK_TIME_ZONES
}

export function displayTimeZoneOptions(
  autoLabel: string,
): DisplayTimeZoneOption[] {
  const browser = browserTimeZone()
  const options: DisplayTimeZoneOption[] = [
    { label: autoLabel, value: AUTO_DISPLAY_TIME_ZONE },
    { label: browser, value: browser },
  ]
  for (const zone of ['UTC', 'Asia/Shanghai', ...supportedTimeZones()]) {
    if (options.some((option) => option.value === zone)) continue
    options.push({ label: zone, value: zone })
  }
  return options
}
