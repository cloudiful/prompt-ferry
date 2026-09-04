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

test('new endpoint forms default to the standard service tier', () => {
  expect(createEmptyEndpointForm().service_tier).toBe('standard')
})

test('normalizeServiceTier keeps priority and falls back to standard', () => {
  expect(normalizeServiceTier('priority')).toBe('priority')
  expect(normalizeServiceTier('standard')).toBe('standard')
  expect(normalizeServiceTier(undefined)).toBe('standard')
  expect(normalizeServiceTier(null)).toBe('standard')
  expect(normalizeServiceTier('legacy-unknown')).toBe('standard')
})

test('endpointToForm preserves priority and defaults legacy values', () => {
  expect(
    endpointToForm(endpointFixture({ service_tier: 'priority' })).service_tier,
  ).toBe('priority')
  expect(
    endpointToForm(endpointFixture({ service_tier: 'standard' })).service_tier,
  ).toBe('standard')
  const { service_tier, ...legacy } = endpointFixture()
  expect(endpointToForm(legacy as ProviderEndpoint).service_tier).toBe(
    'standard',
  )
})

test('endpointFormToRequest round-trips the service tier', () => {
  const priority = {
    ...createEmptyEndpointForm(),
    service_tier: 'priority' as const,
  }
  expect(endpointFormToRequest(priority).service_tier).toBe('priority')
  expect(endpointFormToRequest(createEmptyEndpointForm()).service_tier).toBe(
    'standard',
  )
})

test('service tier copy explains priority admission and cost', () => {
  for (const locale of ['zh-CN', 'en-US'] as const) {
    const messages = endpointMessages[locale]
    expect(messages.serviceTier.length).toBeGreaterThan(0)
    expect(messages.serviceTierStandard.length).toBeGreaterThan(0)
    expect(messages.serviceTierPriority.length).toBeGreaterThan(0)
    expect(messages.serviceTierHint).toContain('1.5')
  }
})
