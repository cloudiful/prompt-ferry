import type { QueryClient } from '@tanstack/react-query'
import { listModelRoutes } from '@/generated/admin-api'
import type {
  ModelEndpointRule,
  ModelRoutePageResponse,
  ModelRouteRequest,
} from '@/generated/admin-api'
import { listModelRoutesQueryKey } from '@/generated/admin-api/@tanstack/react-query.gen'
import { expectData, withData } from '@/api'
import {
  modelRouteFormToRequest,
  modelRouteToForm,
} from '@/admin-mappers/forms/model-route'
import { capturePayload } from '@/stores/record-action-types'
import { refreshPagedQuery } from './paged-query'
import { modelRouteTargetIndex } from '@/models/endpoints/model-route-item'

async function fetchModelRoutesPage(
  first: number,
  rows: number,
): Promise<ModelRoutePageResponse> {
  return expectData(
    await listModelRoutes<true>(withData({ query: { first, rows } })),
  )
}

export function modelRoutePageQueryKey(first: number, rows: number) {
  return listModelRoutesQueryKey({ query: { first, rows } })
}

export function buildModelRouteEnabledRequest(
  route: ModelEndpointRule,
  enabled: boolean,
): ModelRouteRequest {
  const form = modelRouteToForm(route)
  form.enabled = enabled
  return capturePayload(modelRouteFormToRequest(form))
}

/**
 * Enable or disable one configured target of a route.
 *
 * The save API replaces the whole target list, so the request is rebuilt from
 * the stored route: only the addressed target changes and every other target
 * keeps its configuration, its order and its omit-when-untouched secrets.
 */
export function buildModelRouteTargetEnabledRequest(
  route: ModelEndpointRule,
  targetId: string,
  enabled: boolean,
): ModelRouteRequest {
  const index = modelRouteTargetIndex(route, targetId)
  if (index < 0) throw new Error(`model route target ${targetId} not found`)
  const form = modelRouteToForm(route)
  const target = form.targets[index]
  if (!target) throw new Error(`model route target ${targetId} not found`)
  target.enabled = enabled
  return capturePayload(modelRouteFormToRequest(form))
}

export async function refreshModelRoutePage(
  queryClient: QueryClient,
  first: number,
  rows: number,
): Promise<ModelRoutePageResponse> {
  return refreshPagedQuery<ModelRoutePageResponse, ModelEndpointRule>(
    queryClient,
    {
      queryKey: modelRoutePageQueryKey,
      namespaceKey: listModelRoutesQueryKey(),
      fetchPage: fetchModelRoutesPage,
      items: (page) => page.routes,
    },
    first,
    rows,
  )
}

export function patchModelRoutePages(
  queryClient: QueryClient,
  route: ModelEndpointRule,
): void {
  queryClient.setQueriesData<ModelRoutePageResponse>(
    { queryKey: listModelRoutesQueryKey() },
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
