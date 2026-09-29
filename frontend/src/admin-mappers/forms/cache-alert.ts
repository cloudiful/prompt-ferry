import type {
  CacheAlertSettings,
  CacheAlertSettingsResponse,
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

export type CacheAlertSecretMode = 'keep' | 'replace'

export type CacheAlertSecretPatch = {
  mode: CacheAlertSecretMode
  value?: string
}

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
  }
}

export function cacheAlertFormToRequest(
  form: CacheAlertForm,
): CacheAlertSettings {
  // `Required` turns a missing backend field into a compile error instead of a
  // silent default. The derived `has_dingtalk_secret` is response-only and is
  // deliberately never sent back.
  const request: Required<CacheAlertSettings> = {
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
  return request
}
