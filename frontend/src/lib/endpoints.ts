import type { QueryClient } from '@tanstack/react-query'
import { listUserOptions } from '@/generated/admin-api'
import type {
  EndpointPageResponse,
  ProviderEndpoint,
  User,
  UserOptionsResponse,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import {
  deleteEndpointById,
  fetchEndpointsPage,
  persistEndpoint,
  runEndpointTest,
  updateEndpointEnabled,
} from '@/stores/endpoints-api'
import type { EndpointRequest } from '@/generated/admin-api'
import { MODEL_ROUTE_CACHE_NAMESPACE } from '@/models/endpoints'

const endpointPagesNamespace = ['endpoints-page'] as const
const endpointRecordsNamespace = ['endpoint-records'] as const
const endpointUsersNamespace = ['endpoint-user-options'] as const

export function endpointsPageQueryKey(first: number, rows: number) {
  return [...endpointPagesNamespace, first, rows] as const
}

export function endpointsPagesQueryKey() {
  return endpointPagesNamespace
}

export function endpointRecordsQueryKey() {
  return endpointRecordsNamespace
}

export function endpointUserOptionsQueryKey(adminId: number) {
  return [...endpointUsersNamespace, adminId] as const
}

export function modelRoutePagesQueryKey() {
  return [MODEL_ROUTE_CACHE_NAMESPACE] as const
}

export async function listEndpointsPage(
  first: number,
  rows: number,
): Promise<EndpointPageResponse> {
  return fetchEndpointsPage(first, rows)
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

export async function listEndpointUsers(): Promise<User[]> {
  const result = expectData(
    await listUserOptions<true>(withData()),
  ) as UserOptionsResponse
  return result.users
}

export async function saveEndpointRecord(
  endpointId: string | null,
  body: EndpointRequest,
): Promise<ProviderEndpoint> {
  return persistEndpoint(endpointId, body)
}

export async function removeEndpointRecord(endpointId: string): Promise<void> {
  return deleteEndpointById(endpointId)
}

export async function setEndpointEnabled(
  endpoint: ProviderEndpoint,
  enabled: boolean,
): Promise<ProviderEndpoint> {
  return updateEndpointEnabled(endpoint, enabled)
}

export async function testEndpointRecord(endpointId: string) {
  return runEndpointTest(endpointId)
}

export async function refreshEndpointsPage(
  queryClient: QueryClient,
  first: number,
  rows: number,
): Promise<EndpointPageResponse> {
  const currentPageKey = endpointsPageQueryKey(first, rows)
  await queryClient.cancelQueries({ queryKey: currentPageKey, exact: true })
  await queryClient.invalidateQueries({
    queryKey: endpointsPagesQueryKey(),
    refetchType: 'none',
  })

  let requestedFirst = first
  while (true) {
    const page = await queryClient.fetchQuery({
      queryKey: endpointsPageQueryKey(requestedFirst, rows),
      queryFn: () => fetchEndpointsPage(requestedFirst, rows),
      staleTime: 0,
    })
    const pageRows = page.rows > 0 ? page.rows : rows
    if (
      pageRows > 0 &&
      page.endpoints.length === 0 &&
      requestedFirst >= page.total
    ) {
      const correctedFirst =
        page.total > 0 ? Math.floor((page.total - 1) / pageRows) * pageRows : 0
      if (correctedFirst !== requestedFirst) {
        requestedFirst = correctedFirst
        continue
      }
    }
    return page
  }
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
