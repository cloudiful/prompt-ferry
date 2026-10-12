import { mock } from 'bun:test'
import * as adminApi from '../src/generated/admin-api'

import type {
  LlmReviewSettings,
  ModelRouteWhitelistResponse,
  RawObjectStoreSettingsResponse,
  RequestContentLoggingResponse,
  UsageRetentionSettings,
} from '../src/generated/admin-api'

export const setRequestContentLogging = mock(
  async (_options: unknown) =>
    ({ mode: 'off', raw_retention_days: 3 }) as RequestContentLoggingResponse,
)
export const setUsageRetention = mock(
  async (_options: unknown) =>
    ({
      approval_retention_days: 90,
      content_retention_days: 3,
      metadata_retention_days: 90,
      raw_backend: 'object_store',
      raw_retention_days: 3,
      replay_enabled: true,
    }) as UsageRetentionSettings,
)
export const setStreamDeltaBatching = mock(
  async (_options: unknown) =>
    ({
      enabled: false,
      flush_window_ms: 50,
      max_buffer_bytes: 1024,
      max_buffer_chars: 160,
      flush_on_line_break: true,
      flush_on_sentence_end: false,
    }) as Record<string, unknown>,
)
export const setRelayIpWhitelist = mock(
  async (_options: unknown) =>
    ({
      allowed_cidrs: [],
      trusted_proxy_cidrs: [],
    }) as Record<string, unknown>,
)
export const setModelRouteWhitelist = mock(
  async (_options: unknown) =>
    ({ enabled: false }) as ModelRouteWhitelistResponse,
)
export const setLlmReviewSetting = mock(
  async (_options: unknown) =>
    ({
      enabled: false,
      failure_policy: 'fail_closed',
      pending_ttl_seconds: 300,
      review_base_url: '',
      review_api_key: '',
      review_model: '',
      review_timeout_ms: 30000,
      webhook: { enabled: false, url: '', bearer_token: '', extra_headers: {} },
    }) as LlmReviewSettings,
)
export const setRawObjectStore = mock(
  async (_options: unknown) =>
    ({
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
    }) as RawObjectStoreSettingsResponse,
)

export const settingsActionApiMocks = {
  setLlmReviewSetting,
  setModelRouteWhitelist,
  setRawObjectStore,
  setRelayIpWhitelist,
  setRequestContentLogging,
  setStreamDeltaBatching,
  setUsageRetention,
}

const defaults: Record<string, (options: unknown) => Promise<unknown>> = {
  setLlmReviewSetting: setLlmReviewSetting.getMockImplementation() as never,
  setModelRouteWhitelist:
    setModelRouteWhitelist.getMockImplementation() as never,
  setRawObjectStore: setRawObjectStore.getMockImplementation() as never,
  setRelayIpWhitelist: setRelayIpWhitelist.getMockImplementation() as never,
  setRequestContentLogging:
    setRequestContentLogging.getMockImplementation() as never,
  setStreamDeltaBatching:
    setStreamDeltaBatching.getMockImplementation() as never,
  setUsageRetention: setUsageRetention.getMockImplementation() as never,
}

export const apiMocks = Object.values(settingsActionApiMocks)

export function restoreDefaults(): void {
  for (const [name, fallback] of Object.entries(defaults)) {
    const target =
      settingsActionApiMocks[name as keyof typeof settingsActionApiMocks]
    if (target && fallback) target.mockImplementation(fallback)
  }
}

export function registerAdminApiMock(): void {
  mock.module('../src/generated/admin-api', () => ({
    ...adminApi,
    ...settingsActionApiMocks,
  }))
}
