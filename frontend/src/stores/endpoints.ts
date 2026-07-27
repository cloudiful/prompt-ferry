import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'
import type {
  EndpointRequest,
  EndpointTestResponse,
  ModelEndpointRule,
  ModelRouteRequest,
  ModelRouteTestResponse,
  ProviderEndpoint,
} from '../generated/admin-api'
import { useLocale } from '../composables/useLocale'
import { createEndpointsWorkspaceView } from '../models/endpoints'
import {
  deleteEndpointById,
  deleteModelRouteById,
  fetchEndpointsPage,
  fetchModelRoutesPage,
  persistEndpoint,
  persistModelRoute,
  runEndpointTest,
  runModelRouteProbe,
  updateEndpointEnabled,
  updateModelRouteEnabled,
} from './endpoints-api'

export const useEndpointsStore = defineStore('endpoints', () => {
  const { t } = useLocale()
  const endpointState = {
    first: ref(0),
    items: ref<ProviderEndpoint[]>([]),
    rows: useStoredPageSize('endpoints', 10, STANDARD_PAGE_SIZE_OPTIONS),
    testResults: ref<Record<string, EndpointTestResponse>>({}),
    testingId: ref(''),
    togglingId: ref(''),
    total: ref(0),
  }
  const modelRouteState = {
    first: ref(0),
    items: ref<ModelEndpointRule[]>([]),
    rows: useStoredPageSize('model-routes', 10, STANDARD_PAGE_SIZE_OPTIONS),
    testResults: ref<Record<string, ModelRouteTestResponse>>({}),
    testingId: ref(''),
    togglingId: ref(''),
    total: ref(0),
  }
  const loading = ref(false)
  const selectedWorkspaceView = computed(() =>
    createEndpointsWorkspaceView({
      busy: loading.value,
      data: {
        endpoints: {
          items: endpointState.items.value,
          page: {
            first: endpointState.first.value,
            rows: endpointState.rows.value,
            total: endpointState.total.value,
          },
          testResults: endpointState.testResults.value,
        },
        modelRoutes: {
          items: modelRouteState.items.value,
          page: {
            first: modelRouteState.first.value,
            rows: modelRouteState.rows.value,
            total: modelRouteState.total.value,
          },
          testResults: modelRouteState.testResults.value,
        },
      },
      labels: {
        active: t('active'),
        disabled: t('disabled'),
        endpointTestIdle: t('endpointTestIdle'),
        endpointSourceAuto: t('endpointSourceAuto'),
        endpointSourceDetected: t('endpointSourceDetected'),
        endpointSourceManual: t('endpointSourceManual'),
        nativeApiAnthropicMessages: t('nativeApiAnthropicMessages'),
        nativeApiChat: t('nativeApiChat'),
        nativeApiResponses: t('nativeApiResponses'),
        nativeApiRealtime: t('nativeApiRealtime'),
        owner: t('owner'),
        routingStrategyClientKey: t('routingStrategyClientKey'),
        routingStrategySessionAffinity: t('routingStrategySessionAffinity'),
        scopeAdmin: t('scopeAdmin'),
        scopeUser: t('scopeUser'),
      },
      status: {
        endpoint: {
          testingId: endpointState.testingId.value,
          togglingId: endpointState.togglingId.value,
        },
        modelRoute: {
          testingId: modelRouteState.testingId.value,
          togglingId: modelRouteState.togglingId.value,
        },
      },
    }),
  )
  const endpointOptions = computed(
    () => selectedWorkspaceView.value.endpoint_options,
  )

  async function loadEndpoints(
    first = endpointState.first.value,
    rows = endpointState.rows.value,
  ): Promise<void> {
    endpointState.first.value = first
    endpointState.rows.value = rows
    const page = await fetchEndpointsPage(
      endpointState.first.value,
      endpointState.rows.value,
    )
    endpointState.items.value = page.endpoints
    endpointState.total.value = page.total
    endpointState.first.value = page.first
    endpointState.rows.value = page.rows
    if (
      page.endpoints.length === 0 &&
      page.total > 0 &&
      page.first >= page.total
    ) {
      const previousFirst =
        Math.floor((page.total - 1) / page.rows) * page.rows
      await loadEndpoints(previousFirst, page.rows)
    }
  }

  async function loadModelRoutes(
    first = modelRouteState.first.value,
    rows = modelRouteState.rows.value,
  ): Promise<void> {
    modelRouteState.first.value = first
    modelRouteState.rows.value = rows
    const page = await fetchModelRoutesPage(
      modelRouteState.first.value,
      modelRouteState.rows.value,
    )
    modelRouteState.items.value = page.routes
    modelRouteState.total.value = page.total
    modelRouteState.first.value = page.first
    modelRouteState.rows.value = page.rows
    if (
      page.routes.length === 0 &&
      page.total > 0 &&
      page.first >= page.total
    ) {
      const previousFirst =
        Math.floor((page.total - 1) / page.rows) * page.rows
      await loadModelRoutes(previousFirst, page.rows)
    }
  }

  async function reloadWorkspace(): Promise<void> {
    await Promise.all([loadEndpoints(), loadModelRoutes()])
  }

  async function reloadModelRouting(): Promise<void> {
    await loadModelRoutes()
  }

  async function refresh(): Promise<void> {
    loading.value = true
    try {
      await reloadWorkspace()
    } finally {
      loading.value = false
    }
  }

  async function saveEndpoint(
    endpointId: string | null,
    body: EndpointRequest,
  ): Promise<ProviderEndpoint> {
    const saved = await persistEndpoint(endpointId, body)
    await reloadWorkspace()
    return saved
  }

  async function removeEndpoint(endpointId: string): Promise<void> {
    await deleteEndpointById(endpointId)
    await reloadWorkspace()
  }

  async function runEndpointTest(
    endpointId: string,
  ): Promise<EndpointTestResponse> {
    endpointState.testingId.value = endpointId
    try {
      const result = await runEndpointTest(endpointId)
      endpointState.testResults.value[endpointId] = result
      return result
    } finally {
      endpointState.testingId.value = ''
    }
  }

  async function toggleEndpointEnabled(
    endpointId: string,
    enabled: boolean,
  ): Promise<ProviderEndpoint> {
    endpointState.togglingId.value = endpointId
    try {
      const endpoint = endpointState.items.value.find(
        (item) => item.endpoint_id === endpointId,
      )
      if (!endpoint) {
        throw new Error(`Endpoint not found: ${endpointId}`)
      }
      const saved = await updateEndpointEnabled(endpoint, enabled)
      endpointState.items.value = endpointState.items.value.map((endpoint) =>
        endpoint.endpoint_id === endpointId ? saved : endpoint,
      )
      return saved
    } finally {
      endpointState.togglingId.value = ''
    }
  }

  async function saveModelRoute(
    ruleId: string | null,
    body: ModelRouteRequest,
  ): Promise<ModelEndpointRule> {
    const saved = await persistModelRoute(ruleId, body)
    await reloadModelRouting()
    return saved
  }

  async function removeModelRoute(ruleId: string): Promise<void> {
    await deleteModelRouteById(ruleId)
    await reloadModelRouting()
  }

  async function runModelRouteTest(
    ruleId: string,
  ): Promise<ModelRouteTestResponse> {
    modelRouteState.testingId.value = ruleId
    try {
      const result = await runModelRouteProbe(ruleId)
      modelRouteState.testResults.value[ruleId] = result
      return result
    } finally {
      modelRouteState.testingId.value = ''
    }
  }

  async function toggleModelRouteEnabled(
    ruleId: string,
    enabled: boolean,
  ): Promise<ModelEndpointRule> {
    modelRouteState.togglingId.value = ruleId
    try {
      const route = modelRouteState.items.value.find(
        (item) => item.rule_id === ruleId,
      )
      if (!route) {
        throw new Error(`Model route not found: ${ruleId}`)
      }
      const saved = await updateModelRouteEnabled(route, enabled)
      modelRouteState.items.value = modelRouteState.items.value.map((route) =>
        route.rule_id === ruleId ? saved : route,
      )
      return saved
    } finally {
      modelRouteState.togglingId.value = ''
    }
  }

  function findEndpointById(endpointId: string): ProviderEndpoint | null {
    return (
      endpointState.items.value.find(
        (item) => item.endpoint_id === endpointId,
      ) ?? null
    )
  }

  function findModelRouteById(ruleId: string): ModelEndpointRule | null {
    return (
      modelRouteState.items.value.find((item) => item.rule_id === ruleId) ??
      null
    )
  }

  return {
    endpointFirst: endpointState.first,
    endpointOptions,
    endpointRows: endpointState.rows,
    endpointTestResults: endpointState.testResults,
    endpointTotal: endpointState.total,
    endpoints: endpointState.items,
    findEndpointById,
    findModelRouteById,
    loadEndpoints,
    loadModelRoutes,
    loading,
    modelRouteFirst: modelRouteState.first,
    modelRouteRows: modelRouteState.rows,
    modelRouteTestResults: modelRouteState.testResults,
    modelRouteTotal: modelRouteState.total,
    modelRoutes: modelRouteState.items,
    refresh,
    removeEndpoint,
    removeModelRoute,
    runEndpointTest,
    runModelRouteTest,
    saveEndpoint,
    saveModelRoute,
    selectedWorkspaceView,
    testingEndpointId: endpointState.testingId,
    testingModelRouteId: modelRouteState.testingId,
    toggleEndpointEnabled,
    toggleModelRouteEnabled,
    togglingEndpointId: endpointState.togglingId,
    togglingModelRouteId: modelRouteState.togglingId,
  }
})
