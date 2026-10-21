import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'
import type {
  EndpointTestResponse,
  ModelEndpointRule,
  ModelRouteTestResponse,
  ProviderEndpoint,
} from '../generated/admin-api'
import { useLocale } from '../composables/useLocale'
import { useAsyncAction } from '../composables/useAsyncAction'
import { createEndpointsWorkspaceView } from '../models/endpoints'
import { fetchEndpointsPage, fetchModelRoutesPage } from './endpoints-api'
import {
  ENDPOINT_KEY_PREFIX,
  MODEL_ROUTE_KEY_PREFIX,
  createEndpointActions,
  pendingRowsById,
} from './endpoint-actions'

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
        endpointTestIdle: t('endpointTestIdle'),
        endpointSourceAuto: t('endpointSourceAuto'),
        endpointSourceDetected: t('endpointSourceDetected'),
        endpointSourceManual: t('endpointSourceManual'),
        modelRouteTestNoneEligible: t('modelRouteTestNoneEligible'),
        modelRouteTestPassed: (passed: number, total: number) =>
          t('modelRouteTestPassed', { passed, total }),
        nativeApiAnthropicMessages: t('nativeApiAnthropicMessages'),
        nativeApiChat: t('nativeApiChat'),
        nativeApiResponses: t('nativeApiResponses'),
        nativeApiRealtime: t('nativeApiRealtime'),
        owner: t('owner'),
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
      const previousFirst = Math.floor((page.total - 1) / page.rows) * page.rows
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
      const previousFirst = Math.floor((page.total - 1) / page.rows) * page.rows
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

  // The guard lives here rather than at each call site, so a record can be
  // gated against every operation that would write it, not just the one the
  // caller happened to remember.
  const guard = useAsyncAction()
  const actions = createEndpointActions({
    endpoints: {
      items: endpointState.items,
      testResults: endpointState.testResults,
      testingId: endpointState.testingId,
      togglingId: endpointState.togglingId,
    },
    guard,
    modelRoutes: {
      items: modelRouteState.items,
      testResults: modelRouteState.testResults,
      testingId: modelRouteState.testingId,
      togglingId: modelRouteState.togglingId,
    },
    reloadModelRouting,
    reloadWorkspace,
  })

  // Per-row view of the guard: which records are gated, and which action holds
  // each one. `loading` stays the fetch-only busy flag, so a write never blanks
  // the whole page.
  const endpointPendingActions = computed(() =>
    pendingRowsById(guard.pendingActions.value, ENDPOINT_KEY_PREFIX),
  )
  const modelRoutePendingActions = computed(() =>
    pendingRowsById(guard.pendingActions.value, MODEL_ROUTE_KEY_PREFIX),
  )

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
    ...actions,
    endpointFirst: endpointState.first,
    endpointOptions,
    endpointPendingActions,
    endpointRows: endpointState.rows,
    endpointTestResults: endpointState.testResults,
    endpointTotal: endpointState.total,
    endpoints: endpointState.items,
    findEndpointById,
    findModelRouteById,
    isPending: guard.isPending,
    loadEndpoints,
    loadModelRoutes,
    loading,
    modelRouteFirst: modelRouteState.first,
    modelRoutePendingActions,
    modelRouteRows: modelRouteState.rows,
    modelRouteTestResults: modelRouteState.testResults,
    modelRouteTotal: modelRouteState.total,
    modelRoutes: modelRouteState.items,
    pendingActions: guard.pendingActions,
    refresh,
    selectedWorkspaceView,
    testingEndpointId: endpointState.testingId,
    testingModelRouteId: modelRouteState.testingId,
    togglingEndpointId: endpointState.togglingId,
    togglingModelRouteId: modelRouteState.togglingId,
  }
})
