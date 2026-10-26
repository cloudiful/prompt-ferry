/**
 * Pure model for the admin cache-alert settings tab.
 *
 * The defaults, the response projection and the request body already live in
 * `@/admin-mappers/forms/cache-alert`, which encodes the server's serde
 * defaults and the write-only secret rule. This module reuses them unchanged and
 * adds only what the tab itself needs: the setting key, the leaf lists, a
 * per-leaf merge for the accepted response, a secret-draft rule, the dirty
 * check, the cache-alert-scoped refresh blocker and the numeric parsers.
 *
 * Two rules are the point of the port. The presence badge is server-computed:
 * `has_dingtalk_secret` always comes from the response and is never inferred
 * from what the operator typed. The threshold is the settings family's only
 * float, so its parser accepts finite decimals in [0, 1] without quantizing to
 * the 0.05 step or mistaking `0` for an empty value.
 */

import type { CacheAlertSettingsResponse } from '@/generated/admin-api'
import {
  CACHE_ALERT_DEFAULTS,
  cacheAlertFormToRequest,
  cacheAlertToForm,
  type CacheAlertForm,
} from '@/admin-mappers/forms/cache-alert'
import type { PendingActions } from './async-action'
import { settingKey } from './settings-keys'

export {
  CACHE_ALERT_DEFAULTS,
  cacheAlertFormToRequest,
  cacheAlertToForm,
  type CacheAlertForm,
}

/** The one guarded resource this tab writes. */
export const CACHE_ALERT_KEY = settingKey('cacheAlert')

/** The editable leaves the response may overwrite, in a stable order. */
export const CACHE_ALERT_LEAVES: ReadonlyArray<
  Exclude<keyof CacheAlertForm, 'has_dingtalk_secret' | 'secret'>
> = [
  'enabled',
  'window_minutes',
  'min_turns',
  'threshold',
  'cooldown_minutes',
  'dingtalk_webhook_url',
]

/** The server's accepted ranges, matching the form controls and its 400s. */
export const CACHE_ALERT_BOUNDS = {
  window_minutes: { min: 5, max: 1440 },
  min_turns: { min: 2, max: 100 },
  threshold: { min: 0, max: 1 },
  cooldown_minutes: { min: 5, max: 1440 },
} as const

export type CacheAlertBounds = { min: number; max: number }

export function createCacheAlertForm(): CacheAlertForm {
  return cacheAlertToForm({
    ...CACHE_ALERT_DEFAULTS,
    dingtalk_secret: '',
    dingtalk_webhook_url: '',
    has_dingtalk_secret: false,
  })
}

/** A stored response projected onto the form, with the secret on keep. */
export function formFromCacheAlertResponse(
  response: CacheAlertSettingsResponse,
): CacheAlertForm {
  return cacheAlertToForm(response)
}

/**
 * The secret draft for the operator's text: a trimmed-empty entry keeps the
 * stored secret, anything else will replace it. There is no clear mode.
 */
export function setCacheAlertSecretDraft(
  form: CacheAlertForm,
  raw: string,
): CacheAlertForm {
  return {
    ...form,
    secret: { mode: raw.trim() === '' ? 'keep' : 'replace', value: raw },
  }
}

/**
 * Apply a saved response to the leaves the operator has not changed since the
 * submit. The presence flag always comes from the server, and the secret draft
 * is handled by `clearSavedCacheAlertSecret`.
 */
export function mergeCacheAlertResponse(
  current: CacheAlertForm,
  submitted: CacheAlertForm,
  response: CacheAlertSettingsResponse,
): CacheAlertForm {
  const projected = formFromCacheAlertResponse(response)
  const merged: CacheAlertForm = { ...current }
  for (const leaf of CACHE_ALERT_LEAVES) {
    if (Object.is(current[leaf], submitted[leaf])) {
      Object.assign(merged, { [leaf]: projected[leaf] })
    }
  }
  merged.has_dingtalk_secret = projected.has_dingtalk_secret
  return merged
}

/**
 * Reset the secret draft a successful save consumed, and only that one: a draft
 * that no longer matches the captured one is a newer edit and stays.
 */
export function clearSavedCacheAlertSecret(
  current: CacheAlertForm,
  submitted: CacheAlertForm,
): CacheAlertForm {
  if (
    current.secret.mode !== submitted.secret.mode ||
    current.secret.value !== submitted.secret.value
  ) {
    return current
  }
  return { ...current, secret: { mode: 'keep' } }
}

/** Any leaf edit or any secret draft differs from the accepted form. */
export function cacheAlertDirty(
  current: CacheAlertForm,
  baseline: CacheAlertForm,
): boolean {
  if (
    current.secret.mode !== baseline.secret.mode ||
    (current.secret.value ?? '') !== (baseline.secret.value ?? '')
  ) {
    return true
  }
  return CACHE_ALERT_LEAVES.some((leaf) => current[leaf] !== baseline[leaf])
}

/**
 * Whether a scoped refresh is safe: loaded, idle, clean and without an
 * unrecovered save failure.
 */
export function cacheAlertRefreshBlocked(input: {
  pendingActions: PendingActions
  dirty: boolean
  failed: boolean
  loading: boolean
  loaded: boolean
}): boolean {
  return (
    !input.loaded ||
    input.loading ||
    input.dirty ||
    input.failed ||
    Object.hasOwn(input.pendingActions, CACHE_ALERT_KEY)
  )
}

/**
 * Detect the capability-unavailable answer for the cache alert setting.
 *
 * The worker answers 501 with this code when the configured backend cannot keep
 * any admin setting (SQLite). The check reads the structured code, never a
 * message substring.
 */
export const CACHE_ALERT_CAPABILITY_CODE = 'capability_unavailable'

export function isCacheAlertCapabilityUnavailable(cause: unknown): boolean {
  if (!cause || typeof cause !== 'object') return false
  if (!('error' in cause)) return false
  const envelope = (cause as { error?: unknown }).error
  if (!envelope || typeof envelope !== 'object') return false
  return (envelope as { code?: unknown }).code === CACHE_ALERT_CAPABILITY_CODE
}

/**
 * Parse a number input's text for an integer field. `null` means "not a usable
 * value yet", so the caller keeps its local text and never emits `NaN`.
 */
export function parseCacheAlertInteger(
  raw: string,
  bounds: CacheAlertBounds,
): number | null {
  const trimmed = raw.trim()
  if (!/^-?\d+$/.test(trimmed)) return null
  const parsed = Number(trimmed)
  if (!Number.isSafeInteger(parsed)) return null
  if (parsed < bounds.min || parsed > bounds.max) return null
  return parsed
}

/**
 * Parse a number input's text for the threshold. A finite decimal inside
 * [0, 1] is accepted exactly, so `0` stays a real zero and `0.85` is not
 * quantized to the 0.05 step.
 */
export function parseCacheAlertNumber(
  raw: string,
  bounds: CacheAlertBounds,
): number | null {
  const trimmed = raw.trim()
  if (trimmed === '') return null
  const parsed = Number(trimmed)
  if (!Number.isFinite(parsed)) return null
  if (parsed < bounds.min || parsed > bounds.max) return null
  return parsed
}
