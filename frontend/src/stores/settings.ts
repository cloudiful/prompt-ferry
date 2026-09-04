import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
  getLlmReviewSetting,
  getModelRouteWhitelist,
  getRawObjectStore,
  getRequestContentLogging,
  getStreamDeltaBatching,
  getRelayIpWhitelist,
  getUsageRetention,
  setLlmReviewSetting,
  setModelRouteWhitelist,
  setRawObjectStore,
  setRequestContentLogging,
  setStreamDeltaBatching,
  setRelayIpWhitelist,
  setUsageRetention,
} from '../generated/admin-api'
import type {
  LlmReviewSettings,
  LlmReviewWebhookSettings,
  ModelRouteWhitelistResponse,
  RawObjectStoreSettingsRequest,
  RawObjectStoreSettingsResponse,
  RequestContentLoggingResponse,
  RelayIpPolicyResponse,
  StreamDeltaBatchingSettings,
  UsageRetentionSettings,
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
  const rawObjectStore = ref<RawObjectStoreSettingsResponse>({
    backend: 'local',
    local_dir: '',
    s3_endpoint: '',
    s3_bucket: '',
    s3_region: 'auto',
    s3_prefix: 'prompt-ferry/raw',
    s3_allow_http: false,
    s3_path_style: true,
    has_s3_access_key: false,
    has_s3_secret_key: false,
  })
  const rawObjectStoreError = ref<string | null>(null)
  const requestContentLogging = ref<RequestContentLoggingResponse>({
    mode: 'off',
    raw_retention_days: 3,
  })
  const usageRetention = ref<UsageRetentionSettings>({
    metadata_retention_days: 90,
    content_retention_days: 3,
    raw_retention_days: 3,
    approval_retention_days: 90,
    replay_enabled: true,
    raw_backend: 'object_store',
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
    rawObjectStoreError.value = null
    try {
      const [
        contentLogging,
        streamCoalescing,
        relayPolicy,
        routeWhitelist,
        reviewSettings,
        retention,
      ] = await Promise.all([
        getRequestContentLogging<true>(withData()),
        getStreamDeltaBatching<true>(withData()),
        getRelayIpWhitelist<true>(withData()),
        getModelRouteWhitelist<true>(withData()),
        getLlmReviewSetting<true>(withData()),
        getUsageRetention<true>(withData()),
      ])
      requestContentLogging.value = expectData(contentLogging)
      streamDeltaBatching.value = streamDeltaBatchingToForm(
        expectData(streamCoalescing),
      )
      relayIpWhitelist.value = relayPolicyToForm(expectData(relayPolicy))
      modelRouteWhitelist.value = expectData(routeWhitelist)
      llmReview.value = ensureLlmReviewDefaults(expectData(reviewSettings))
      usageRetention.value = expectData(retention)
      requestContentLogging.value.raw_retention_days =
        usageRetention.value.raw_retention_days ??
        requestContentLogging.value.raw_retention_days
      llmReviewWebhookHeadersText.value = webhookHeadersToText(
        llmReview.value.webhook?.extra_headers ?? {},
      )
      try {
        const raw = await getRawObjectStore<true>(withData())
        rawObjectStore.value = expectData(raw)
        rawObjectStoreError.value = null
      } catch (cause) {
        const message =
          cause instanceof Error ? cause.message : String(cause ?? '')
        const isUnavailable =
          message.toLowerCase().includes('sqlite') ||
          message.toLowerCase().includes('not available') ||
          message.toLowerCase().includes('unavailable')
        if (isUnavailable) {
          rawObjectStoreError.value = message || 'unavailable'
        } else {
          rawObjectStoreError.value = message || 'failed to load'
        }
      }
    } finally {
      loading.value = false
    }
  }

  async function saveRequestContentLogging(): Promise<void> {
    const response = expectData(
      await setRequestContentLogging<true>(
        withData({ body: requestContentLogging.value }),
      ),
    )
    requestContentLogging.value = response
    usageRetention.value.raw_retention_days = response.raw_retention_days
  }

  async function saveUsageRetention(): Promise<void> {
    usageRetention.value = expectData(
      await setUsageRetention<true>(withData({ body: usageRetention.value })),
    )
    requestContentLogging.value.raw_retention_days =
      usageRetention.value.raw_retention_days ??
      requestContentLogging.value.raw_retention_days
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

  async function saveRawObjectStore(
    payload: RawObjectStoreSettingsRequest,
  ): Promise<RawObjectStoreSettingsResponse> {
    const response = expectData(
      await setRawObjectStore<true>(withData({ body: payload })),
    )
    rawObjectStore.value = response
    rawObjectStoreError.value = null
    return response
  }

  async function refreshRawObjectStore(): Promise<void> {
    try {
      const raw = await getRawObjectStore<true>(withData())
      rawObjectStore.value = expectData(raw)
      rawObjectStoreError.value = null
    } catch (cause) {
      const message =
        cause instanceof Error ? cause.message : String(cause ?? '')
      rawObjectStoreError.value = message
      throw cause
    }
  }

  return {
    llmReview,
    llmReviewWebhookHeadersText,
    loading,
    modelRouteWhitelist,
    rawObjectStore,
    rawObjectStoreError,
    refresh,
    refreshRawObjectStore,
    relayIpWhitelist,
    saveLlmReview,
    saveModelRouteWhitelist,
    saveRawObjectStore,
    saveRelayIpWhitelist,
    requestContentLogging,
    saveRequestContentLogging,
    saveStreamDeltaBatching,
    streamDeltaBatching,
    usageRetention,
    saveUsageRetention,
  }
})
