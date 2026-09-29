import { expect, test } from 'bun:test'
import {
  createEmptyEndpointForm,
  endpointFormToRequest,
  endpointToForm,
  normalizeServiceTier,
} from '../src/admin-mappers/forms/endpoint'
import type { ProviderEndpoint } from '../src/generated/admin-api'
import { endpointMessages } from '../src/i18n/modules/endpoints'

function endpointFixture(
  overrides: Partial<ProviderEndpoint> = {},
): ProviderEndpoint {
  return {
    api_keys: [],
    base_url: 'https://api.minimaxi.com',
    created_at: '2026-09-04T00:00:00Z',
    enabled: true,
    endpoint_id: 'endpoint-1',
    key_lb_enabled: false,
    mcp_enabled: false,
    name: 'minimax',
    native_api: 'chat',
    native_api_source: 'manual',
    provider: 'minimax',
    scope: 'admin',
    updated_at: '2026-09-04T00:00:00Z',
    ...overrides,
  }
}

test('new endpoint forms default to inherit', () => {
  expect(createEmptyEndpointForm().service_tier).toBeNull()
})

test('normalizeServiceTier trims values and treats blank as inherit', () => {
  expect(normalizeServiceTier('priority')).toBe('priority')
  expect(normalizeServiceTier('standard')).toBe('standard')
  // Issue #637: free-form values survive so a provider-specific vocabulary
  // (e.g. OpenAI `fast`) is never coerced back to a MiniMax default.
  expect(normalizeServiceTier('fast')).toBe('fast')
  expect(normalizeServiceTier(' legacy-unknown ')).toBe('legacy-unknown')
  expect(normalizeServiceTier(undefined)).toBeNull()
  expect(normalizeServiceTier(null)).toBeNull()
  expect(normalizeServiceTier('')).toBeNull()
  expect(normalizeServiceTier('   ')).toBeNull()
})

test('endpointToForm preserves a configured tier and inherits legacy values', () => {
  expect(
    endpointToForm(endpointFixture({ service_tier: 'priority' })).service_tier,
  ).toBe('priority')
  expect(
    endpointToForm(endpointFixture({ service_tier: 'standard' })).service_tier,
  ).toBe('standard')
  expect(
    endpointToForm(endpointFixture({ service_tier: 'fast' })).service_tier,
  ).toBe('fast')
  expect(
    endpointToForm(endpointFixture({ service_tier: null })).service_tier,
  ).toBeNull()
  const { service_tier, ...legacy } = endpointFixture()
  expect(endpointToForm(legacy as ProviderEndpoint).service_tier).toBeNull()
})

test('endpointFormToRequest round-trips the service tier and inherits blank', () => {
  const priority = {
    ...createEmptyEndpointForm(),
    service_tier: 'priority',
  }
  expect(endpointFormToRequest(priority).service_tier).toBe('priority')
  const freeForm = {
    ...createEmptyEndpointForm(),
    service_tier: ' fast ',
  }
  expect(endpointFormToRequest(freeForm).service_tier).toBe('fast')
  expect(
    endpointFormToRequest(createEmptyEndpointForm()).service_tier,
  ).toBeNull()
  const blank = {
    ...createEmptyEndpointForm(),
    service_tier: '   ',
  }
  expect(endpointFormToRequest(blank).service_tier).toBeNull()
})

test('a configured tier is submitted for every provider', () => {
  // Issue #644: the override is provider-agnostic, so the request keeps a
  // configured value on any provider instead of inheriting it away.
  for (const provider of [
    'generic',
    'minimax',
    'openai',
    'deepseek',
    'glm',
    'openrouter',
    'command_code',
    'opencode_go',
  ] as const) {
    const configured = {
      ...createEmptyEndpointForm(),
      provider,
      service_tier: 'priority',
    }
    expect(endpointFormToRequest(configured).service_tier).toBe('priority')
  }
})

test('service tier copy explains the free-form inherit contract', () => {
  for (const locale of ['zh-CN', 'en-US'] as const) {
    const messages = endpointMessages[locale]
    expect(messages.serviceTier.length).toBeGreaterThan(0)
    expect(messages.proxyInherit.length).toBeGreaterThan(0)
    expect(messages.serviceTierHint.length).toBeGreaterThan(0)
  }
  // Issue #637: the tier control reuses the shared `proxyInherit` copy as its
  // blank/inherited placeholder, which must read exactly `继承` in zh-CN.
  expect(endpointMessages['zh-CN'].proxyInherit).toBe('继承')
})

test('endpoint settings copy names the service-tier control', () => {
  // Issue #644: the endpoint settings subpage now also hosts the tier control.
  for (const locale of ['zh-CN', 'en-US'] as const) {
    const messages = endpointMessages[locale]
    expect(messages.endpointSettingsHint.toLowerCase()).toContain(
      messages.serviceTier.toLowerCase(),
    )
  }
})
