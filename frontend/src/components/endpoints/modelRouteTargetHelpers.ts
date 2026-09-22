import type { TableColumn } from '@nuxt/ui'
import type { Ref } from 'vue'
import type { ModelRouteForm, ModelRouteTargetForm } from '@/models'

type ScheduleWindow = { start: string; end: string; days?: number[] }

// Issue #457: `end` may additionally be `24:00` (exclusive midnight);
// `start` stays within `00:00-23:59`.
const HHMM_RE = /^([01]\d|2[0-3]):[0-5]\d$/
const HHMM_END_RE = /^(([01]\d|2[0-3]):[0-5]\d|24:00)$/

export function hasTargetProxy(
  target: ModelRouteTargetForm | null | undefined,
): boolean {
  if (!target) return false
  return (
    (target.proxy_url_override ?? '').trim() !== '' ||
    (target.has_saved_proxy_url_override ?? false)
  )
}

export function targetProxySummary(
  target: ModelRouteTargetForm | null | undefined,
  t: TranslateFn,
): string {
  return hasTargetProxy(target) ? t('proxySet') : t('proxyInherit')
}

export function sortedTargetWindows(
  target: ModelRouteTargetForm | null | undefined,
): ScheduleWindow[] {
  const windows = Array.isArray(target?.active_windows)
    ? (target?.active_windows ?? [])
    : []
  return [...windows]
    .map((window) => ({
      start: (window?.start ?? '').trim(),
      end: (window?.end ?? '').trim(),
      days: Array.isArray(window?.days)
        ? [...(window.days as number[])]
        : undefined,
    }))
    .filter((window) => window.start !== '' && window.end !== '')
    .sort((a, b) =>
      a.start === b.start
        ? a.end.localeCompare(b.end)
        : a.start.localeCompare(b.start),
    )
}

export function hasTargetSchedule(
  target: ModelRouteTargetForm | null | undefined,
): boolean {
  return sortedTargetWindows(target).length > 0
}

export function formatTargetWindow(
  window: ScheduleWindow,
  t?: TranslateFn,
): string {
  const time = `${window.start}–${window.end}`
  const days = window.days
  if (!days || days.length === 0 || days.length >= 7) return time
  if (!t) return `${time} ${days.join(',')}`
  return `${time} ${days.map((d) => t('weekday' + d)).join(', ')}`
}

export function targetScheduleSummary(
  target: ModelRouteTargetForm | null | undefined,
  t: TranslateFn,
): string {
  const windows = sortedTargetWindows(target)
  if (windows.length === 0) return t('scheduleEmptyHint')
  const first = windows[0]
  if (!first) return t('scheduleEmptyHint')
  if (windows.length === 1) return formatTargetWindow(first, t)
  return `${formatTargetWindow(first, t)} ${t('scheduleMoreWindows', { count: windows.length })}`
}

export function targetScheduleTooltip(
  target: ModelRouteTargetForm | null | undefined,
  t: TranslateFn,
): string {
  const windows = sortedTargetWindows(target)
  if (windows.length === 0) return t('scheduleEmptyHint')
  return windows.map((w) => formatTargetWindow(w, t)).join(', ')
}

export function hasTargetSettings(
  target: ModelRouteTargetForm | null | undefined,
): boolean {
  if (!target) return false
  return (
    hasTargetProxy(target) ||
    hasTargetSchedule(target) ||
    (target.dev_system_normalize ?? false) ||
    (target.thinking_downgrade_enabled ?? false) ||
    hasTargetThinkingEffort(target) ||
    hasTargetCompactMode(target)
  )
}

export function hasTargetThinkingEffort(
  target: ModelRouteTargetForm | null | undefined,
): boolean {
  return (target?.thinking_effort_override ?? '').trim() !== ''
}

export function hasTargetCompactMode(
  target: ModelRouteTargetForm | null | undefined,
): boolean {
  const mode = (target?.compact_mode ?? '').trim()
  return mode !== '' && mode !== 'passthrough'
}

export function isTargetProxyValid(
  target: ModelRouteTargetForm | null | undefined,
): boolean {
  const trimmed = (target?.proxy_url_override ?? '').trim()
  if (!trimmed) return true
  const match = trimmed.match(/^(http|https|socks5h|socks5):\/\/(.*)$/i)
  if (match) return (match[2] ?? '').trim() !== ''
  return true
}

export function isTargetScheduleValid(
  target: ModelRouteTargetForm | null | undefined,
): boolean {
  return (target?.active_windows ?? []).every((window) => {
    const start = (window?.start ?? '').trim()
    const end = (window?.end ?? '').trim()
    if (!start || !end) return false
    if (!HHMM_RE.test(start) || !HHMM_END_RE.test(end)) return false
    return start !== end
  })
}

export function canSaveTargets(
  targets: Array<ModelRouteTargetForm | null | undefined>,
): boolean {
  return targets.every(
    (target) => isTargetProxyValid(target) && isTargetScheduleValid(target),
  )
}

export function createTargetColumns(
  t: TranslateFn,
): TableColumn<ModelRouteForm['targets'][number]>[] {
  return [
    { id: 'order' },
    { id: 'endpoint', header: t('endpoint') },
    { id: 'status', header: t('status') },
    { id: 'actions' },
  ]
}

export function createTargetMeta(dragOver: Ref<number | null>) {
  return {
    class: {
      tr: (row: { index: number }) =>
        row.index === dragOver.value ? 'bg-elevated' : '',
    },
  }
}
