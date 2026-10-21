import { computed, ref, watch } from 'vue'
import {
  DISPLAY_TIME_ZONE_STORAGE_KEY,
  readStorage,
  writeStorage,
} from '@/storage'

/**
 * The display time zone is a browser-local preference, like the locale and
 * the theme: `auto` follows the zone the browser reports, and a manual IANA
 * name overrides it. It never changes what the server stores, and it never
 * changes an instant — only the label an instant is shown with.
 */
export const AUTO_DISPLAY_TIME_ZONE = 'auto'

/**
 * The zones offered when the runtime cannot enumerate the IANA database.
 * `UTC` and `Asia/Shanghai` are always present, because a fallback list that
 * cannot express the current zone would leave the preference unusable.
 */
const FALLBACK_TIME_ZONES = [
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

/**
 * Whether the runtime accepts the zone name. An unknown name is rejected
 * instead of being stored and silently ignored later.
 */
export function isValidTimeZone(zone: string): boolean {
  if (!zone) return false
  try {
    new Intl.DateTimeFormat('en-US', { timeZone: zone })
    return true
  } catch {
    return false
  }
}

/**
 * The zone the browser reports. A browser that reports nothing usable falls
 * back to UTC, which is a real zone rather than a guess about the user.
 */
export function browserTimeZone(): string {
  try {
    const reported = Intl.DateTimeFormat().resolvedOptions().timeZone
    if (reported && isValidTimeZone(reported)) return reported
  } catch {
    // A runtime without Intl zone support falls through to UTC.
  }
  return 'UTC'
}

/**
 * The single rule that turns a persisted value into a display zone. A stored
 * value that is not a known IANA name falls back to `auto`, so a corrupted or
 * outdated preference never breaks the display.
 */
export function resolveDisplayTimeZone(stored: string | null): string {
  const value = (stored ?? '').trim()
  if (!value || value === AUTO_DISPLAY_TIME_ZONE) return AUTO_DISPLAY_TIME_ZONE
  return isValidTimeZone(value) ? value : AUTO_DISPLAY_TIME_ZONE
}

export const displayTimeZone = ref<string>(
  resolveDisplayTimeZone(readStorage(DISPLAY_TIME_ZONE_STORAGE_KEY)),
)

watch(displayTimeZone, (value) => {
  writeStorage(DISPLAY_TIME_ZONE_STORAGE_KEY, value)
})

/**
 * The zone the shared formatter renders with: the manual override, or the
 * browser zone while the preference is `auto`.
 */
export function effectiveTimeZone(): string {
  const selected = displayTimeZone.value
  if (selected !== AUTO_DISPLAY_TIME_ZONE && isValidTimeZone(selected)) {
    return selected
  }
  return browserTimeZone()
}

/**
 * The IANA zones the runtime knows. `Intl.supportedValuesOf` is preferred and
 * guarded, because a runtime without it still gets a usable list.
 */
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

/**
 * The selectable zones: the automatic choice, the current browser zone, UTC,
 * Asia/Shanghai, and every zone the runtime knows.
 */
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

export function useDisplayTimeZone() {
  return computed<string>({
    get: () => displayTimeZone.value,
    set: (value: string) => {
      displayTimeZone.value = resolveDisplayTimeZone(value)
    },
  })
}

export function setDisplayTimeZone(next: string): void {
  displayTimeZone.value = resolveDisplayTimeZone(next)
}
