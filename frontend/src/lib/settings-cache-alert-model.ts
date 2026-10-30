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
 *
 * The SMTP channel follows the same two rules and adds a third: its password
 * is write-only too, so the form keeps the draft, the server-owned presence
 * flag and the stored username apart. The stored username decides whether a
 * blank password still means "keep the stored one" or "the server drops it",
 * which is exactly what the server does with the same pair.
 */

import type { CacheAlertSettingsResponse } from '@/generated/admin-api'
import {
  CACHE_ALERT_DEFAULTS,
  CACHE_ALERT_SMTP_DEFAULTS,
  cacheAlertFormToRequest,
  cacheAlertToForm,
  normalizeCacheAlertRecipients,
  type CacheAlertForm,
  type CacheAlertSmtpForm,
  type CacheAlertSmtpLeaf,
} from '@/admin-mappers/forms/cache-alert'
import type { PendingActions } from './async-action'
import { settingKey } from './settings-keys'

export {
  CACHE_ALERT_DEFAULTS,
  CACHE_ALERT_SMTP_DEFAULTS,
  cacheAlertFormToRequest,
  cacheAlertToForm,
  normalizeCacheAlertRecipients,
  type CacheAlertForm,
  type CacheAlertSmtpForm,
  type CacheAlertSmtpLeaf,
}

/** The one guarded resource this tab writes. */
export const CACHE_ALERT_KEY = settingKey('cacheAlert')

/** The editable leaves the response may overwrite, in a stable order. */
export const CACHE_ALERT_LEAVES: ReadonlyArray<
  Exclude<keyof CacheAlertForm, 'has_dingtalk_secret' | 'secret' | 'smtp'>
> = [
  'enabled',
  'window_minutes',
  'min_turns',
  'threshold',
  'cooldown_minutes',
  'dingtalk_webhook_url',
]

/** The editable leaves of the SMTP channel, in a stable order. */
export const CACHE_ALERT_SMTP_LEAVES: ReadonlyArray<CacheAlertSmtpLeaf> = [
  'enabled',
  'host',
  'port',
  'tls',
  'from',
  'to',
  'username',
]

/** The server's accepted ranges, matching the form controls and its 400s. */
export const CACHE_ALERT_BOUNDS = {
  window_minutes: { min: 5, max: 1440 },
  min_turns: { min: 2, max: 100 },
  threshold: { min: 0, max: 1 },
  cooldown_minutes: { min: 5, max: 1440 },
  smtp_port: { min: 1, max: 65535 },
} as const

export type CacheAlertBounds = { min: number; max: number }

/**
 * Two leaves are the same value when they carry the same content, not the
 * same identity: the recipient list is rebuilt on every edit, so an unchanged
 * list must still compare as unchanged.
 */
function sameCacheAlertLeaf(left: unknown, right: unknown): boolean {
  if (Array.isArray(left) && Array.isArray(right)) {
    const leftRecipients = normalizeCacheAlertRecipients(left)
    const rightRecipients = normalizeCacheAlertRecipients(right)
    return (
      leftRecipients.length === rightRecipients.length &&
      leftRecipients.every(
        (recipient, index) => recipient === rightRecipients[index],
      )
    )
  }
  return Object.is(left, right)
}

export function createCacheAlertForm(): CacheAlertForm {
  return cacheAlertToForm({
    ...CACHE_ALERT_DEFAULTS,
    dingtalk_secret: '',
    dingtalk_webhook_url: '',
    has_dingtalk_secret: false,
    smtp: null,
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
 * The SMTP password draft, with the same rule as the DingTalk secret: a blank
 * entry keeps the stored password, anything else replaces it.
 */
export function setCacheAlertSmtpPasswordDraft(
  form: CacheAlertForm,
  raw: string,
): CacheAlertForm {
  return {
    ...form,
    smtp: {
      ...form.smtp,
      password: {
        mode: raw.trim() === '' ? 'keep' : 'replace',
        value: raw,
      },
    },
  }
}

/**
 * Apply a saved response to the leaves the operator has not changed since the
 * submit. The presence flags always come from the server, and the secret
 * drafts are handled by `clearSavedCacheAlertSecret`.
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
  const smtp: CacheAlertSmtpForm = { ...current.smtp }
  for (const leaf of CACHE_ALERT_SMTP_LEAVES) {
    if (sameCacheAlertLeaf(current.smtp[leaf], submitted.smtp[leaf])) {
      Object.assign(smtp, { [leaf]: projected.smtp[leaf] })
    }
  }
  smtp.has_password = projected.smtp.has_password
  smtp.stored_username = projected.smtp.stored_username
  smtp.configured = projected.smtp.configured
  merged.smtp = smtp
  return merged
}

/**
 * Reset the secret drafts a successful save consumed, and only those: a draft
 * that no longer matches the captured one is a newer edit and stays.
 */
export function clearSavedCacheAlertSecret(
  current: CacheAlertForm,
  submitted: CacheAlertForm,
): CacheAlertForm {
  let merged = current
  if (
    current.secret.mode === submitted.secret.mode &&
    current.secret.value === submitted.secret.value
  ) {
    merged = { ...merged, secret: { mode: 'keep' } }
  }
  if (
    current.smtp.password.mode === submitted.smtp.password.mode &&
    current.smtp.password.value === submitted.smtp.password.value
  ) {
    merged = {
      ...merged,
      smtp: { ...merged.smtp, password: { mode: 'keep' } },
    }
  }
  return merged
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
  if (
    current.smtp.password.mode !== baseline.smtp.password.mode ||
    (current.smtp.password.value ?? '') !== (baseline.smtp.password.value ?? '')
  ) {
    return true
  }
  if (CACHE_ALERT_LEAVES.some((leaf) => current[leaf] !== baseline[leaf])) {
    return true
  }
  return CACHE_ALERT_SMTP_LEAVES.some(
    (leaf) => !sameCacheAlertLeaf(current.smtp[leaf], baseline.smtp[leaf]),
  )
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

/** The SMTP problems the tab reports, with the field each one belongs to. */
export type CacheAlertSmtpIssueCode =
  | 'host_required'
  | 'host_invalid'
  | 'from_required'
  | 'from_invalid'
  | 'recipient_required'
  | 'recipient_invalid'
  | 'credentials_paired'

export type CacheAlertSmtpIssue = {
  code: CacheAlertSmtpIssueCode
  field: 'host' | 'from' | 'to' | 'credentials'
  index?: number
}

/** Whitespace and control characters, which no host or address may carry. */
const UNPRINTABLE = /[\s\p{Cc}]/u

/**
 * A host name or address without a scheme, as the server accepts it.
 */
export function validCacheAlertSmtpHost(value: string): boolean {
  const trimmed = value.trim()
  if (trimmed === '' || trimmed.length > 253) return false
  if (
    trimmed.includes('://') ||
    trimmed.includes('/') ||
    trimmed.includes('@')
  ) {
    return false
  }
  return !UNPRINTABLE.test(trimmed)
}

/**
 * An email address, as the server accepts it: one `@`, a domain with a dot
 * and no empty label, and no whitespace anywhere.
 */
export function validCacheAlertEmail(value: string): boolean {
  const trimmed = value.trim()
  if (trimmed === '' || trimmed.length > 254) return false
  if (UNPRINTABLE.test(trimmed)) return false
  const separator = trimmed.lastIndexOf('@')
  if (separator <= 0) return false
  const local = trimmed.slice(0, separator)
  const domain = trimmed.slice(separator + 1)
  if (local.length === 0 || local.length > 64 || local.includes('@'))
    return false
  if (
    domain.length === 0 ||
    !domain.includes('.') ||
    domain.startsWith('.') ||
    domain.endsWith('.') ||
    domain.includes('..')
  ) {
    return false
  }
  return /^[A-Za-z0-9.-]+$/.test(domain)
}

/**
 * The effective SMTP password of the form.
 *
 * A typed password always wins. A blank draft keeps the stored one, and the
 * server keeps it only while the username stays the same, so a changed
 * username leaves the channel without a password.
 */
export function cacheAlertSmtpEffectivePassword(
  smtp: CacheAlertSmtpForm,
): string {
  const typed =
    smtp.password.mode === 'replace' ? (smtp.password.value ?? '').trim() : ''
  if (typed !== '') return typed
  if (!smtp.has_password) return ''
  return smtp.username.trim() === smtp.stored_username.trim() ? 'stored' : ''
}

/**
 * The SMTP problems the operator has to fix, with the same rules the server
 * applies: the host and the addresses are checked whenever they carry a value,
 * the required ones only when the channel is on, and the credentials have to
 * stay paired.
 */
export function cacheAlertSmtpIssues(
  form: CacheAlertForm,
): CacheAlertSmtpIssue[] {
  const issues: CacheAlertSmtpIssue[] = []
  const smtp = form.smtp

  const host = smtp.host.trim()
  if (host === '') {
    if (smtp.enabled) issues.push({ code: 'host_required', field: 'host' })
  } else if (!validCacheAlertSmtpHost(host)) {
    issues.push({ code: 'host_invalid', field: 'host' })
  }

  const from = smtp.from.trim()
  if (from === '') {
    if (smtp.enabled) issues.push({ code: 'from_required', field: 'from' })
  } else if (!validCacheAlertEmail(from)) {
    issues.push({ code: 'from_invalid', field: 'from' })
  }

  const recipients = smtp.to.map((recipient) => recipient.trim())
  if (!recipients.some((recipient) => recipient !== '')) {
    if (smtp.enabled) {
      issues.push({ code: 'recipient_required', field: 'to' })
    }
  } else {
    recipients.forEach((recipient, index) => {
      if (recipient === '' || validCacheAlertEmail(recipient)) return
      issues.push({ code: 'recipient_invalid', field: 'to', index })
    })
  }

  const username = smtp.username.trim()
  const hasPassword = cacheAlertSmtpEffectivePassword(smtp) !== ''
  if ((username !== '') !== hasPassword) {
    issues.push({ code: 'credentials_paired', field: 'credentials' })
  }

  return issues
}
