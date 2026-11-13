import type { QueryClient } from '@tanstack/react-query'
import type {
  RequestRecordFacets,
  RequestRecordListRow,
  User,
} from '@/generated/admin-api'
import {
  listRequestRecordsOptions,
  listRequestRecordsQueryKey,
  listUserOptionsOptions,
  requestRecordFacetsOptions,
  requestRecordOverviewOptions,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import {
  isUsageEmptyTail,
  usageLastPageFirst,
  type RequestRecordOverviewResponse,
  type UsageFacetsQuery,
  type UsageListQuery,
  type UsageOverviewQuery,
  type UsageScope,
} from './usage-model'

export type UsagePage = {
  records: RequestRecordListRow[]
  total: number
  first: number
  rows: number
}

export type UsageDrilldown = {
  endpoint_id?: string | null
  model?: string | null
  mcp_server_id?: string | null
  mcp_bearer_token_slot?: number | null
  mcp_scope?: UsageScope | null
}

export async function refreshUsageRecords(
  queryClient: QueryClient,
  query: UsageListQuery,
): Promise<UsagePage> {
  await queryClient.cancelQueries({
    queryKey: listRequestRecordsQueryKey({ query }),
    exact: true,
  })
  let page: UsagePage = await queryClient.fetchQuery({
    ...listRequestRecordsOptions({ query }),
    staleTime: 0,
  })
  if (isUsageEmptyTail(page)) {
    const first = usageLastPageFirst(page.total, page.rows)
    if (first !== page.first) {
      page = await queryClient.fetchQuery({
        ...listRequestRecordsOptions({ query: { ...query, first } }),
        staleTime: 0,
      })
    }
  }
  return page
}

export async function refreshUsageFacets(
  queryClient: QueryClient,
  query: UsageFacetsQuery,
): Promise<RequestRecordFacets> {
  return queryClient.fetchQuery({
    ...requestRecordFacetsOptions({ query }),
    staleTime: 0,
  })
}

export async function refreshUsageOverview(
  queryClient: QueryClient,
  query: UsageOverviewQuery,
): Promise<RequestRecordOverviewResponse> {
  return queryClient.fetchQuery({
    ...requestRecordOverviewOptions({ query }),
    staleTime: 0,
  })
}

export async function refreshUsageUserOptions(
  queryClient: QueryClient,
): Promise<User[]> {
  const response = await queryClient.fetchQuery({
    ...listUserOptionsOptions(),
    staleTime: 0,
  })
  return response.users
}
