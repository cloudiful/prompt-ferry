import { expect, mock, test } from 'bun:test'
import type {
  ProviderEndpoint,
  TokenPlanUsageResponse,
} from '../src/generated/admin-api'
import type { EndpointListItemView } from '../src/models/endpoints'
import { createEndpointListItemView } from '../src/models/endpoints/endpoint-item'
import { isQuotaEligible } from '../src/models/endpoints/quota'
import * as endpointsApi from '../src/stores/endpoints-api'
import {
  prefetchTokenPlanBatch,
  renderEndpointMobileCard,
  renderEndpointsTable,
} from './helpers/quota-list-renderer'

// Partial mock (Bun spread pattern): the dialog gate is observable without
// leaving the module's real exports behind for the other test files.
const fetchTokenPlanUsage = mock(
  async (_endpointId: string): Promise<TokenPlanUsageResponse> => ({
    keys: [],
    provider: 'minimax',
    provider_region: null,
  }),
)

mock.module('../src/stores/endpoints-api', () => ({
  ...endpointsApi,
  fetchTokenPlanUsage,
}))

function listItem(
  overrides: Partial<ProviderEndpoint> = {},
): EndpointListItemView {
  return createEndpointListItemView(
    {
      api_keys: [],
      base_url: 'https://api.openai.com',
      created_at: '2026-09-27T00:00:00Z',
      enabled: true,
      endpoint_id: 'endpoint-1',
      key_lb_enabled: false,
      mcp_enabled: false,
      name: 'openai',
      native_api: 'responses',
      native_api_source: 'manual',
      provider: 'openai',
      scope: 'admin',
      updated_at: '2026-09-27T00:00:00Z',
      ...overrides,
    },
    {
      endpointTestIdle: 'idle',
      endpointSourceAuto: 'auto',
      endpointSourceDetected: 'detected',
      endpointSourceManual: 'manual',
      nativeApiAnthropicMessages: 'anthropic',
      nativeApiChat: 'chat',
      nativeApiResponses: 'responses',
      nativeApiRealtime: 'realtime',
      ownerLabel: 'owner',
      scopeAdmin: 'admin',
      scopeUser: 'user',
      testResult: null,
      testingEndpointId: '',
      togglingEndpointId: '',
    },
  )
}

const subscribedItem = listItem({
  endpoint_id: 'sub',
  plan: 'chatgpt_subscription',
  has_oauth_token: true,
})
const platformItem = listItem({ endpoint_id: 'plain' })
const minimaxItem = listItem({ endpoint_id: 'mm', provider: 'minimax' })
const genericItem = listItem({ endpoint_id: 'gen', provider: 'generic' })

// Read the renderer's stub contract: the desktop table reports its columns
// as an attribute, and the badge mock records the ids it was asked to fetch.
function tableColumns(html: string): string[] {
  const match = html.match(/data-columns="([^"]*)"/)
  return match?.[1] ? match[1].split(',') : []
}

function prefetchedIds(): string[] {
  return prefetchTokenPlanBatch.mock.calls.flatMap(([ids]) => [
    ...(ids as Iterable<string>),
  ])
}

test('OpenAI is quota-eligible only with a stored subscription token', () => {
  expect(isQuotaEligible(subscribedItem)).toBe(true)
  expect(isQuotaEligible(platformItem)).toBe(false)
  expect(isQuotaEligible({ provider: 'openai' })).toBe(false)
  expect(isQuotaEligible({ provider: 'openai', has_oauth_token: null })).toBe(
    false,
  )
})

test('existing quota providers are unchanged and others stay closed', () => {
  for (const provider of [
    'minimax',
    'command_code',
    'opencode_go',
    'openrouter',
    'glm',
    'deepseek',
  ] as const) {
    expect(isQuotaEligible({ provider })).toBe(true)
    expect(isQuotaEligible({ provider, has_oauth_token: false })).toBe(true)
  }
  expect(isQuotaEligible(genericItem)).toBe(false)
})

test('the list view carries the derived plan and token presence', () => {
  expect(subscribedItem.plan).toBe('chatgpt_subscription')
  expect(subscribedItem.has_oauth_token).toBe(true)
  expect(platformItem.plan).toBe('platform_api_key')
  expect(platformItem.has_oauth_token).toBe(false)
  // Optional response fields must not make an OpenAI row eligible by accident.
  expect(isQuotaEligible(listItem({ provider: 'openai' }))).toBe(false)
})

test('desktop list exposes quota only on eligible rows', async () => {
  prefetchTokenPlanBatch.mockClear()
  const html = await renderEndpointsTable([
    subscribedItem,
    platformItem,
    minimaxItem,
  ])
  expect(tableColumns(html)).toContain('usage')
  expect(html).toContain('QUOTA:sub')
  expect(html).toContain('QUOTA:mm')
  expect(html).not.toContain('QUOTA:plain')
  expect(html.match(/aria-label="tokenPlanUsage"/g)?.length ?? 0).toBe(2)
  expect(prefetchedIds()).toEqual(['sub', 'mm'])
})

test('desktop list hides the usage column when no row is eligible', async () => {
  prefetchTokenPlanBatch.mockClear()
  const html = await renderEndpointsTable([platformItem, genericItem])
  expect(tableColumns(html)).not.toContain('usage')
  expect(html).not.toContain('QUOTA:')
  expect(prefetchedIds()).toEqual([])
})

test('mobile card renders subscription quota only for eligible rows', async () => {
  const cases = [
    [subscribedItem, true],
    [platformItem, false],
    [minimaxItem, true],
    [genericItem, false],
  ] as const
  for (const [item, visible] of cases) {
    prefetchTokenPlanBatch.mockClear()
    const html = await renderEndpointMobileCard(item)
    expect(html.includes(`QUOTA:${item.endpoint_id}`)).toBe(visible)
    expect(html.includes('tokenPlanUsage')).toBe(visible)
    expect(prefetchedIds()).toEqual(visible ? [item.endpoint_id] : [])
  }
})

const { useEndpointTokenPlanUsage } =
  await import('../src/composables/useEndpointTokenPlanUsage')

test('the usage dialog gate follows the shared eligibility', async () => {
  const cases = [
    [subscribedItem, true],
    [platformItem, false],
    [minimaxItem, true],
    [genericItem, false],
  ] as const
  for (const [item, opens] of cases) {
    fetchTokenPlanUsage.mockClear()
    const usage = useEndpointTokenPlanUsage(
      () => item,
      mock(() => {}),
    )
    await usage.openTokenPlanUsage(item.endpoint_id)
    expect(usage.tokenPlanUsageVisible.value).toBe(opens)
    expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(opens ? 1 : 0)
  }
  const missing = useEndpointTokenPlanUsage(
    () => null,
    mock(() => {}),
  )
  await missing.openTokenPlanUsage('missing')
  expect(missing.tokenPlanUsageVisible.value).toBe(false)
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(0)
})
