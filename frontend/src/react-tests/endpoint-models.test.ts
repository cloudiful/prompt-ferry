import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  EndpointModelsResponse,
  EndpointPageResponse,
  ProviderEndpoint,
} from '@/generated/admin-api'
import {
  clearEndpointModelCatalogCache,
  ENDPOINT_MODEL_CATALOG_TTL_MS,
  endpointModelCatalogKey,
  loadEndpointModelCatalog,
} from '@/lib/endpoint-models'
import {
  endpointRecordsQueryKey,
  listAllEndpointRecords,
} from '@/lib/endpoints'
import { fetchEndpointModels, fetchEndpointsPage } from '@/stores/endpoints-api'

vi.mock('@/stores/endpoints-api', () => ({
  fetchEndpointModels: vi.fn(),
  fetchEndpointsPage: vi.fn(),
}))

const fetchModelsMock = vi.mocked(fetchEndpointModels)
const fetchPageMock = vi.mocked(fetchEndpointsPage)

function endpoint(id: string): ProviderEndpoint {
  return {
    active_windows: [],
    api_keys: [],
    base_url: `https://${id}.example.test`,
    created_at: '2026-10-23T12:00:00.000Z',
    enabled: true,
    endpoint_id: id,
    has_admin_api_key: false,
    has_oauth_token: false,
    has_proxy_url: false,
    key_lb_enabled: false,
    mcp_enabled: false,
    name: id,
    native_api: 'auto',
    native_api_source: 'auto',
    owner_user_id: null,
    plan: 'platform_api_key',
    provider: 'generic',
    provider_region: 'global',
    scope: 'global',
    service_tier: null,
    updated_at: '2026-10-23T12:00:00.000Z',
  }
}

function page(
  endpoints: ProviderEndpoint[],
  first: number,
  total: number,
): EndpointPageResponse {
  return { endpoints, first, rows: 200, total }
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.useFakeTimers()
  vi.setSystemTime(new Date('2026-10-24T12:00:00.000Z'))
  clearEndpointModelCatalogCache()
})

afterEach(() => {
  clearEndpointModelCatalogCache()
  vi.useRealTimers()
})

describe('endpoint model suggestions', () => {
  it('uses endpoint and protocol as the key and caches normalized successful results for one minute', async () => {
    fetchModelsMock.mockResolvedValue({
      models: [' z-model ', 'a-model', 'a-model', '  '],
    } as EndpointModelsResponse)

    expect(endpointModelCatalogKey('ep-1', 'chat')).toBe('ep-1::chat')
    expect(endpointModelCatalogKey('ep-1')).toBe('ep-1::auto')
    await expect(loadEndpointModelCatalog('ep-1', 'chat')).resolves.toEqual([
      'a-model',
      'z-model',
    ])
    await expect(loadEndpointModelCatalog('ep-1', 'chat')).resolves.toEqual([
      'a-model',
      'z-model',
    ])
    expect(fetchModelsMock).toHaveBeenCalledTimes(1)

    vi.advanceTimersByTime(ENDPOINT_MODEL_CATALOG_TTL_MS)
    await loadEndpointModelCatalog('ep-1', 'chat')
    expect(fetchModelsMock).toHaveBeenCalledTimes(2)
  })

  it('coalesces concurrent requests and retries a failed catalog without caching the failure', async () => {
    let resolve!: (value: EndpointModelsResponse) => void
    fetchModelsMock.mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done
        }),
    )
    const first = loadEndpointModelCatalog('ep-2', 'responses')
    const second = loadEndpointModelCatalog('ep-2', 'responses')
    expect(fetchModelsMock).toHaveBeenCalledTimes(1)
    resolve({ models: ['first'] })
    await expect(Promise.all([first, second])).resolves.toEqual([
      ['first'],
      ['first'],
    ])

    fetchModelsMock
      .mockRejectedValueOnce(new Error('catalog unavailable'))
      .mockResolvedValueOnce({ models: ['retry'] })
    await expect(loadEndpointModelCatalog('ep-3', 'chat')).rejects.toThrow(
      'catalog unavailable',
    )
    await expect(loadEndpointModelCatalog('ep-3', 'chat')).resolves.toEqual([
      'retry',
    ])
    expect(fetchModelsMock).toHaveBeenCalledTimes(3)
  })
})

describe('all endpoint records query', () => {
  it('loads every 200-row page and deduplicates endpoint ids', async () => {
    const firstPage = Array.from({ length: 200 }, (_, index) =>
      endpoint(`ep-${index}`),
    )
    fetchPageMock
      .mockResolvedValueOnce(page(firstPage, 0, 201))
      .mockResolvedValueOnce(
        page([firstPage[199]!, endpoint('ep-200')], 200, 201),
      )

    const records = await listAllEndpointRecords()

    expect(fetchPageMock.mock.calls).toEqual([
      [0, 200],
      [200, 200],
    ])
    expect(records).toHaveLength(201)
    expect(records.at(-1)?.endpoint_id).toBe('ep-200')
    expect(endpointRecordsQueryKey()).toEqual(['endpoint-records'])
  })

  it('fails instead of looping when pagination stops making progress', async () => {
    fetchPageMock
      .mockResolvedValueOnce(page([endpoint('ep-1')], 0, 2))
      .mockResolvedValueOnce(page([endpoint('ep-1')], 200, 2))

    await expect(listAllEndpointRecords()).rejects.toThrow(
      'pagination made no progress',
    )
    expect(fetchPageMock).toHaveBeenCalledTimes(2)
  })
})
