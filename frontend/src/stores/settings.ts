import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
  getLlmReviewSetting,
  getModelRouteWhitelist,
  getRequestContentLogging,
  getStreamDeltaBatching,
  getRelayIpWhitelist,
  setLlmReviewSetting,
  setModelRouteWhitelist,
  setRequestContentLogging,
  setStreamDeltaBatching,
  setRelayIpWhitelist,
} from '../generated/admin-api'
import type {
  LlmReviewSettings,
  LlmReviewWebhookSettings,
  ModelRouteWhitelistResponse,
  RequestContentLoggingResponse,
  RelayIpPolicyResponse,
  StreamDeltaBatchingSettings,
} from '../generated/admin-api'
import {
  ensureLlmReviewDefaults,
  relayFormToRequest,
  relayPolicyToForm,
  streamDeltaBatchingFormToRequest,
  streamDeltaBatchingToForm,
  webhookHeadersFromText,
  webhookHeadersToText,
} from '../admin-mappers'
import { expectData, withData } from '../api'
import type { StreamDeltaBatchingForm } from '../models'

const emptyLlmReview = ensureLlmReviewDefaults({
  enabled: false,
  review_base_url: '',
  review_api_key: '',
  review_model: '',
  review_timeout_ms: 30000,
  failure_policy: 'fail_closed',
  pending_ttl_seconds: 300,
  custom_policy_text: '',
  webhook: {
    enabled: false,
    url: '',
    bearer_token: '',
    extra_headers: {},
  },
})

export const useSettingsStore = defineStore('settings', () => {
  const loading = ref(false)
  const requestContentLogging = ref<RequestContentLoggingResponse>({
    mode: 'off',
    raw_retention_days: 3,
  })
  const streamDeltaBatching = ref<StreamDeltaBatchingForm>({
    enabled: false,
    flush_window_ms: 50,
    max_buffer_chars: 160,
    max_buffer_bytes: 1024,
    flush_on_line_break: true,
    flush_on_sentence_end: false,
  })
  const relayIpWhitelist = ref<{
    allowed_cidrs_text: string
    trusted_proxy_cidrs_text: string
  }>({
    allowed_cidrs_text: '',
    trusted_proxy_cidrs_text: '',
  })
  const modelRouteWhitelist = ref<ModelRouteWhitelistResponse>({
    enabled: false,
  })
  const llmReview = ref<LlmReviewSettings>(emptyLlmReview)
  const llmReviewWebhookHeadersText = ref('')

  async function refresh(): Promise<void> {
    loading.value = true
    try {
      const [
        contentLogging,
        streamCoalescing,
        relayPolicy,
        routeWhitelist,
        reviewSettings,
      ] = await Promise.all([
        getRequestContentLogging<true>(withData()),
        getStreamDeltaBatching<true>(withData()),
        getRelayIpWhitelist<true>(withData()),
        getModelRouteWhitelist<true>(withData()),
        getLlmReviewSetting<true>(withData()),
      ])
      requestContentLogging.value = expectData(contentLogging)
      streamDeltaBatching.value = streamDeltaBatchingToForm(
        expectData(streamCoalescing),
      )
      relayIpWhitelist.value = relayPolicyToForm(expectData(relayPolicy))
      modelRouteWhitelist.value = expectData(routeWhitelist)
      llmReview.value = ensureLlmReviewDefaults(expectData(reviewSettings))
      llmReviewWebhookHeadersText.value = webhookHeadersToText(
        llmReview.value.webhook?.extra_headers ?? {},
      )
    } finally {
      loading.value = false
    }
  }

  async function saveRequestContentLogging(): Promise<void> {
    requestContentLogging.value = expectData(
      await setRequestContentLogging<true>(
        withData({ body: requestContentLogging.value }),
      ),
    )
  }

  async function saveStreamDeltaBatching(): Promise<void> {
    const response: StreamDeltaBatchingSettings = expectData(
      await setStreamDeltaBatching<true>(
        withData({
          body: streamDeltaBatchingFormToRequest(streamDeltaBatching.value),
        }),
      ),
    )
    streamDeltaBatching.value = streamDeltaBatchingToForm(response)
  }

  async function saveRelayIpWhitelist(): Promise<void> {
    const response: RelayIpPolicyResponse = expectData(
      await setRelayIpWhitelist<true>(
        withData({ body: relayFormToRequest(relayIpWhitelist.value) }),
      ),
    )
    relayIpWhitelist.value = relayPolicyToForm(response)
  }

  async function saveModelRouteWhitelist(): Promise<void> {
    modelRouteWhitelist.value = expectData(
      await setModelRouteWhitelist<true>(
        withData({ body: modelRouteWhitelist.value }),
      ),
    )
  }

  async function saveLlmReview(): Promise<void> {
    const webhook: LlmReviewWebhookSettings = {
      enabled: llmReview.value.webhook?.enabled ?? false,
      url: llmReview.value.webhook?.url ?? '',
      bearer_token: llmReview.value.webhook?.bearer_token ?? '',
      extra_headers: webhookHeadersFromText(llmReviewWebhookHeadersText.value),
    }
    llmReview.value = expectData(
      await setLlmReviewSetting<true>(
        withData({
          body: {
            ...llmReview.value,
            webhook,
          },
        }),
      ),
    )
    llmReview.value = ensureLlmReviewDefaults(llmReview.value)
    llmReviewWebhookHeadersText.value = webhookHeadersToText(
      llmReview.value.webhook?.extra_headers ?? {},
    )
  }

  return {
    llmReview,
    llmReviewWebhookHeadersText,
    loading,
    modelRouteWhitelist,
    refresh,
    relayIpWhitelist,
    saveLlmReview,
    saveModelRouteWhitelist,
    saveRelayIpWhitelist,
    requestContentLogging,
    saveRequestContentLogging,
    saveStreamDeltaBatching,
    streamDeltaBatching,
  }
})
