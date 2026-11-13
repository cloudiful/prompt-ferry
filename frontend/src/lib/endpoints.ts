import type { QueryClient } from '@tanstack/react-query'
import { listEndpoints } from '@/generated/admin-api'
import type {
  EndpointPageResponse,
  EndpointRequest,
  ProviderEndpoint,
} from '@/generated/admin-api'
import {
  listEndpointsQueryKey,
  listModelRoutesQueryKey,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { expectData, withData } from '@/api'
import {
  endpointFormToRequest,
  endpointToForm,
} from '@/admin-mappers/forms/endpoint'
import { refreshPagedQuery } from './paged-query'

const endpointRecordsNamespace = ['endpoint-records'] as const

async function fetchEndpointsPage(
  first: number,
  rows: number,
): Promise<EndpointPageResponse> {
  return expectData(
    await listEndpoints<true>(withData({ query: { first, rows } })),
  )
}

export function endpointsPageQueryKey(first: number, rows: number) {
  return listEndpointsQueryKey({ query: { first, rows } })
}

export function endpointsPagesQueryKey() {
  return listEndpointsQueryKey()
}

export function endpointRecordsQueryKey() {
  return endpointRecordsNamespace
}

export function modelRoutePagesQueryKey() {
  return listModelRoutesQueryKey()
}

export async function listAllEndpointRecords(): Promise<ProviderEndpoint[]> {
  const endpoints = new Map<string, ProviderEndpoint>()
  let first = 0
  let total = 0
  let rows = 200

  do {
    const page = await fetchEndpointsPage(first, rows)
    rows = page.rows > 0 ? page.rows : rows
    total = page.total
    const previousSize = endpoints.size
    for (const endpoint of page.endpoints)
      endpoints.set(endpoint.endpoint_id, endpoint)
    if (endpoints.size < total && endpoints.size === previousSize)
      throw new Error('Endpoint list pagination made no progress')
    if (endpoints.size < total && page.endpoints.length === 0)
      throw new Error(
        'Endpoint list pagination ended before the reported total',
      )
    first = page.first + rows
  } while (endpoints.size < total)

  return [...endpoints.values()]
}

export function buildEndpointEnabledRequest(
  endpoint: ProviderEndpoint,
  enabled: boolean,
): EndpointRequest {
  const form = endpointToForm(endpoint)
  form.enabled = enabled
  return endpointFormToRequest(form)
}

export async function refreshEndpointsPage(
  queryClient: QueryClient,
  first: number,
  rows: number,
): Promise<EndpointPageResponse> {
  return refreshPagedQuery<EndpointPageResponse, ProviderEndpoint>(
    queryClient,
    {
      queryKey: endpointsPageQueryKey,
      namespaceKey: endpointsPagesQueryKey(),
      fetchPage: fetchEndpointsPage,
      items: (page) => page.endpoints,
    },
    first,
    rows,
  )
}

export function patchEndpointPages(
  queryClient: QueryClient,
  endpoint: ProviderEndpoint,
): void {
  queryClient.setQueriesData<EndpointPageResponse>(
    { queryKey: endpointsPagesQueryKey() },
    (page) =>
      page
        ? {
            ...page,
            endpoints: page.endpoints.map((item) =>
              item.endpoint_id === endpoint.endpoint_id ? endpoint : item,
            ),
          }
        : page,
  )
}
