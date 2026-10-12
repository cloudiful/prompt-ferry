import type { Ref } from 'vue'

import type {
  AsyncActionGuard,
  AsyncActionOutcome,
} from '@/composables/useAsyncAction'
import type {
  EndpointRequest,
  EndpointTestResponse,
  ModelEndpointRule,
  ModelRouteRequest,
  ModelRouteTestResponse,
  ProviderEndpoint,
} from '@/generated/admin-api'
import {
  deleteEndpointById,
  deleteModelRouteById,
  persistEndpoint,
  persistModelRoute,
  runEndpointTest as requestEndpointTest,
  runModelRouteProbe,
  updateEndpointEnabled,
  updateModelRouteEnabled,
} from '@/stores/endpoints-api'
import { CREATE_KEY, capturePayload } from '@/stores/record-action-types'
import type { RunEndpointOAuthAction } from '@/stores/endpoint-oauth-action-types'

// The key grammar, row projection, and payload snapshot are shared with every
// other record-scoped action module, so they live in `record-action-types` and
// are re-exported here: the endpoints domain remains the one place its callers
// import them from.
export { CREATE_KEY, pendingRowsById } from '@/stores/record-action-types'
export type {
  PendingActionLabel,
  PendingRowActions,
} from '@/stores/record-action-types'

/**
 * Key namespaces for the guarded writes.
 *
 * Save, delete, and toggle of one record deliberately share a key so they
 * cannot run against the same record at once; create has its own key because it
 * is not yet a record the table can act on. The network probes sit behind a
 * separate prefix so a running test never blocks a write, and vice versa.
 */
export const ENDPOINT_KEY_PREFIX = 'endpoint'
export const MODEL_ROUTE_KEY_PREFIX = 'modelroute'
export const NETWORK_TEST_KEY_PREFIX = 'networktest'

export type EndpointActionKeyDomain = 'endpoint' | 'modelroute'

export function endpointActionKey(endpointId: string | null): string {
  return `${ENDPOINT_KEY_PREFIX}:${endpointId ?? CREATE_KEY}`
}

export function modelRouteActionKey(ruleId: string | null): string {
  return `${MODEL_ROUTE_KEY_PREFIX}:${ruleId ?? CREATE_KEY}`
}

export function networkTestKey(
  domain: EndpointActionKeyDomain,
  id: string,
): string {
  return `${NETWORK_TEST_KEY_PREFIX}:${domain}:${id}`
}

type EndpointSlice = {
  items: Ref<ProviderEndpoint[]>
  testingId: Ref<string>
  togglingId: Ref<string>
  testResults: Ref<Record<string, EndpointTestResponse>>
}

type ModelRouteSlice = {
  items: Ref<ModelEndpointRule[]>
  testingId: Ref<string>
  togglingId: Ref<string>
  testResults: Ref<Record<string, ModelRouteTestResponse>>
}

/**
 * Everything the actions need, injected rather than reached for, so the store
 * stays a thin state holder and these actions can be exercised on plain refs.
 */
export type EndpointActionDeps = {
  guard: AsyncActionGuard
  endpoints: EndpointSlice
  modelRoutes: ModelRouteSlice
  reloadWorkspace: () => Promise<void>
  reloadModelRouting: () => Promise<void>
}

export function createEndpointActions(deps: EndpointActionDeps) {
  const { endpoints, guard, modelRoutes, reloadModelRouting, reloadWorkspace } =
    deps

  function saveEndpoint(
    endpointId: string | null,
    body: EndpointRequest,
  ): Promise<AsyncActionOutcome<ProviderEndpoint>> {
    const captured = capturePayload(body)
    return guard.run(
      endpointActionKey(endpointId),
      async () => {
        const saved = await persistEndpoint(endpointId, captured)
        // The guard is held across the reload too, so the spinner covers the
        // refreshed list rather than stopping at the write.
        await reloadWorkspace()
        return saved
      },
      'save',
    )
  }

  function removeEndpoint(
    endpointId: string,
  ): Promise<AsyncActionOutcome<void>> {
    return guard.run(
      endpointActionKey(endpointId),
      async () => {
        await deleteEndpointById(endpointId)
        await reloadWorkspace()
      },
      'delete',
    )
  }

  function toggleEndpointEnabled(
    endpointId: string,
    enabled: boolean,
  ): Promise<AsyncActionOutcome<ProviderEndpoint>> {
    return guard.run(
      endpointActionKey(endpointId),
      async () => {
        endpoints.togglingId.value = endpointId
        try {
          const endpoint = endpoints.items.value.find(
            (item) => item.endpoint_id === endpointId,
          )
          if (!endpoint) {
            throw new Error(`Endpoint not found: ${endpointId}`)
          }
          const saved = await updateEndpointEnabled(endpoint, enabled)
          endpoints.items.value = endpoints.items.value.map((row) =>
            row.endpoint_id === endpointId ? saved : row,
          )
          return saved
        } finally {
          endpoints.togglingId.value = ''
        }
      },
      'toggle',
    )
  }

  const runEndpointOAuthAction: RunEndpointOAuthAction = (
    endpointId,
    action,
    label,
  ) => guard.run(endpointActionKey(endpointId), action, label)

  function runEndpointTest(
    endpointId: string,
  ): Promise<AsyncActionOutcome<EndpointTestResponse>> {
    return guard.run(
      networkTestKey('endpoint', endpointId),
      async () => {
        endpoints.testingId.value = endpointId
        try {
          const result = await requestEndpointTest(endpointId)
          endpoints.testResults.value[endpointId] = result
          return result
        } finally {
          endpoints.testingId.value = ''
        }
      },
      'test',
    )
  }

  function saveModelRoute(
    ruleId: string | null,
    body: ModelRouteRequest,
  ): Promise<AsyncActionOutcome<ModelEndpointRule>> {
    const captured = capturePayload(body)
    return guard.run(
      modelRouteActionKey(ruleId),
      async () => {
        const saved = await persistModelRoute(ruleId, captured)
        await reloadModelRouting()
        return saved
      },
      'save',
    )
  }

  function removeModelRoute(ruleId: string): Promise<AsyncActionOutcome<void>> {
    return guard.run(
      modelRouteActionKey(ruleId),
      async () => {
        await deleteModelRouteById(ruleId)
        await reloadModelRouting()
      },
      'delete',
    )
  }

  function toggleModelRouteEnabled(
    ruleId: string,
    enabled: boolean,
  ): Promise<AsyncActionOutcome<ModelEndpointRule>> {
    return guard.run(
      modelRouteActionKey(ruleId),
      async () => {
        modelRoutes.togglingId.value = ruleId
        try {
          const route = modelRoutes.items.value.find(
            (item) => item.rule_id === ruleId,
          )
          if (!route) {
            throw new Error(`Model route not found: ${ruleId}`)
          }
          const saved = await updateModelRouteEnabled(route, enabled)
          modelRoutes.items.value = modelRoutes.items.value.map((row) =>
            row.rule_id === ruleId ? saved : row,
          )
          return saved
        } finally {
          modelRoutes.togglingId.value = ''
        }
      },
      'toggle',
    )
  }

  function runModelRouteTest(
    ruleId: string,
  ): Promise<AsyncActionOutcome<ModelRouteTestResponse>> {
    return guard.run(
      networkTestKey('modelroute', ruleId),
      async () => {
        modelRoutes.testingId.value = ruleId
        try {
          const result = await runModelRouteProbe(ruleId)
          modelRoutes.testResults.value[ruleId] = result
          return result
        } finally {
          modelRoutes.testingId.value = ''
        }
      },
      'test',
    )
  }

  return {
    removeEndpoint,
    removeModelRoute,
    runEndpointOAuthAction,
    runEndpointTest,
    runModelRouteTest,
    saveEndpoint,
    saveModelRoute,
    toggleEndpointEnabled,
    toggleModelRouteEnabled,
  }
}

export type EndpointActions = ReturnType<typeof createEndpointActions>
