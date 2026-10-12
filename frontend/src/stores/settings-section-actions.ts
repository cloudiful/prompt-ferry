/**
 * The per-section settings saves.
 *
 * Each of these used to be a bare `await` that wrote the section's form and then
 * reported itself, so a held key and a click sent two PUTs and toasted twice.
 * They share one gate per section, which is owned here rather than at each call
 * site: an autosave that fires while the same section's Save button is still
 * running has to collapse, and it can only do that if both go through one claim.
 *
 * The bodies live here instead of in the store so the store keeps to composing
 * state, and so the mapping of each response back onto its form sits next to the
 * request that produced it.
 */

import { ref, type Ref } from 'vue'

import type {
  AsyncActionGuard,
  AsyncActionOutcome,
} from '@/composables/useAsyncAction'
import type {
  LlmReviewSettings,
  LlmReviewWebhookSettings,
  ModelRouteWhitelistResponse,
  RequestContentLoggingResponse,
  RelayIpPolicyResponse,
  StreamDeltaBatchingSettings,
  UsageRetentionSettings,
} from '@/generated/admin-api'
import {
  setLlmReviewSetting,
  setModelRouteWhitelist,
  setRelayIpWhitelist,
  setRequestContentLogging,
  setStreamDeltaBatching,
  setUsageRetention,
} from '@/generated/admin-api'
import {
  ensureLlmReviewDefaults,
  relayFormToRequest,
  relayPolicyToForm,
  streamDeltaBatchingFormToRequest,
  streamDeltaBatchingToForm,
  webhookHeadersFromText,
  webhookHeadersToText,
} from '@/admin-mappers'
import { expectData, withData } from '@/api'
import type { StreamDeltaBatchingForm } from '@/models'

import { capturePayload } from '@/stores/record-action-types'
import { settingFormKey } from '@/stores/settings-actions'
import type { SettingForm } from '@/stores/settings-actions'

export type SettingsSectionDeps = {
  guard: AsyncActionGuard
  llmReview: Ref<LlmReviewSettings>
  llmReviewWebhookHeadersText: Ref<string>
  modelRouteWhitelist: Ref<ModelRouteWhitelistResponse>
  relayIpWhitelist: Ref<{
    allowed_cidrs_text: string
    trusted_proxy_cidrs_text: string
  }>
  requestContentLogging: Ref<RequestContentLoggingResponse>
  streamDeltaBatching: Ref<StreamDeltaBatchingForm>
  usageRetention: Ref<UsageRetentionSettings>
}

/**
 * Settings forms are gated by the resource they write, not by the form that
 * exposes it. The two retention forms share `raw_retention_days`, so their
 * writes must claim the same key even though they have separate autosave flows.
 */
/**
 * The form that currently holds the shared retention key.
 *
 * Both retention forms are reported busy while either one writes, because both
 * are waiting on the same stored value — but only the form that actually claimed
 * the key has a write in flight, so only that one spins.
 */
export type PendingRetentionForm = Ref<SettingForm | null>

export function createSettingsSectionActions(deps: SettingsSectionDeps) {
  const {
    guard,
    llmReview,
    llmReviewWebhookHeadersText,
    modelRouteWhitelist,
    relayIpWhitelist,
    requestContentLogging,
    streamDeltaBatching,
    usageRetention,
  } = deps

  const retentionOrigin = ref<SettingForm | null>(null)

  function clearOriginIfMine(form: SettingForm): void {
    if (retentionOrigin.value === form) {
      retentionOrigin.value = null
    }
  }

  /**
   * Claim the shared retention key on one form's behalf.
   *
   * `guard.run` claims synchronously, so reading the key immediately before the
   * run is what distinguishes the two forms: if the key was free, this call is
   * the one holding it. A declined call leaves the winner's spinner alone.
   */
  function claimRetention(
    form: SettingForm,
    write: () => Promise<void>,
  ): Promise<AsyncActionOutcome<void>> {
    const key = settingFormKey(form)
    const claimed = !guard.isPending(key)
    const outcome = guard.run(key, write, 'settings')
    if (claimed) {
      retentionOrigin.value = form
      // The guard releases the key in its own `finally`, so clearing here keeps
      // the origin in step with the key rather than ahead of it.
      void outcome.then(
        () => clearOriginIfMine(form),
        () => clearOriginIfMine(form),
      )
    }
    return outcome
  }

  function saveRequestContentLogging(): Promise<AsyncActionOutcome<void>> {
    return claimRetention('requestContentLogging', async () => {
      const submitted = capturePayload(requestContentLogging.value)
      const sharedRawRetention = usageRetention.value.raw_retention_days
      const response = expectData(
        await setRequestContentLogging<true>(withData({ body: submitted })),
      )
      if (requestContentLogging.value.mode === submitted.mode) {
        requestContentLogging.value.mode = response.mode
      }
      if (
        requestContentLogging.value.raw_retention_days ===
        submitted.raw_retention_days
      ) {
        requestContentLogging.value.raw_retention_days =
          response.raw_retention_days
      }
      if (usageRetention.value.raw_retention_days === sharedRawRetention) {
        usageRetention.value.raw_retention_days = response.raw_retention_days
      }
    })
  }

  function saveUsageRetention(): Promise<AsyncActionOutcome<void>> {
    return claimRetention('usageRetention', async () => {
      const submitted = capturePayload(usageRetention.value)
      const sharedRawRetention = requestContentLogging.value.raw_retention_days
      const response = expectData(
        await setUsageRetention<true>(withData({ body: submitted })),
      )
      const merged = { ...usageRetention.value }
      for (const key of Object.keys(submitted) as Array<
        keyof UsageRetentionSettings
      >) {
        if (Object.is(usageRetention.value[key], submitted[key])) {
          Object.assign(merged, { [key]: response[key] })
        }
      }
      usageRetention.value = merged
      if (
        requestContentLogging.value.raw_retention_days === sharedRawRetention
      ) {
        requestContentLogging.value.raw_retention_days =
          response.raw_retention_days ?? sharedRawRetention
      }
    })
  }

  function saveStreamDeltaBatching(): Promise<
    AsyncActionOutcome<StreamDeltaBatchingForm>
  > {
    return guard.run(
      settingFormKey('streamDeltaBatching'),
      async () => {
        const submitted = capturePayload(streamDeltaBatching.value)
        const response: StreamDeltaBatchingSettings = expectData(
          await setStreamDeltaBatching<true>(
            withData({ body: streamDeltaBatchingFormToRequest(submitted) }),
          ),
        )
        const confirmed = streamDeltaBatchingToForm(response)
        const merged = { ...streamDeltaBatching.value }
        for (const key of Object.keys(submitted) as Array<
          keyof StreamDeltaBatchingForm
        >) {
          if (Object.is(streamDeltaBatching.value[key], submitted[key])) {
            Object.assign(merged, { [key]: confirmed[key] })
          }
        }
        streamDeltaBatching.value = merged
        return confirmed
      },
      'settings',
    )
  }

  function saveRelayIpWhitelist(): Promise<AsyncActionOutcome<void>> {
    return guard.run(
      settingFormKey('relayIpWhitelist'),
      async () => {
        const response: RelayIpPolicyResponse = expectData(
          await setRelayIpWhitelist<true>(
            withData({
              body: capturePayload(relayFormToRequest(relayIpWhitelist.value)),
            }),
          ),
        )
        relayIpWhitelist.value = relayPolicyToForm(response)
      },
      'settings',
    )
  }

  function saveModelRouteWhitelist(): Promise<
    AsyncActionOutcome<ModelRouteWhitelistResponse>
  > {
    return guard.run(
      settingFormKey('modelRouteWhitelist'),
      async () => {
        const submitted = capturePayload(modelRouteWhitelist.value)
        const response = expectData(
          await setModelRouteWhitelist<true>(withData({ body: submitted })),
        )
        if (Object.is(modelRouteWhitelist.value.enabled, submitted.enabled)) {
          modelRouteWhitelist.value = response
        }
        return response
      },
      'settings',
    )
  }

  function saveLlmReview(): Promise<AsyncActionOutcome<void>> {
    return guard.run(
      settingFormKey('llmReview'),
      async () => {
        // The header text is a textarea, not structured form state, so the
        // webhook body is assembled from both before the request goes out.
        const webhook: LlmReviewWebhookSettings = {
          enabled: llmReview.value.webhook?.enabled ?? false,
          url: llmReview.value.webhook?.url ?? '',
          bearer_token: llmReview.value.webhook?.bearer_token ?? '',
          extra_headers: webhookHeadersFromText(
            llmReviewWebhookHeadersText.value,
          ),
        }
        llmReview.value = expectData(
          await setLlmReviewSetting<true>(
            withData({ body: { ...llmReview.value, webhook } }),
          ),
        )
        llmReview.value = ensureLlmReviewDefaults(llmReview.value)
        llmReviewWebhookHeadersText.value = webhookHeadersToText(
          llmReview.value.webhook?.extra_headers ?? {},
        )
      },
      'settings',
    )
  }

  return {
    pendingRetentionForm: retentionOrigin,
    saveRequestContentLogging,
    saveUsageRetention,
    saveStreamDeltaBatching,
    saveRelayIpWhitelist,
    saveModelRouteWhitelist,
    saveLlmReview,
  }
}

export type SettingsSectionActions = ReturnType<
  typeof createSettingsSectionActions
>
