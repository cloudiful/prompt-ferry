import { expect, mock, test } from 'bun:test'
import type { OpenAiOrganizationUsageResponse } from '../src/generated/admin-api'
import * as endpointsApi from '../src/stores/endpoints-api'

const usageFixture: OpenAiOrganizationUsageResponse = {
  cached: false,
  cost_usd: 12.5,
  currency: 'usd',
  fetched_at: '2026-09-27T10:00:00Z',
  input_tokens: 1000,
  output_tokens: 250,
  period_end: '2026-09-27T10:00:00Z',
  period_start: '2026-09-01T00:00:00Z',
  provider: 'openai',
  total_tokens: 1250,
  truncated: false,
}

const fetchOrganizationUsage = mock(
  async (_endpointId: string): Promise<OpenAiOrganizationUsageResponse> =>
    usageFixture,
)

mock.module('../src/stores/endpoints-api', () => ({
  ...endpointsApi,
  fetchOrganizationUsage,
}))

const { useEndpointOrganizationUsage } =
  await import('../src/composables/useEndpointOrganizationUsage')

test('loads organization usage for an OpenAI endpoint with an Admin API Key', async () => {
  fetchOrganizationUsage.mockClear()
  const usage = useEndpointOrganizationUsage()
  await usage.loadOrganizationUsage({
    endpointId: 'endpoint-openai',
    provider: 'openai',
    has_admin_api_key: true,
  })
  expect(fetchOrganizationUsage).toHaveBeenCalledTimes(1)
  expect(fetchOrganizationUsage).toHaveBeenCalledWith('endpoint-openai')
  expect(usage.organizationUsage.value).toEqual(usageFixture)
  expect(usage.organizationUsageLoading.value).toBe(false)
  expect(usage.errorMessage.value).toBe('')
})

test('stays closed without an endpoint id, provider, or key', async () => {
  const cases = [
    { endpointId: '', provider: 'openai' as const, has_admin_api_key: true },
    { endpointId: 'e1', provider: 'generic' as const, has_admin_api_key: true },
    { endpointId: 'e1', provider: 'openai' as const, has_admin_api_key: false },
    { endpointId: 'e1', provider: 'openai' as const, has_admin_api_key: null },
  ]
  for (const source of cases) {
    fetchOrganizationUsage.mockClear()
    const usage = useEndpointOrganizationUsage()
    await usage.loadOrganizationUsage(source)
    expect(fetchOrganizationUsage).toHaveBeenCalledTimes(0)
    expect(usage.organizationUsage.value).toBeNull()
  }
})

test('surfaces the API error in the UI state', async () => {
  fetchOrganizationUsage.mockClear()
  fetchOrganizationUsage.mockImplementationOnce(async () => {
    throw new Error('organization usage unavailable')
  })
  const usage = useEndpointOrganizationUsage()
  await usage.loadOrganizationUsage({
    endpointId: 'endpoint-openai',
    provider: 'openai',
    has_admin_api_key: true,
  })
  expect(usage.organizationUsage.value).toBeNull()
  expect(usage.errorMessage.value).toBe('organization usage unavailable')
  expect(usage.organizationUsageLoading.value).toBe(false)
  fetchOrganizationUsage.mockImplementation(async () => usageFixture)
})

test('reset clears a previous endpoint snapshot', async () => {
  fetchOrganizationUsage.mockClear()
  const usage = useEndpointOrganizationUsage()
  await usage.loadOrganizationUsage({
    endpointId: 'endpoint-openai',
    provider: 'openai',
    has_admin_api_key: true,
  })
  expect(usage.organizationUsage.value).not.toBeNull()
  usage.resetOrganizationUsage()
  expect(usage.organizationUsage.value).toBeNull()
  expect(usage.errorMessage.value).toBe('')
})
