import type {
  CacheAlertSettingsRequest,
  CacheAlertSettingsResponse,
  CacheAlertSmtpSettingsResponse,
  CacheAlertSmtpTls,
} from '../../generated/admin-api'

// Backend defaults (`src/worker_admin/types/cache_alert.rs`); the admin API
// accepts partial payloads and fills the missing fields with these values.
export const CACHE_ALERT_DEFAULTS = {
  enabled: false,
  window_minutes: 30,
  min_turns: 5,
  threshold: 0.2,
  cooldown_minutes: 60,
} as const

// The SMTP channel's own serde defaults: off, implicit TLS on the default
// port, and no host, sender, recipient or credential.
export const CACHE_ALERT_SMTP_DEFAULTS = {
  enabled: false,
  host: '',
  port: 465,
  tls: 'implicit_tls',
  from: '',
  username: '',
} as const

export type CacheAlertSecretMode = 'keep' | 'replace'

export type CacheAlertSecretPatch = {
  mode: CacheAlertSecretMode
  value?: string
}

/**
 * The editable SMTP channel plus the three facts the server owns.
 *
 * The password is write-only like the DingTalk secret: the server reports
 * whether one is stored and never echoes it, so the form keeps a draft and the
 * presence flag apart. The stored username matters because the server keeps a
 * stored password only while the username stays the same, and the stored
 * configuration matters because a saved one is always sent back, so an operator
 * can switch the channel off without losing it.
 */
export type CacheAlertSmtpForm = {
  enabled: boolean
  host: string
  port: number
  tls: CacheAlertSmtpTls
  from: string
  to: string[]
  username: string
  has_password: boolean
  stored_username: string
  configured: boolean
  password: CacheAlertSecretPatch
}

/** The leaves the operator edits; the rest of the SMTP form is server-owned. */
export type CacheAlertSmtpLeaf =
  'enabled' | 'host' | 'port' | 'tls' | 'from' | 'to' | 'username'

export type CacheAlertForm = {
  enabled: boolean
  window_minutes: number
  min_turns: number
  threshold: number
  cooldown_minutes: number
  dingtalk_webhook_url: string
  // Server-computed presence of the stored secret; never inferred from the
  // local input action.
  has_dingtalk_secret: boolean
  secret: CacheAlertSecretPatch
  smtp: CacheAlertSmtpForm
}

/** Whether the stored SMTP configuration holds anything at all. */
function smtpConfigured(view: CacheAlertSmtpSettingsResponse): boolean {
  return (
    view.enabled === true ||
    view.has_smtp_password === true ||
    (view.host ?? '').trim() !== '' ||
    (view.from ?? '').trim() !== '' ||
    (view.username ?? '').trim() !== '' ||
    (view.to ?? []).some((recipient) => recipient.trim() !== '')
  )
}

/**
 * The recipient list of the form.
 *
 * The list always keeps one entry so the tab can show an input; an empty entry
 * is a placeholder the server drops, exactly as it drops an empty recipient.
 */
function smtpRecipients(view: CacheAlertSmtpSettingsResponse | null): string[] {
  const recipients = (view?.to ?? []).map((recipient) => recipient.trim())
  return recipients.length > 0 ? recipients : ['']
}

/** A stored response projected onto the SMTP form, with the password on keep. */
export function cacheAlertSmtpToForm(
  view: CacheAlertSmtpSettingsResponse | null | undefined,
): CacheAlertSmtpForm {
  if (!view) {
    return {
      enabled: CACHE_ALERT_SMTP_DEFAULTS.enabled,
      host: CACHE_ALERT_SMTP_DEFAULTS.host,
      port: CACHE_ALERT_SMTP_DEFAULTS.port,
      tls: CACHE_ALERT_SMTP_DEFAULTS.tls,
      from: CACHE_ALERT_SMTP_DEFAULTS.from,
      to: [''],
      username: CACHE_ALERT_SMTP_DEFAULTS.username,
      has_password: false,
      stored_username: '',
      configured: false,
      password: { mode: 'keep' },
    }
  }
  const username = view.username ?? ''
  return {
    enabled: view.enabled ?? CACHE_ALERT_SMTP_DEFAULTS.enabled,
    host: view.host ?? '',
    port: view.port ?? CACHE_ALERT_SMTP_DEFAULTS.port,
    tls: view.tls ?? CACHE_ALERT_SMTP_DEFAULTS.tls,
    from: view.from ?? '',
    to: smtpRecipients(view),
    username,
    has_password: view.has_smtp_password ?? false,
    stored_username: username,
    configured: smtpConfigured(view),
    password: { mode: 'keep' },
  }
}

export function cacheAlertToForm(
  api: CacheAlertSettingsResponse,
): CacheAlertForm {
  return {
    enabled: api.enabled ?? CACHE_ALERT_DEFAULTS.enabled,
    window_minutes: api.window_minutes ?? CACHE_ALERT_DEFAULTS.window_minutes,
    min_turns: api.min_turns ?? CACHE_ALERT_DEFAULTS.min_turns,
    threshold: api.threshold ?? CACHE_ALERT_DEFAULTS.threshold,
    cooldown_minutes:
      api.cooldown_minutes ?? CACHE_ALERT_DEFAULTS.cooldown_minutes,
    dingtalk_webhook_url: api.dingtalk_webhook_url ?? '',
    has_dingtalk_secret: api.has_dingtalk_secret ?? false,
    // The admin API never echoes the stored secret, so the form can only
    // start from "keep". `clear` does not exist server-side.
    secret: { mode: 'keep' },
    smtp: cacheAlertSmtpToForm(api.smtp),
  }
}

/**
 * The recipients as the API takes them: trimmed, without the empty entries.
 * The server normalizes the same way.
 */
export function normalizeCacheAlertRecipients(
  recipients: readonly string[],
): string[] {
  return recipients
    .map((recipient) => recipient.trim())
    .filter((recipient) => recipient !== '')
}

/**
 * Whether the SMTP channel takes part in this write.
 *
 * The admin API keeps the stored SMTP configuration when a request omits the
 * object, exactly as it does for a client that never knew the channel. So the
 * object is sent when the channel is enabled, when the server already holds a
 * configuration, or when the operator filled in any part of one; a
 * DingTalk-only save leaves it out and keeps the stored channel untouched.
 */
export function cacheAlertSmtpInPlay(smtp: CacheAlertSmtpForm): boolean {
  return (
    smtp.enabled ||
    smtp.configured ||
    smtp.host.trim() !== '' ||
    smtp.from.trim() !== '' ||
    smtp.username.trim() !== '' ||
    smtp.to.some((recipient) => recipient.trim() !== '')
  )
}

export function cacheAlertFormToRequest(
  form: CacheAlertForm,
): CacheAlertSettingsRequest {
  // The derived `has_dingtalk_secret` is response-only and is deliberately
  // never sent back.
  const request: CacheAlertSettingsRequest = {
    enabled: form.enabled,
    window_minutes: form.window_minutes,
    min_turns: form.min_turns,
    threshold: form.threshold,
    cooldown_minutes: form.cooldown_minutes,
    dingtalk_webhook_url: form.dingtalk_webhook_url.trim(),
    // A blank value keeps the stored secret; only `keep` / `replace` exist.
    dingtalk_secret:
      form.secret.mode === 'replace' ? (form.secret.value ?? '').trim() : '',
  }
  if (cacheAlertSmtpInPlay(form.smtp)) {
    request.smtp = {
      enabled: form.smtp.enabled,
      host: form.smtp.host.trim(),
      port: form.smtp.port,
      tls: form.smtp.tls,
      from: form.smtp.from.trim(),
      to: normalizeCacheAlertRecipients(form.smtp.to),
      username: form.smtp.username.trim(),
      // A blank value keeps the stored password; the server decides whether it
      // may, which depends on the username staying the same.
      password:
        form.smtp.password.mode === 'replace'
          ? (form.smtp.password.value ?? '').trim()
          : '',
    }
  }
  return request
}
