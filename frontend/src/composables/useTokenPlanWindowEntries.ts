import { onBeforeUnmount, ref, watch, type Ref } from 'vue'
import type {
  CommandCodeWindowUsage,
  OpencodeGoWindowUsage,
  TokenPlanKeyUsage,
  TokenPlanWindowUsage,
} from '@/generated/admin-api'

// Window entries shared by the token-plan dialog (issues #184 P4, #193 P2,
// #203 P2). MiniMax windows reuse percent/countdown rendering directly;
// CommandCode USD windows and OpencodeGo percent windows are adapted onto
// the same TokenPlanWindowUsage shape; OpenRouter spend is a static
// balance/spend display with no countdown window.
export function useTokenPlanTicker(visible: Ref<boolean>): Ref<number> {
  // Live "now" anchor for the reset countdown; ticks only while open.
  const nowMs = ref<number>(Date.now())
  let ticker: ReturnType<typeof setInterval> | null = null
  function startTicker(): void {
    if (ticker !== null) return
    nowMs.value = Date.now()
    ticker = setInterval(() => {
      nowMs.value = Date.now()
    }, 1000)
  }
  function stopTicker(): void {
    if (ticker === null) return
    clearInterval(ticker)
    ticker = null
  }
  watch(
    () => visible.value,
    (open) => {
      if (open) startTicker()
      else stopTicker()
    },
    { immediate: true },
  )
  onBeforeUnmount(stopTicker)
  return nowMs
}

export type CcEntry = {
  adapted: TokenPlanWindowUsage
  labelKey: string
  raw: CommandCodeWindowUsage
}

export type OpencodeGoEntry = {
  adapted: TokenPlanWindowUsage
  labelKey: string
  raw: OpencodeGoWindowUsage
}

export type OpenRouterSpendEntry = {
  labelKey: string
  value: number
}

export function useTokenPlanWindowEntries(t: TranslateFn, nowMs: Ref<number>) {
  function remainingPercent(window: TokenPlanWindowUsage): number {
    const raw = window.remaining_percent
    if (raw == null || !Number.isFinite(raw)) return 0
    return Math.max(0, Math.min(100, raw))
  }

  function usedPercent(window: TokenPlanWindowUsage): number {
    return 100 - remainingPercent(window)
  }

  function keyWindows(key: TokenPlanKeyUsage): TokenPlanWindowUsage[] {
    return key.model_remains.flatMap((model) =>
      [model.interval, model.weekly].filter(
        (window): window is TokenPlanWindowUsage => window != null,
      ),
    )
  }

  function keyWindowCount(key: TokenPlanKeyUsage): number {
    return keyWindows(key).length
  }

  function minimumRemainingPercent(key: TokenPlanKeyUsage): number | null {
    const windows = keyWindows(key)
    if (windows.length === 0) return null
    return Math.min(...windows.map(remainingPercent))
  }

  function progressColor(window: TokenPlanWindowUsage): string {
    const used = usedPercent(window)
    const hue = 120 - used * 1.2
    return `hsl(${hue} 80% 45%)`
  }

  function endTimeMs(window: TokenPlanWindowUsage): number | null {
    const value = window.end_at
    if (typeof value !== 'string' || value.length === 0) return null
    const ts = Date.parse(value)
    return Number.isNaN(ts) ? null : ts
  }

  function remainingMs(window: TokenPlanWindowUsage): number | null {
    // Prefer end_at so the countdown tracks wall-clock time; fall back to
    // the snapshot value when no parseable end_at is available.
    const end = endTimeMs(window)
    if (end !== null) return end - nowMs.value
    const snapshot = window.remains_time_ms
    if (typeof snapshot === 'number' && Number.isFinite(snapshot)) {
      return snapshot
    }
    return null
  }

  function formatRemaining(window: TokenPlanWindowUsage): string {
    const ms = remainingMs(window)
    if (ms === null) return '-'
    if (ms <= 0) return t('tokenPlanExpired')
    const totalSeconds = Math.floor(ms / 1000)
    const hours = Math.floor(totalSeconds / 3600)
    const minutes = Math.floor((totalSeconds % 3600) / 60)
    if (hours > 0) {
      return t('tokenPlanResetExpiresHoursMinutes', { hours, minutes })
    }
    if (minutes > 0) {
      return t('tokenPlanResetExpiresMinutes', { minutes })
    }
    return t('tokenPlanResetExpiresSeconds', { seconds: totalSeconds })
  }

  // CommandCode USD windows reuse MiniMax percent/countdown rendering by
  // adapting reset_at onto the TokenPlanWindowUsage end_at shape.
  function ccAsWindow(
    window: CommandCodeWindowUsage | null | undefined,
  ): TokenPlanWindowUsage | null {
    if (!window) return null
    return {
      end_at: window.reset_at,
      remaining_percent: window.remaining_percent,
    }
  }

  function ccEntries(key: TokenPlanKeyUsage): CcEntry[] {
    const entries: CcEntry[] = []
    const five = ccAsWindow(key.five_hour)
    if (key.five_hour && five) {
      entries.push({
        adapted: five,
        labelKey: 'tokenPlanFiveHour',
        raw: key.five_hour,
      })
    }
    const weekly = ccAsWindow(key.weekly)
    if (key.weekly && weekly) {
      entries.push({
        adapted: weekly,
        labelKey: 'tokenPlanWeeklyUsd',
        raw: key.weekly,
      })
    }
    return entries
  }

  function ccMinRemaining(key: TokenPlanKeyUsage): number | null {
    const adapted = [ccAsWindow(key.five_hour), ccAsWindow(key.weekly)].filter(
      (window): window is TokenPlanWindowUsage => window != null,
    )
    if (adapted.length === 0) return null
    return Math.min(...adapted.map(remainingPercent))
  }

  // OpencodeGo windows carry a `percent` (used share) plus a `resets_at`
  // anchor. We adapt them onto the shared progress/countdown rendering by
  // folding the used percent into a remaining percent (100 - used) and
  // reusing `resets_at` as the end_at reset anchor.
  function opencodeGoAsWindow(
    window: OpencodeGoWindowUsage | null | undefined,
  ): TokenPlanWindowUsage | null {
    if (!window) return null
    const raw = window.percent
    const used = raw == null || !Number.isFinite(raw) ? null : raw
    return {
      end_at: window.resets_at,
      remaining_percent:
        used == null ? null : Math.max(0, Math.min(100, 100 - used)),
    }
  }

  function opencodeGoEntries(key: TokenPlanKeyUsage): OpencodeGoEntry[] {
    const entries: OpencodeGoEntry[] = []
    const byKey = [
      ['tokenPlanRolling', key.opencodego_rolling],
      ['tokenPlanWeekly', key.opencodego_weekly],
      ['tokenPlanMonthly', key.opencodego_monthly],
    ] as const
    for (const [labelKey, window] of byKey) {
      const adapted = opencodeGoAsWindow(window)
      if (window && adapted) entries.push({ adapted, labelKey, raw: window })
    }
    return entries
  }

  function opencodeGoMinRemaining(key: TokenPlanKeyUsage): number | null {
    const adapted = [
      opencodeGoAsWindow(key.opencodego_rolling),
      opencodeGoAsWindow(key.opencodego_weekly),
      opencodeGoAsWindow(key.opencodego_monthly),
    ].filter((window): window is TokenPlanWindowUsage => window != null)
    if (adapted.length === 0) return null
    return Math.min(...adapted.map(remainingPercent))
  }

  function formatOpenRouterCredits(value: number | null | undefined): string {
    if (value == null || !Number.isFinite(value)) return t('tokenPlanNoLimit')
    return value.toFixed(2)
  }

  // OpenRouter spend (`GET /api/v1/key` usage fields) is a static
  // balance/spend display with no countdown window. Missing spend degrades
  // to an empty list (never padded).
  function openrouterEntries(key: TokenPlanKeyUsage): OpenRouterSpendEntry[] {
    const spend = key.openrouter_spend
    if (!spend) return []
    return [
      { labelKey: 'tokenPlanSpendUsage', value: spend.usage },
      { labelKey: 'tokenPlanSpendDaily', value: spend.daily },
      { labelKey: 'tokenPlanSpendWeekly', value: spend.weekly },
      { labelKey: 'tokenPlanSpendMonthly', value: spend.monthly },
    ]
  }

  return {
    remainingPercent,
    usedPercent,
    keyWindows,
    keyWindowCount,
    minimumRemainingPercent,
    progressColor,
    formatRemaining,
    ccEntries,
    ccMinRemaining,
    opencodeGoEntries,
    opencodeGoMinRemaining,
    openrouterEntries,
    formatOpenRouterCredits,
  }
}
