import type { QueryClient } from '@tanstack/react-query'
import type {
  ModelEndpointRule,
  ModelRoutePageResponse,
  ModelRouteRequest,
} from '@/generated/admin-api'
import {
  deleteModelRouteById,
  fetchModelRoutesPage,
  persistModelRoute,
  runModelRouteProbe,
  updateModelRouteEnabled,
} from '@/stores/endpoints-api'
import { modelRoutePagesQueryKey } from '@/lib/endpoints'

export function modelRoutePageQueryKey(first: number, rows: number) {
  return [...modelRoutePagesQueryKey(), first, rows] as const
}

export async function listModelRoutePage(
  first: number,
  rows: number,
): Promise<ModelRoutePageResponse> {
  return fetchModelRoutesPage(first, rows)
}

export async function saveModelRoute(
  ruleId: string | null,
  body: ModelRouteRequest,
): Promise<ModelEndpointRule> {
  return persistModelRoute(ruleId, body)
}

export async function removeModelRoute(ruleId: string): Promise<void> {
  return deleteModelRouteById(ruleId)
}

export async function testModelRoute(ruleId: string) {
  return runModelRouteProbe(ruleId)
}

export async function setModelRouteEnabled(
  route: ModelEndpointRule,
  enabled: boolean,
): Promise<ModelEndpointRule> {
  return updateModelRouteEnabled(route, enabled)
}

export async function refreshModelRoutePage(
  queryClient: QueryClient,
  first: number,
  rows: number,
): Promise<ModelRoutePageResponse> {
  await queryClient.cancelQueries({
    queryKey: modelRoutePageQueryKey(first, rows),
    exact: true,
  })
  await queryClient.invalidateQueries({
    queryKey: modelRoutePagesQueryKey(),
    refetchType: 'none',
  })

  let requestedFirst = first
  while (true) {
    const page = await queryClient.fetchQuery({
      queryKey: modelRoutePageQueryKey(requestedFirst, rows),
      queryFn: () => listModelRoutePage(requestedFirst, rows),
      staleTime: 0,
    })
    const pageRows = page.rows > 0 ? page.rows : rows
    if (
      pageRows > 0 &&
      page.routes.length === 0 &&
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

export function patchModelRoutePages(
  queryClient: QueryClient,
  route: ModelEndpointRule,
): void {
  queryClient.setQueriesData<ModelRoutePageResponse>(
    { queryKey: modelRoutePagesQueryKey() },
    (page) =>
      page
        ? {
            ...page,
            routes: page.routes.map((item) =>
              item.rule_id === route.rule_id ? route : item,
            ),
          }
        : page,
  )
}
