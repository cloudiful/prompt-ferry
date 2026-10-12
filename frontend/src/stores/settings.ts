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
  setRawObjectStore,
} from '../generated/admin-api'
import type {
  LlmReviewSettings,
  ModelRouteWhitelistResponse,
  RawObjectStoreSettingsRequest,
  RawObjectStoreSettingsResponse,
  RequestContentLoggingResponse,
  UsageRetentionSettings,
} from '../generated/admin-api'
import {
  ensureLlmReviewDefaults,
  relayPolicyToForm,
  streamDeltaBatchingToForm,
  webhookHeadersToText,
} from '../admin-mappers'
import { expectData, withData } from '../api'
import type { StreamDeltaBatchingForm } from '../models'
import { computed } from 'vue'
import { useAsyncAction } from '../composables/useAsyncAction'
import {
  SETTING_KEY_PREFIX,
  createSettingsResourceActions,
} from './settings-actions'
import { createSettingsSectionActions } from './settings-section-actions'
import { pendingRowsById } from './record-action-types'
import { createCacheAlertStore } from './settings/cache-alert'

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
  const {
    cacheAlert,
    cacheAlertError,
    loadCacheAlert,
    refreshCacheAlert,
    saveCacheAlert,
  } = createCacheAlertStore()

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
        loadCacheAlert(),
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

  async function writeRawObjectStore(
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

  // One guard per settings store: every section's save claims through it, so a
  // section is gated against every write that would touch it, not only against
  // the button the operator happened to press.
  const guard = useAsyncAction()
  const sectionActions = createSettingsSectionActions({
    guard,
    llmReview,
    llmReviewWebhookHeadersText,
    modelRouteWhitelist,
    relayIpWhitelist,
    requestContentLogging,
    streamDeltaBatching,
    usageRetention,
  })
  const resourceActions = createSettingsResourceActions({
    guard,
    saveCacheAlert,
    saveRawObjectStore: writeRawObjectStore,
  })

  // Per-section view of the guard, so a form can show only its own state
  // instead of the whole page going busy.
  const settingsPendingActions = computed(() =>
    pendingRowsById(guard.pendingActions.value, SETTING_KEY_PREFIX),
  )

  return {
    ...sectionActions,
    ...resourceActions,
    cacheAlert,
    cacheAlertError,
    llmReview,
    llmReviewWebhookHeadersText,
    loading,
    modelRouteWhitelist,
    rawObjectStore,
    rawObjectStoreError,
    isPending: guard.isPending,
    pendingActions: guard.pendingActions,
    refresh,
    refreshCacheAlert,
    refreshRawObjectStore,
    relayIpWhitelist,
    requestContentLogging,
    streamDeltaBatching,
    usageRetention,
  }
})
