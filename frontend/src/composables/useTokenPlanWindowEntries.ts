import { onBeforeUnmount, ref, watch, type Ref } from 'vue'
import type {
  CommandCodeWindowUsage,
  GlmWindowUsage,
  OpencodeGoWindowUsage,
  TokenPlanKeyUsage,
  TokenPlanWindowUsage,
} from '@/generated/admin-api'
import {
  canonicalQuantityCaption,
  toCanonicalQuotaRows,
  type CanonicalQuotaRow,
} from './useCanonicalQuotaWindows'

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
// shown below the row (null for opencodeGo, `${current} / ${limit}` for GLM
// windows with a known quantity, null when the GLM quantity is unknown).
// Exposed at module scope so dialog callers can type the v-for
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

// Currency glyph for a DeepSeek balance entry (issue #287). Unknown codes
// fall back to the code plus a space so the amount is never ambiguous.
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

// Pure adapt helpers shared by the dialog and badges without instantiating
// the dialog's ticker.
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

// GLM (Zhipu Coding Plan) quantity subline: amounts render only for a
// positive finite limit with a finite current. Otherwise the quantity is
// unknown and no `0 / 0` may be manufactured — the window itself stays
// visible with its honestly reported percentage.
export function glmQuantitySubline(window: GlmWindowUsage): string | null {
  const { current_value, limit } = window
  if (
    typeof limit !== 'number' ||
    !Number.isFinite(limit) ||
    limit <= 0 ||
    typeof current_value !== 'number' ||
    !Number.isFinite(current_value)
  ) {
    return null
  }
  return `${current_value.toFixed(2)} / ${limit.toFixed(2)}`
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

// Reset-window countdown text, split out of the composable so the boundaries
// are unit-testable without mounting it. A window of a full day or more renders
// days, hours and minutes; everything shorter keeps the existing hour/minute,
// minute-only and second-only output, and a window in the past stays expired.
// `null` means the window reported no usable reset anchor at all.
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

function minimumKnownRemaining(windows: TokenPlanWindowUsage[]): number | null {
  const known = windows
    .map(knownRemainingPercent)
    .filter((value): value is number => value !== null)
  return known.length ? Math.min(...known) : null
}

export function useTokenPlanWindowEntries(t: TranslateFn, nowMs: Ref<number>) {
  // Canonical time-window kinds rendered as quota rows. Balance/unlimited
  // stay in their dedicated panels and never count as windows.
  function isQuotaRow(row: CanonicalQuotaRow): boolean {
    return (
      row.kind === 'five_hour' ||
      row.kind === 'rolling' ||
      row.kind === 'weekly' ||
      row.kind === 'monthly' ||
      row.kind === 'custom'
    )
  }

  function keyWindows(_key: TokenPlanKeyUsage): TokenPlanWindowUsage[] {
    // Retained for API compatibility only: header counts and minima now
    // come from the canonical adapter below, never raw model windows.
    return []
  }

  function keyWindowCount(key: TokenPlanKeyUsage): number {
    // Count all visible quota observations (known, unknown and expired all
    // render a row); an authoritative empty stays empty via the adapter.
    return toCanonicalQuotaRows(key, nowMs.value).filter(isQuotaRow).length
  }

  function minimumRemainingPercent(key: TokenPlanKeyUsage): number | null {
    // Overall pool minimum from default-scope rows only: detail rows stay
    // visible in their model group but never bind the pool.
    const values = toCanonicalQuotaRows(key, nowMs.value)
      .filter(
        (row) =>
          isQuotaRow(row) &&
          row.remainingPercent != null &&
          (row.sourceModel == null || row.isDefaultScope),
      )
      .map((row) => row.remainingPercent as number)
    return values.length === 0 ? null : Math.min(...values)
  }

  // Minimum for one model group, scoped to that model only (default and
  // detail rows of the same model both count here, unlike the pool min).
  function modelMinimumRemainingPercent(
    key: TokenPlanKeyUsage,
    modelName: string,
  ): number | null {
    const values = toCanonicalQuotaRows(key, nowMs.value)
      .filter(
        (row) =>
          isQuotaRow(row) &&
          row.remainingPercent != null &&
          row.sourceModel === modelName,
      )
      .map((row) => row.remainingPercent as number)
    return values.length === 0 ? null : Math.min(...values)
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
    return formatResetDuration(remainingMs(window), t)
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
    return minimumKnownRemaining(adapted)
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
    return minimumKnownRemaining(adapted)
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
    return minimumKnownRemaining(adapted)
  }

  // OpencodeGo and GLM share one row template; only GLM has a used/total
  // subline. Rows come from the canonical adapter (observations when the
  // backend reports them, otherwise the conservative legacy projection):
  // the dialog keeps its 3-column template while quantities hide their
  // ambiguity (legacy GLM amounts stay unknown, percents stay honest).
  function progressWindowEntries(
    key: TokenPlanKeyUsage,
  ): ProgressWindowEntry[] {
    const now = nowMs.value
    const entries: ProgressWindowEntry[] = []
    for (const row of toCanonicalQuotaRows(key, now)) {
      const labelKey = progressRowLabelKey(row)
      if (labelKey === null) continue
      entries.push({
        adapted: {
          end_at: row.resetAt,
          remaining_percent: row.remainingPercent,
        },
        labelKey,
        subline: canonicalQuantityCaption(row),
      })
    }
    return entries
  }

  // Label for an adapter row inside the shared progress template. Only the
  // opencodeGo/GLM slots render here (other providers keep their dedicated
  // dialog sections until P3 unifies them); anything else is skipped so the
  // frozen template never gains a duplicated or mislabeled row.
  function progressRowLabelKey(row: CanonicalQuotaRow): string | null {
    if (row.sourceId.startsWith('opencode_go:')) {
      if (row.kind === 'rolling') return 'tokenPlanRolling'
      if (row.kind === 'weekly' || row.kind === 'monthly') {
        return row.kind === 'weekly' ? 'tokenPlanWeekly' : 'tokenPlanMonthly'
      }
      return null
    }
    if (row.sourceId.startsWith('glm:')) {
      if (row.kind === 'five_hour') return 'tokenPlanInterval'
      if (row.kind === 'weekly') return 'tokenPlanWeekly'
      return null
    }
    return null
  }

  function progressWindowMinRemaining(key: TokenPlanKeyUsage): number | null {
    const values = progressWindowEntries(key)
      .map((entry) => knownRemainingPercent(entry.adapted))
      .filter((value): value is number => value !== null)
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
    modelMinimumRemainingPercent,
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
