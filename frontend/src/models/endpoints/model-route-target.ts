import type { NativeApi } from '@/generated/admin-api'
import type { ModelRouteTargetForm } from '@/models'
import { effectiveTargetProtocol } from '@/models/endpoints/service-tier'
import {
  isScheduleRowValid,
  scheduleWindowsForDisplay,
} from '@/models/endpoints/schedule-windows'

type ScheduleWindow = { start: string; end: string; days?: number[] }
type Translate = (
  key: string,
  values?: Record<string, string | number>,
) => string

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
  t: Translate,
): string {
  return hasTargetProxy(target) ? t('proxySet') : t('proxyInherit')
}

export function sortedTargetWindows(
  target: ModelRouteTargetForm | null | undefined,
): ScheduleWindow[] {
  const windows = Array.isArray(target?.active_windows)
    ? (target?.active_windows ?? [])
    : []
  return scheduleWindowsForDisplay(windows).sort((a, b) =>
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
  t?: Translate,
): string {
  const time = `${window.start}–${window.end}`
  const days = window.days
  if (!days || days.length === 0 || days.length >= 7) return time
  if (!t) return `${time} ${days.join(',')}`
  return `${time} ${days.map((d) => t('weekday' + d)).join(', ')}`
}

export function targetScheduleSummary(
  target: ModelRouteTargetForm | null | undefined,
  t: Translate,
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
  t: Translate,
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

export function hasTargetEmbeddingDimensions(
  target: ModelRouteTargetForm | null | undefined,
): boolean {
  return (target?.embedding_dimensions_override ?? null) !== null
}

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
  const match = trimmed.match(/^(http|https|socks5h|socks5):\/\/(.+)$/i)
  return Boolean(match && match[2]?.trim())
}

export function isTargetScheduleValid(
  target: ModelRouteTargetForm | null | undefined,
): boolean {
  return (target?.active_windows ?? []).every((window) =>
    isScheduleRowValid(window),
  )
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
