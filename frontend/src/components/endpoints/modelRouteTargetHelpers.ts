import type { TableColumn } from '@nuxt/ui'
import type { Ref } from 'vue'
import type { NativeApi } from '@/generated/admin-api'
import type { ModelRouteForm, ModelRouteTargetForm } from '@/models'
import { effectiveTargetProtocol } from '@/models/endpoints/service-tier'

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
  if (windows.length === 0) return ''
  const first = windows[0]
  if (!first) return ''
  if (windows.length === 1) return formatTargetWindow(first, t)
  return `${formatTargetWindow(first, t)} ${t('scheduleMoreWindows', { count: windows.length })}`
}

export function targetScheduleTooltip(
  target: ModelRouteTargetForm | null | undefined,
  t: TranslateFn,
): string {
  const windows = sortedTargetWindows(target)
  if (windows.length === 0) return ''
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
    hasTargetCompactMode(target) ||
    hasTargetServiceTier(target) ||
    hasTargetEmbeddingDimensions(target)
  )
}

// Issue #637: a configured free-form tier is non-default, so it must light up
// the target settings gear like the other overrides.
export function hasTargetServiceTier(
  target: ModelRouteTargetForm | null | undefined,
): boolean {
  return (target?.service_tier ?? '').trim() !== ''
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

// Issue #1032: a configured dimension override is non-default, so it lights up
// the target settings gear like the other overrides.
export function hasTargetEmbeddingDimensions(
  target: ModelRouteTargetForm | null | undefined,
): boolean {
  return (target?.embedding_dimensions_override ?? null) !== null
}

// Issue #1032: a malformed dimension entry blocks save instead of being
// silently turned into inherit.
export function isTargetEmbeddingDimensionsValid(
  target: ModelRouteTargetForm | null | undefined,
): boolean {
  return target?.embedding_dimensions_invalid !== true
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
    (target) =>
      isTargetProxyValid(target) &&
      isTargetScheduleValid(target) &&
      isTargetEmbeddingDimensionsValid(target),
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

// Issue #1032: the effective protocol of one target, used to scope that row's
// model suggestions. An explicit target protocol wins; `auto` inherits the
// endpoint protocol. When both are `auto` the runtime resolves the operation
// per request, so the suggestion listing stays unscoped (`undefined`) and the
// endpoint's own union is preserved.
export function suggestProtocolForTarget(
  target:
    Pick<ModelRouteTargetForm, 'native_api' | 'endpoint_id'> | null | undefined,
  endpointProtocolFor: (endpointId: string) => NativeApi,
): NativeApi | undefined {
  if (!target) return undefined
  const protocol = effectiveTargetProtocol(
    target.native_api,
    endpointProtocolFor(target.endpoint_id),
  )
  return protocol === 'auto' ? undefined : protocol
}
