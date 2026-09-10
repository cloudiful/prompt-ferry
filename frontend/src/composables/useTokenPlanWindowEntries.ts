import { onBeforeUnmount, reactive, ref, watch, type Ref } from 'vue'
import type {
  CommandCodeWindowUsage,
  GlmWindowUsage,
  OpencodeGoWindowUsage,
  TokenPlanKeyUsage,
  TokenPlanWindowUsage,
} from '@/generated/admin-api'

// Window entries shared by the token-plan dialog (issues #184 P4, #193 P2,
// #203 P2, #230 P3). MiniMax windows reuse percent/countdown rendering
// directly; CommandCode USD windows and OpencodeGo percent windows are
// adapted onto the same TokenPlanWindowUsage shape; GLM (Zhipu Coding
// Plan) token/credit windows follow the same adapt-then-render pattern
// as OpencodeGo (the API reports the used share as `percentage`, so we
// fold it into the shared `remaining_percent` rendering); OpenRouter
// spend is a static balance/spend display with no countdown window.
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
  subline: string
}

export type OpencodeGoEntry = {
  adapted: TokenPlanWindowUsage
  labelKey: string
  raw: OpencodeGoWindowUsage
}

export type GlmEntry = {
  adapted: TokenPlanWindowUsage
  labelKey: string
  raw: GlmWindowUsage
}

// Unified progress-window row: opencodeGo + GLM share the same 3-column
// rendering template. `subline` is the optional `used / total` caption
// shown below the row (null for opencodeGo, `${current} / ${limit}` for
// GLM). Exposed at module scope so dialog callers can type the v-for
// iterator without re-declaring the shape.
export type ProgressWindowEntry = {
  adapted: TokenPlanWindowUsage
  labelKey: string
  subline: string | null
}

export type OpenRouterSpendEntry = {
  labelKey: string
  value: number
}

// Pure adapt helpers exposed at module scope so the badge composable
// (which only needs remaining-percent math, no t/nowMs) can reuse the
// same provider-specific folding logic without instantiating the full
// dialog composable.
export function remainingPercent(window: TokenPlanWindowUsage): number {
  const raw = window.remaining_percent
  if (raw == null || !Number.isFinite(raw)) return 0
  return Math.max(0, Math.min(100, raw))
}

export function usedPercent(window: TokenPlanWindowUsage): number {
  return 100 - remainingPercent(window)
}

// CommandCode USD windows reuse MiniMax percent/countdown rendering by
// adapting reset_at onto the TokenPlanWindowUsage end_at shape.
export function ccAsWindow(
  window: CommandCodeWindowUsage | null | undefined,
): TokenPlanWindowUsage | null {
  if (!window) return null
  return {
    end_at: window.reset_at,
    remaining_percent: window.remaining_percent,
  }
}

// OpencodeGo windows carry a `percent` (used share) plus a `resets_at`
// anchor. We adapt them onto the shared progress/countdown rendering by
// folding the used percent into a remaining percent (100 - used) and
// reusing `resets_at` as the end_at reset anchor.
export function opencodeGoAsWindow(
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

// GLM (Zhipu Coding Plan) windows report the *used* share as
// `percentage` and the reset anchor as `next_reset_at`. We adapt them
// onto the shared progress/countdown rendering by folding the used
// percent into a remaining percent (100 - used) and reusing
// `next_reset_at` as the end_at reset anchor — same adapt pattern as
// OpencodeGo so the dialog can share the same progress bar.
export function glmAsWindow(
  window: GlmWindowUsage | null | undefined,
): TokenPlanWindowUsage | null {
  if (!window) return null
  const raw = window.percentage
  const used = raw == null || !Number.isFinite(raw) ? null : raw
  return {
    end_at: window.next_reset_at,
    remaining_percent:
      used == null ? null : Math.max(0, Math.min(100, 100 - used)),
  }
}

export function progressColor(window: TokenPlanWindowUsage): string {
  const used = usedPercent(window)
  const hue = 120 - used * 1.2
  return `hsl(${hue} 80% 45%)`
}

export function useTokenPlanWindowEntries(t: TranslateFn, nowMs: Ref<number>) {
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

  function ccEntries(key: TokenPlanKeyUsage): CcEntry[] {
    const entries: CcEntry[] = []
    const byKey = [
      ['tokenPlanFiveHour', key.five_hour],
      ['tokenPlanWeeklyUsd', key.weekly],
    ] as const
    for (const [labelKey, window] of byKey) {
      const adapted = ccAsWindow(window)
      if (window && adapted) {
        entries.push({
          adapted,
          labelKey,
          raw: window,
          subline: `${window.used.toFixed(2)} / ${window.cap.toFixed(2)} USD`,
        })
      }
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

  function glmEntries(key: TokenPlanKeyUsage): GlmEntry[] {
    const entries: GlmEntry[] = []
    const byKey = [
      ['tokenPlanInterval', key.glm_five_hour],
      ['tokenPlanWeekly', key.glm_weekly],
    ] as const
    for (const [labelKey, window] of byKey) {
      const adapted = glmAsWindow(window)
      if (window && adapted) entries.push({ adapted, labelKey, raw: window })
    }
    return entries
  }

  function glmMinRemaining(key: TokenPlanKeyUsage): number | null {
    const adapted = [
      glmAsWindow(key.glm_five_hour),
      glmAsWindow(key.glm_weekly),
    ].filter((window): window is TokenPlanWindowUsage => window != null)
    if (adapted.length === 0) return null
    return Math.min(...adapted.map(remainingPercent))
  }

  // Unified progress-window entries: opencodeGo + GLM rendered on the
  // same 3-column row template. GLM adds a used/total subline because
  // its raw payload carries `current_value` + `limit`; OpencodeGo
  // payload has no used/total so its subline is null. Keeping both
  // providers on one entry list lets the dialog render them with a
  // single v-for instead of two parallel blocks.
  function progressWindowEntries(
    key: TokenPlanKeyUsage,
  ): ProgressWindowEntry[] {
    const entries: ProgressWindowEntry[] = []
    for (const raw of opencodeGoEntries(key)) {
      entries.push({
        adapted: raw.adapted,
        labelKey: raw.labelKey,
        subline: null,
      })
    }
    for (const raw of glmEntries(key)) {
      entries.push({
        adapted: raw.adapted,
        labelKey: raw.labelKey,
        subline: `${raw.raw.current_value.toFixed(2)} / ${raw.raw.limit.toFixed(2)}`,
      })
    }
    return entries
  }

  function progressWindowMinRemaining(key: TokenPlanKeyUsage): number | null {
    const values: number[] = []
    const oc = opencodeGoMinRemaining(key)
    if (oc !== null) values.push(oc)
    const glm = glmMinRemaining(key)
    if (glm !== null) values.push(glm)
    if (values.length === 0) return null
    return Math.min(...values)
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
    glmEntries,
    glmMinRemaining,
    progressWindowEntries,
    progressWindowMinRemaining,
    openrouterEntries,
    formatOpenRouterCredits,
  }
}

const KEY_ROTATION_MS = 4000

// #277 P1 single-card carousel: 4s autoplay over `ok` keys, hover pauses.
export function useTokenPlanKeyCarousel(
  keys: Ref<TokenPlanKeyUsage[]>,
  visible: Ref<boolean>,
) {
  const index = ref(0)
  let timer: ReturnType<typeof setInterval> | null = null

  function stop(): void {
    if (timer !== null) clearInterval(timer)
    timer = null
  }
  function start(): void {
    stop()
    const list = keys.value
    if (index.value >= list.length) index.value = 0
    if (!visible.value || list.length < 2 || !list.some((k) => k.ok)) return
    timer = setInterval(() => {
      for (let offset = 1; offset <= keys.value.length; offset += 1) {
        const candidate = (index.value + offset) % keys.value.length
        if (keys.value[candidate]?.ok) {
          index.value = candidate
          return
        }
      }
    }, KEY_ROTATION_MS)
  }
  function go(target: number): void {
    const total = keys.value.length
    if (total === 0) return
    index.value = (target + total) % total
    start()
  }
  function setPaused(value: boolean): void {
    if (value) stop()
    else start()
  }

  watch([() => visible.value, () => keys.value.length], start, {
    immediate: true,
  })
  onBeforeUnmount(stop)

  return reactive({ index, go, setPaused })
}
