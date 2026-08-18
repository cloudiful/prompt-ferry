import {
  createEndpoint,
  createModelRoute,
  deleteEndpoint,
  deleteModelRoute,
  listEndpoints,
  listModelRoutes,
  testEndpoint,
  tokenPlanUsage,
  testModelRoute,
  updateEndpoint,
  updateModelRoute,
} from '../generated/admin-api'
import type {
  EndpointPageResponse,
  EndpointRequest,
  EndpointTestResponse,
  ModelEndpointRule,
  ModelRoutePageResponse,
  ModelRouteRequest,
  ModelRouteTestResponse,
  ProviderEndpoint,
  TokenPlanUsageResponse,
} from '../generated/admin-api'
import {
  endpointFormToRequest,
  endpointToForm,
  modelRouteFormToRequest,
  modelRouteToForm,
} from '../admin-mappers'
import { expectData, withData } from '../api'

export async function fetchEndpointsPage(
  first: number,
  rows: number,
): Promise<EndpointPageResponse> {
  return expectData(
    await listEndpoints<true>(
      withData({
        query: { first, rows },
      }),
    ),
  )
}

export async function fetchModelRoutesPage(
  first: number,
  rows: number,
): Promise<ModelRoutePageResponse> {
  return expectData(
    await listModelRoutes<true>(
      withData({
        query: { first, rows },
      }),
    ),
  )
}

export async function persistEndpoint(
  endpointId: string | null,
  body: EndpointRequest,
): Promise<ProviderEndpoint> {
  if (endpointId) {
    return expectData(
      await updateEndpoint<true>(
        withData({
          path: { endpoint_id: endpointId },
          body,
        }),
      ),
    )
  }
  return expectData(await createEndpoint<true>(withData({ body })))
}

export async function deleteEndpointById(endpointId: string): Promise<void> {
  await deleteEndpoint<true>(withData({ path: { endpoint_id: endpointId } }))
}

export async function runEndpointTest(
  endpointId: string,
): Promise<EndpointTestResponse> {
  return expectData(
    await testEndpoint<true>(withData({ path: { endpoint_id: endpointId } })),
  )
}

export async function fetchTokenPlanUsage(
  endpointId: string,
): Promise<TokenPlanUsageResponse> {
  return expectData(
    await tokenPlanUsage<true>(withData({ path: { endpoint_id: endpointId } })),
  )
}

export async function updateEndpointEnabled(
  endpoint: ProviderEndpoint,
  enabled: boolean,
): Promise<ProviderEndpoint> {
  const form = endpointToForm(endpoint)
  form.enabled = enabled
  return expectData(
    await updateEndpoint<true>(
      withData({
        path: { endpoint_id: endpoint.endpoint_id },
        body: endpointFormToRequest(form),
      }),
    ),
  )
}

export async function persistModelRoute(
  ruleId: string | null,
  body: ModelRouteRequest,
): Promise<ModelEndpointRule> {
  if (ruleId) {
    return expectData(
      await updateModelRoute<true>(
        withData({
          path: { rule_id: ruleId },
          body,
        }),
      ),
    )
  }
  return expectData(await createModelRoute<true>(withData({ body })))
}

export async function deleteModelRouteById(ruleId: string): Promise<void> {
  await deleteModelRoute<true>(withData({ path: { rule_id: ruleId } }))
}

export async function runModelRouteProbe(
  ruleId: string,
): Promise<ModelRouteTestResponse> {
  return expectData(
    await testModelRoute<true>(withData({ body: { rule_id: ruleId } })),
  )
}

export async function updateModelRouteEnabled(
  route: ModelEndpointRule,
  enabled: boolean,
): Promise<ModelEndpointRule> {
  const form = modelRouteToForm(route)
  form.enabled = enabled
  return expectData(
    await updateModelRoute<true>(
      withData({
        path: { rule_id: route.rule_id },
        body: modelRouteFormToRequest(form),
      }),
    ),
  )
}
