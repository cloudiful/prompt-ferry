import type {
  EndpointTestResponse,
  ProviderEndpoint,
  ModelEndpointRule,
  ModelRouteTestResponse,
} from '../generated/admin-api'
import type { Option } from '../models'
import {
  createEndpointListItemView,
  type EndpointViewItemLabels,
} from './endpoints/endpoint-item'
import {
  createModelRouteListItemView,
  type ModelRouteViewItemLabels,
} from './endpoints/model-route-item'

export type EndpointListItemView = {
  endpoint_id: string
  name: string
  base_url: string
  scope: string
  scope_label: string
  native_api: string
  native_api_label: string
  native_api_source: string
  native_api_source_label: string
  enabled: boolean
  owner_label: string
  testing: boolean
  toggling: boolean
  test_message: string
  test_severity: 'success' | 'error' | null
}

export type ModelRouteTargetItemView = {
  target_id: string
  endpoint_label: string
  endpoint_enabled: boolean
  upstream_model: string | null
}

export type ModelRouteListItemView = {
  rule_id: string
  model_pattern: string
  scope: string
  scope_label: string
  enabled: boolean
  owner_label: string
  routing_strategy_label: string
  targets: ModelRouteTargetItemView[]
  testing: boolean
  toggling: boolean
  test_message: string
  test_severity: 'success' | 'error' | null
}

export type EndpointsWorkspaceView = {
  busy: boolean
  endpoint_first: number
  endpoint_rows: number
  endpoint_total: number
  endpoint_items: EndpointListItemView[]
  model_route_first: number
  model_route_rows: number
  model_route_total: number
  model_route_items: ModelRouteListItemView[]
  endpoint_options: Option<string>[]
}

type EndpointViewLabels = {
  endpointTestIdle: string
  endpointSourceAuto: string
  endpointSourceDetected: string
  endpointSourceManual: string
  nativeApiAnthropicMessages: string
  nativeApiChat: string
  nativeApiResponses: string
  nativeApiRealtime: string
  owner: string
  routingStrategyClientKey: string
  routingStrategySessionAffinity: string
  scopeAdmin: string
  scopeUser: string
}

export function createEndpointsWorkspaceView(options: {
  busy: boolean
  data: {
    endpoints: {
      items: ProviderEndpoint[]
      page: {
        first: number
        rows: number
        total: number
      }
      testResults: Record<string, EndpointTestResponse>
    }
    modelRoutes: {
      items: ModelEndpointRule[]
      page: {
        first: number
        rows: number
        total: number
      }
      testResults: Record<string, ModelRouteTestResponse>
    }
  }
  labels: EndpointViewLabels
  status: {
    endpoint: {
      testingId: string
      togglingId: string
    }
    modelRoute: {
      testingId: string
      togglingId: string
    }
  }
}): EndpointsWorkspaceView {
  const endpointLabels: EndpointViewItemLabels = {
    endpointTestIdle: options.labels.endpointTestIdle,
    endpointSourceAuto: options.labels.endpointSourceAuto,
    endpointSourceDetected: options.labels.endpointSourceDetected,
    endpointSourceManual: options.labels.endpointSourceManual,
    nativeApiAnthropicMessages: options.labels.nativeApiAnthropicMessages,
    nativeApiChat: options.labels.nativeApiChat,
    nativeApiResponses: options.labels.nativeApiResponses,
    nativeApiRealtime: options.labels.nativeApiRealtime,
    ownerLabel: options.labels.owner,
    scopeAdmin: options.labels.scopeAdmin,
    scopeUser: options.labels.scopeUser,
  }
  const modelRouteLabels: ModelRouteViewItemLabels = {
    endpointTestIdle: options.labels.endpointTestIdle,
    ownerLabel: options.labels.owner,
    routingStrategyClientKey: options.labels.routingStrategyClientKey,
    routingStrategySessionAffinity:
      options.labels.routingStrategySessionAffinity,
    scopeAdmin: options.labels.scopeAdmin,
    scopeUser: options.labels.scopeUser,
  }
  return {
    busy: options.busy,
    endpoint_first: options.data.endpoints.page.first,
    endpoint_rows: options.data.endpoints.page.rows,
    endpoint_total: options.data.endpoints.page.total,
    endpoint_items: options.data.endpoints.items.map((endpoint) =>
      createEndpointListItemView(endpoint, {
        ...endpointLabels,
        testResult:
          options.data.endpoints.testResults[endpoint.endpoint_id] ?? null,
        testingEndpointId: options.status.endpoint.testingId,
        togglingEndpointId: options.status.endpoint.togglingId,
      }),
    ),
    model_route_first: options.data.modelRoutes.page.first,
    model_route_rows: options.data.modelRoutes.page.rows,
    model_route_total: options.data.modelRoutes.page.total,
    model_route_items: options.data.modelRoutes.items.map((route) =>
      createModelRouteListItemView(route, {
        ...modelRouteLabels,
        testResult: options.data.modelRoutes.testResults[route.rule_id] ?? null,
        testingModelRouteId: options.status.modelRoute.testingId,
        togglingModelRouteId: options.status.modelRoute.togglingId,
      }),
    ),
    endpoint_options: options.data.endpoints.items.map((endpoint) => ({
      label: endpoint.name,
      value: endpoint.endpoint_id,
    })),
  }
}
