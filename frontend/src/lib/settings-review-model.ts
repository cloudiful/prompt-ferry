/**
 * Pure model for the admin Review settings tab: the LLM review settings.
 *
 * The form is flattened to leaves — including the webhook's four fields and the
 * extra-header text — because the server echoes every stored value back and the
 * merge after a save must decide, leaf by leaf, whether the operator has edited
 * again since the submit. The two secrets are ordinary string leaves: an
 * untouched save sends the stored value back unchanged, and an emptied field
 * sends an empty string, which clears it. There is no keep/replace toggle.
 *
 * Defaults come from the server (`LlmReviewSettings::default`), so a
 * pre-load form never invents a value the server would not send.
 */

import type {
  LlmReviewSettings,
  ReviewFailurePolicy,
} from '@/generated/admin-api'
import {
  ensureLlmReviewDefaults,
  webhookHeadersFromText,
  webhookHeadersToText,
} from '@/admin-mappers/forms/misc'
import type { PendingActions } from './async-action'
import { settingKey } from './settings-keys'

export type LlmReviewForm = {
  enabled: boolean
  review_base_url: string
  review_api_key: string
  review_model: string
  review_timeout_ms: number
  failure_policy: ReviewFailurePolicy
  pending_ttl_seconds: number
  custom_policy_text: string
  webhook_enabled: boolean
  webhook_url: string
  webhook_bearer_token: string
  webhook_headers_text: string
}

export type LlmReviewField = keyof LlmReviewForm

export const LLM_REVIEW_FIELDS: ReadonlyArray<LlmReviewField> = [
  'enabled',
  'review_base_url',
  'review_api_key',
  'review_model',
  'review_timeout_ms',
  'failure_policy',
  'pending_ttl_seconds',
  'custom_policy_text',
  'webhook_enabled',
  'webhook_url',
  'webhook_bearer_token',
  'webhook_headers_text',
]

export const LLM_REVIEW_KEY = settingKey('llmReview')

export const REVIEW_TIMEOUT_RANGE = { min: 100 } as const
export const PENDING_TTL_RANGE = { min: 1 } as const

/** The failure policies the backend accepts, in the security-first order. */
export const FAILURE_POLICY_OPTIONS: ReadonlyArray<{
  labelKey: 'failurePolicyFailClosed' | 'failurePolicyFailOpen'
  value: ReviewFailurePolicy
}> = [
  { labelKey: 'failurePolicyFailClosed', value: 'fail_closed' },
  { labelKey: 'failurePolicyFailOpen', value: 'fail_open' },
]

/** The server default, flattened for the form. */
export function createLlmReviewForm(): LlmReviewForm {
  return {
    enabled: false,
    review_base_url: '',
    review_api_key: '',
    review_model: '',
    review_timeout_ms: 3000,
    failure_policy: 'fail_closed',
    pending_ttl_seconds: 300,
    custom_policy_text: '',
    webhook_enabled: false,
    webhook_url: '',
    webhook_bearer_token: '',
    webhook_headers_text: '',
  }
}

/** A stored settings object projected onto the flat form. */
export function formFromSettings(settings: LlmReviewSettings): LlmReviewForm {
  const safe = ensureLlmReviewDefaults(settings)
  const webhook = safe.webhook ?? {
    enabled: false,
    url: '',
    bearer_token: '',
    extra_headers: {},
  }
  return {
    enabled: safe.enabled,
    review_base_url: safe.review_base_url,
    review_api_key: safe.review_api_key,
    review_model: safe.review_model,
    review_timeout_ms: safe.review_timeout_ms,
    failure_policy: safe.failure_policy,
    pending_ttl_seconds: safe.pending_ttl_seconds,
    custom_policy_text: safe.custom_policy_text,
    webhook_enabled: webhook.enabled,
    webhook_url: webhook.url,
    webhook_bearer_token: webhook.bearer_token,
    webhook_headers_text: webhookHeadersToText(webhook.extra_headers),
  }
}

/**
 * The PATCH body: every field, always, with the webhook assembled from its four
 * leaves. Header text is parsed by the caller so a malformed line can be
 * reported before any request.
 */
export function llmReviewFormToRequest(
  form: LlmReviewForm,
  extraHeaders: Record<string, string>,
): LlmReviewSettings {
  return {
    enabled: form.enabled,
    review_base_url: form.review_base_url,
    review_api_key: form.review_api_key,
    review_model: form.review_model,
    review_timeout_ms: form.review_timeout_ms,
    failure_policy: form.failure_policy,
    pending_ttl_seconds: form.pending_ttl_seconds,
    custom_policy_text: form.custom_policy_text,
    webhook: {
      enabled: form.webhook_enabled,
      url: form.webhook_url,
      bearer_token: form.webhook_bearer_token,
      extra_headers: extraHeaders,
    },
  }
}

/** Parse the extra-header text, or report that it is malformed. */
export function parseLlmReviewHeaders(
  text: string,
): { ok: true; headers: Record<string, string> } | { ok: false } {
  try {
    return { ok: true, headers: webhookHeadersFromText(text) }
  } catch {
    return { ok: false }
  }
}

/**
 * Apply a saved settings object to the leaves the operator has not changed
 * since the submit. The header text counts as one leaf.
 */
export function mergeLlmReviewResponse(
  current: LlmReviewForm,
  submitted: LlmReviewForm,
  response: LlmReviewSettings,
): LlmReviewForm {
  const normalized = formFromSettings(response)
  const merged: LlmReviewForm = { ...current }
  for (const field of LLM_REVIEW_FIELDS) {
    if (Object.is(current[field], submitted[field])) {
      Object.assign(merged, { [field]: normalized[field] })
    }
  }
  return merged
}

/** The leaves whose value differs from the last accepted form. */
export function llmReviewDirtyFields(
  current: LlmReviewForm,
  baseline: LlmReviewForm,
): LlmReviewField[] {
  return LLM_REVIEW_FIELDS.filter((field) => current[field] !== baseline[field])
}

/**
 * Whether a scoped refresh is safe: loaded, no read or save in flight, no
 * unsaved draft, and no failed save the operator has not retried.
 */
export function llmReviewRefreshBlocked(input: {
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
    Object.hasOwn(input.pendingActions, LLM_REVIEW_KEY)
  )
}
