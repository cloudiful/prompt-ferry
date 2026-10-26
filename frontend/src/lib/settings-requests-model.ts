/**
 * Pure model for the admin Requests settings tab.
 *
 * The tab edits four independent stored settings. Everything here is a value
 * transform — defaults, the sorted-key snapshot the autosave compares, integer
 * bounds, and the "apply only what the operator did not touch" response merges —
 * so the store stays about lifecycle and the request contract stays testable
 * without a component.
 *
 * The two retention forms are deliberately not merged with each other here: the
 * logging save writes a read-modify-write of the usage-retention record, so the
 * mirroring runs through `mirrorSharedRawRetention` and keeps the stored raw
 * value in step with whoever still holds the original value.
 */

import type {
  ModelRouteWhitelistResponse,
  RequestContentLoggingMode,
  RequestContentLoggingResponse,
  UsageRetentionSettings,
} from '@/generated/admin-api'
import type { MessageKey } from '@/i18n'
import type { StreamDeltaBatchingForm } from '@/models'
import type { PendingActions } from './async-action'
import type { SettingForm } from './settings-keys'
import { SETTING_FORM_ORDER, settingFormKey } from './settings-keys'

export type ModelRouteWhitelistForm = { enabled: boolean }

/** The three logging modes the backend accepts, in menu order. */
export const REQUEST_CONTENT_LOGGING_MODES: ReadonlyArray<{
  labelKey: MessageKey
  value: RequestContentLoggingMode
}> = [
  { labelKey: 'contentLoggingModeOff', value: 'off' },
  { labelKey: 'contentLoggingModeNormalizedOnly', value: 'normalized_only' },
  {
    labelKey: 'contentLoggingModeNormalizedAndRaw',
    value: 'normalized_and_raw',
  },
]

export const RAW_RETENTION_DAYS_RANGE = { min: 1, max: 30 } as const
export const APPROVAL_RETENTION_DAYS_RANGE = { min: 1 } as const
export const FLUSH_WINDOW_MS_RANGE = { min: 1, max: 1000 } as const
export const MAX_BUFFER_CHARS_RANGE = { min: 1, max: 8192 } as const
export const MAX_BUFFER_BYTES_RANGE = { min: 1, max: 65536 } as const

/** The editable integer bounds of the two retention controls. */
export const SETTING_FORM_RANGES = {
  rawRetentionDays: RAW_RETENTION_DAYS_RANGE,
  approvalRetentionDays: APPROVAL_RETENTION_DAYS_RANGE,
} as const

export function createRequestContentLoggingForm(): RequestContentLoggingResponse {
  return { mode: 'off', raw_retention_days: RAW_RETENTION_DAYS_RANGE.min }
}

export function createUsageRetentionForm(): UsageRetentionSettings {
  return { approval_retention_days: APPROVAL_RETENTION_DAYS_RANGE.min }
}

export function createStreamDeltaBatchingForm(): StreamDeltaBatchingForm {
  return {
    enabled: true,
    flush_window_ms: 40,
    max_buffer_chars: MAX_BUFFER_CHARS_RANGE.max,
    max_buffer_bytes: MAX_BUFFER_BYTES_RANGE.max,
    flush_on_line_break: true,
    flush_on_sentence_end: true,
  }
}

export function createModelRouteWhitelistForm(): ModelRouteWhitelistForm {
  return { enabled: false }
}

/**
 * The autosave identity of a form: a JSON serialization with sorted keys so two
 * field orders never look like an edit.
 */
export function settingsSnapshot(value: unknown): string {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return JSON.stringify(value) ?? ''
  }
  return JSON.stringify(
    Object.fromEntries(
      Object.entries(value).sort(([left], [right]) =>
        left.localeCompare(right),
      ),
    ),
  )
}

/**
 * Clamp an edited integer to its control bounds.
 *
 * A loaded value outside the bounds is not repaired here: reading a setting is
 * not an edit, so the tab keeps what the server returned until the operator
 * actually changes it.
 */
export function clampSettingInteger(
  value: number,
  range: { min: number; max?: number },
): number {
  const rounded = Math.trunc(value)
  if (!Number.isFinite(rounded)) return range.min
  const lower = Math.max(rounded, range.min)
  return range.max === undefined ? lower : Math.min(lower, range.max)
}

/** Parse a number input's text; `null` means "not a number yet, keep the value". */
export function parseSettingInteger(
  raw: string,
  range: { min: number; max?: number },
): number | null {
  const trimmed = raw.trim()
  if (trimmed === '') return null
  if (!/^-?\d+$/.test(trimmed)) return null
  const parsed = Number(trimmed)
  if (!Number.isSafeInteger(parsed)) return null
  return clampSettingInteger(parsed, range)
}

/**
 * Apply a logging response to the fields the operator has not changed since the
 * submit. A field that already differs from the submitted snapshot is a newer
 * edit and is never overwritten, including the raw-retention number the two
 * forms share.
 */
export function mergeRequestContentLoggingResponse(
  current: RequestContentLoggingResponse,
  submitted: RequestContentLoggingResponse,
  response: RequestContentLoggingResponse,
): RequestContentLoggingResponse {
  return {
    mode: Object.is(current.mode, submitted.mode)
      ? response.mode
      : current.mode,
    raw_retention_days: Object.is(
      current.raw_retention_days,
      submitted.raw_retention_days,
    )
      ? response.raw_retention_days
      : current.raw_retention_days,
  }
}

/** Apply a usage-retention response to the fields the operator has not changed. */
export function mergeUsageRetentionResponse(
  current: UsageRetentionSettings,
  submitted: UsageRetentionSettings,
  response: UsageRetentionSettings,
): UsageRetentionSettings {
  const merged: UsageRetentionSettings = { ...current }
  for (const key of Object.keys(submitted) as Array<
    keyof UsageRetentionSettings
  >) {
    if (Object.is(current[key], submitted[key])) {
      Object.assign(merged, { [key]: response[key] })
    }
  }
  return merged
}

/**
 * Mirror the stored raw-retention days into a form that still holds the value
 * that was submitted with the write.
 *
 * The fallback keeps the shared value when the response omits the field, so a
 * partial retention response can never blank the number both forms display.
 */
export function mirrorSharedRawRetention<
  T extends { raw_retention_days?: number },
>(
  current: T,
  submittedRawRetention: number | undefined,
  responseRawRetention: number | undefined,
): T {
  if (!Object.is(current.raw_retention_days, submittedRawRetention))
    return current
  return {
    ...current,
    raw_retention_days: responseRawRetention ?? submittedRawRetention,
  }
}

/** Apply a stream-batching response to the fields the operator has not changed. */
export function mergeStreamDeltaBatchingResponse(
  current: StreamDeltaBatchingForm,
  submitted: StreamDeltaBatchingForm,
  confirmed: StreamDeltaBatchingForm,
): StreamDeltaBatchingForm {
  const merged: StreamDeltaBatchingForm = { ...current }
  for (const key of Object.keys(submitted) as Array<
    keyof StreamDeltaBatchingForm
  >) {
    if (Object.is(current[key], submitted[key])) {
      Object.assign(merged, { [key]: confirmed[key] })
    }
  }
  return merged
}

/** Apply a whitelist response only when the switch still holds the submitted value. */
export function mergeModelRouteWhitelistResponse(
  current: ModelRouteWhitelistForm,
  submitted: ModelRouteWhitelistForm,
  response: ModelRouteWhitelistResponse,
): ModelRouteWhitelistForm {
  return Object.is(current.enabled, submitted.enabled)
    ? { enabled: response.enabled }
    : current
}

/**
 * Whether a scoped refresh is safe: no requests write is in flight, and no form
 * is dirty, debouncing, queued or failed. A refresh replaces the loaded values,
 * so it must not discard a draft the operator has not saved yet.
 */
export function settingsRefreshBlocked(input: {
  pendingActions: PendingActions
  dirty: Readonly<Record<SettingForm, boolean>>
  failed: Readonly<Record<SettingForm, boolean>>
}): boolean {
  for (const form of SETTING_FORM_ORDER) {
    if (input.dirty[form] || input.failed[form]) return true
    if (Object.hasOwn(input.pendingActions, settingFormKey(form))) return true
  }
  return false
}
