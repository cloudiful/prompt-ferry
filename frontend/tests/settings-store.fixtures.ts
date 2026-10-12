import { reactive, toRef, type Ref } from 'vue'

import type {
  LlmReviewSettings,
  ModelRouteWhitelistResponse,
  RawObjectStoreSettingsResponse,
  RequestContentLoggingResponse,
  UsageRetentionSettings,
} from '../src/generated/admin-api'
import type { StreamDeltaBatchingForm } from '../src/models'
import { useAsyncAction } from '../src/composables/useAsyncAction'
import type { AsyncActionGuard } from '../src/composables/useAsyncAction'
import { withData } from '../src/api'
import {
  apiMocks,
  registerAdminApiMock,
  restoreDefaults,
  setRawObjectStore,
  settingsActionApiMocks,
} from './settings-action-data.fixtures'

/** A gate the tests open by hand, so "in flight" is really in flight. */
export function deferred<T = void>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

function settingsState() {
  return reactive({
    llmReview: {
      enabled: false,
      failure_policy: 'fail_closed',
      pending_ttl_seconds: 300,
      review_base_url: '',
      review_api_key: '',
      review_model: '',
      review_timeout_ms: 30000,
      webhook: {
        enabled: false,
        url: '',
        bearer_token: '',
        extra_headers: {} as Record<string, unknown>,
      },
    } as LlmReviewSettings,
    llmReviewWebhookHeadersText: '',
    modelRouteWhitelist: { enabled: false } as ModelRouteWhitelistResponse,
    rawObjectStore: {
      backend: 'local',
      has_s3_access_key: false,
      has_s3_secret_key: false,
      local_dir: '',
      s3_allow_http: false,
      s3_bucket: '',
      s3_endpoint: '',
      s3_path_style: true,
      s3_prefix: 'prompt-ferry/raw',
      s3_region: 'auto',
    } as RawObjectStoreSettingsResponse,
    relayIpWhitelist: { allowed_cidrs_text: '', trusted_proxy_cidrs_text: '' },
    requestContentLogging: {
      mode: 'off',
      raw_retention_days: 3,
    } as RequestContentLoggingResponse,
    streamDeltaBatching: {
      enabled: false,
      flush_window_ms: 50,
      max_buffer_bytes: 1024,
      max_buffer_chars: 160,
      flush_on_line_break: true,
      flush_on_sentence_end: false,
    } as StreamDeltaBatchingForm,
    usageRetention: {
      approval_retention_days: 90,
      content_retention_days: 3,
      metadata_retention_days: 90,
      raw_backend: 'object_store',
      raw_retention_days: 3,
      replay_enabled: true,
    } as UsageRetentionSettings,
  })
}

export type SettingsHarness = {
  actions: Record<string, never>
  api: typeof settingsActionApiMocks
  cacheAlertSaves: { count: number }
  guard: AsyncActionGuard
  pageStore: Record<string, never>
  rawObjectStore: Ref<RawObjectStoreSettingsResponse>
  requestContentLogging: Ref<RequestContentLoggingResponse>
  refs: Record<string, Ref<never>>
}

export async function freshHarness(): Promise<SettingsHarness> {
  restoreDefaults()
  for (const item of apiMocks) item.mockClear()
  registerAdminApiMock()

  // Imported after the module is mocked, so the factories bind the mock.
  const sections = await import('../src/stores/settings-section-actions')
  const resources = await import('../src/stores/settings-actions')

  const guard = useAsyncAction()
  const state = settingsState()
  const llmReview = toRef(state, 'llmReview')
  const llmReviewWebhookHeadersText = toRef(
    state,
    'llmReviewWebhookHeadersText',
  )
  const modelRouteWhitelist = toRef(state, 'modelRouteWhitelist')
  const rawObjectStore = toRef(state, 'rawObjectStore')
  const relayIpWhitelist = toRef(state, 'relayIpWhitelist')
  const requestContentLogging = toRef(state, 'requestContentLogging')
  const streamDeltaBatching = toRef(state, 'streamDeltaBatching')
  const usageRetention = toRef(state, 'usageRetention')
  const cacheAlertSaves = { count: 0 }

  const sectionActions = sections.createSettingsSectionActions({
    guard,
    llmReview,
    llmReviewWebhookHeadersText,
    modelRouteWhitelist,
    relayIpWhitelist,
    requestContentLogging,
    streamDeltaBatching,
    usageRetention,
  })

  const resourceActions = resources.createSettingsResourceActions({
    guard,
    saveCacheAlert: async () => {
      cacheAlertSaves.count += 1
    },
    // Goes through the API mock the way the store does, so a test can hold the
    // storage write open and see the payload it was actually sent.
    saveRawObjectStore: async (payload) => {
      const response = (await setRawObjectStore(
        withData({ body: payload as never }),
      )) as RawObjectStoreSettingsResponse
      rawObjectStore.value = response
      return response
    },
  })

  return {
    actions: { ...sectionActions, ...resourceActions },
    api: settingsActionApiMocks,
    cacheAlertSaves,
    guard,
    rawObjectStore,
    requestContentLogging,
    // The surface a page reads: plain values off the reactive state, plus the
    // guarded saves. Spelled out rather than spread, so nothing the guard owns
    // leaks into what the page can serialize.
    pageStore: {
      loading: false,
      isPending: guard.isPending,
      get llmReview() {
        return llmReview.value
      },
      get llmReviewWebhookHeadersText() {
        return llmReviewWebhookHeadersText.value
      },
      get modelRouteWhitelist() {
        return modelRouteWhitelist.value
      },
      get rawObjectStore() {
        return rawObjectStore.value
      },
      get relayIpWhitelist() {
        return relayIpWhitelist.value
      },
      get requestContentLogging() {
        return requestContentLogging.value
      },
      get streamDeltaBatching() {
        return streamDeltaBatching.value
      },
      get usageRetention() {
        return usageRetention.value
      },
      ...sectionActions,
      ...resourceActions,
    },
    refs: {
      requestContentLogging,
      usageRetention,
    },
  } as unknown as SettingsHarness
}
