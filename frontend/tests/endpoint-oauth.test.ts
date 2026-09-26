import { expect, test } from 'bun:test'
import {
  createEmptyEndpointForm,
  endpointFormToRequest,
  endpointToForm,
  normalizeEndpointPlan,
  normalizeProviderPlan,
} from '../src/admin-mappers/forms/endpoint'
import type { ProviderEndpoint } from '../src/generated/admin-api'
import { endpointMessages } from '../src/i18n/modules/endpoints'

function endpointFixture(
  overrides: Partial<ProviderEndpoint> = {},
): ProviderEndpoint {
  return {
    api_keys: [],
    base_url: 'https://api.openai.com',
    created_at: '2026-09-26T00:00:00Z',
    enabled: true,
    endpoint_id: 'endpoint-1',
    key_lb_enabled: false,
    mcp_enabled: false,
    name: 'openai',
    native_api: 'responses',
    native_api_source: 'manual',
    provider: 'openai',
    scope: 'admin',
    updated_at: '2026-09-26T00:00:00Z',
    ...overrides,
  }
}

test('new endpoint forms start on the platform plan without a token', () => {
  const form = createEmptyEndpointForm()
  expect(form.plan).toBe('platform_api_key')
  expect(form.has_oauth_token).toBe(false)
})

test('normalizeEndpointPlan keeps subscription and falls back to platform', () => {
  expect(normalizeEndpointPlan('chatgpt_subscription')).toBe(
    'chatgpt_subscription',
  )
  expect(normalizeEndpointPlan('platform_api_key')).toBe('platform_api_key')
  expect(normalizeEndpointPlan(undefined)).toBe('platform_api_key')
  expect(normalizeEndpointPlan(null)).toBe('platform_api_key')
  expect(normalizeEndpointPlan('legacy-unknown')).toBe('platform_api_key')
})

test('normalizeProviderPlan forces the platform plan off OpenAI', () => {
  expect(normalizeProviderPlan('openai', 'chatgpt_subscription')).toBe(
    'chatgpt_subscription',
  )
  for (const provider of [
    'generic',
    'minimax',
    'command_code',
    'opencode_go',
    'openrouter',
    'glm',
    'deepseek',
  ] as const) {
    expect(normalizeProviderPlan(provider, 'chatgpt_subscription')).toBe(
      'platform_api_key',
    )
  }
})

test('endpointToForm carries the derived plan and token presence', () => {
  const subscribed = endpointToForm(
    endpointFixture({
      has_oauth_token: true,
      plan: 'chatgpt_subscription',
    }),
  )
  expect(subscribed.plan).toBe('chatgpt_subscription')
  expect(subscribed.has_oauth_token).toBe(true)

  const platform = endpointToForm(endpointFixture())
  expect(platform.plan).toBe('platform_api_key')
  expect(platform.has_oauth_token).toBe(false)

  // A stale plan on a non-OpenAI endpoint never survives the mapper.
  const legacy = endpointToForm(
    endpointFixture({ plan: 'chatgpt_subscription', provider: 'generic' }),
  )
  expect(legacy.plan).toBe('platform_api_key')
})

test('endpointFormToRequest sends the effective plan', () => {
  const subscribed = {
    ...createEmptyEndpointForm(),
    provider: 'openai' as const,
    plan: 'chatgpt_subscription' as const,
    has_oauth_token: true,
  }
  expect(endpointFormToRequest(subscribed).plan).toBe('chatgpt_subscription')
  expect(endpointFormToRequest(createEmptyEndpointForm()).plan).toBe(
    'platform_api_key',
  )
  const switched = {
    ...subscribed,
    provider: 'generic' as const,
  }
  expect(endpointFormToRequest(switched).plan).toBe('platform_api_key')
})

test('ChatGPT OAuth copy exists in both locales', () => {
  const keys = [
    'endpointPlan',
    'endpointPlanHint',
    'endpointPlanPlatformApiKey',
    'endpointPlanChatgptSubscription',
    'endpointPlanLoginRequired',
    'endpointOAuth',
    'endpointOAuthSaveFirst',
    'endpointOAuthNotLoggedIn',
    'endpointOAuthLoggedIn',
    'endpointOAuthExpired',
    'endpointOAuthExpiresAt',
    'endpointOAuthRefresh',
    'endpointOAuthClear',
    'endpointOAuthQuota',
    'endpointOAuthQuotaHint',
    'endpointOAuthQuotaPrimary',
    'endpointOAuthQuotaSecondary',
    'endpointOAuthQuotaUnavailable',
    'endpointOAuthDeviceLogin',
    'endpointOAuthDeviceStart',
    'endpointOAuthDeviceHint',
    'endpointOAuthDeviceWaiting',
    'endpointOAuthBrowserLogin',
    'endpointOAuthBrowserStart',
    'endpointOAuthBrowserHint',
    'endpointOAuthAuthorizeUrl',
    'endpointOAuthRedirectUrl',
    'endpointOAuthRedirectPlaceholder',
    'endpointOAuthComplete',
    'endpointOAuthCopy',
  ] as const
  for (const locale of ['zh-CN', 'en-US'] as const) {
    const messages = endpointMessages[locale] as Record<string, string>
    for (const key of keys) {
      expect(messages[key]?.length ?? 0).toBeGreaterThan(0)
    }
  }
})
