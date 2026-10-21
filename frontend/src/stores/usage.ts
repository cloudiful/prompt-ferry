import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  REQUEST_RECORD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'
import type {
  RequestRecordCategory,
  RequestRecordFacets,
  RequestRecordOverviewRange,
} from '../generated/admin-api'
import { useLocale } from '../composables/useLocale'
import type { RequestRecordFilterModel, RequestRecordRowView } from '../models'
import {
  createUsageFacetOptionsView,
  createUsageStateOptions,
  createUsageWorkspaceView,
  DEFAULT_REQUEST_RECORD_MCP_SCOPE,
  resolveUsageRangeWindow,
  type RequestRecordMcpScope,
} from '../models/usage'
import {
  DEFAULT_REQUEST_OVERVIEW_PERSPECTIVE,
  type RequestOverviewDrilldown,
  type RequestOverviewPerspective,
} from '../request-overview'
import type { RequestRecordOverviewResponse } from '../generated/admin-api'
import { createRequestRecordDetailState } from './request-record-detail'
import { createDefaultRequestRecordFilters } from './request-records-query'
import {
  fetchUsageFacets,
  fetchUsageOverview,
  fetchUsageRecords,
} from './usage-api'
import { useAsyncAction } from '@/composables/useAsyncAction'
import { createRequestRecordsActions } from './request-records-actions'
import { createUsageActions, type UsageOverviewRequest } from './usage-actions'

export const useRequestRecordsStore = defineStore('request-records', () => {
  const { t } = useLocale()
  const loadingState = {
    overview: ref(false),
    page: ref(false),
    records: ref(false),
  }
  const overviewState = {
    overview: ref<RequestRecordOverviewResponse | null>(null),
  }
  const queryState = {
    aiPerspective: ref<RequestOverviewPerspective>(
      DEFAULT_REQUEST_OVERVIEW_PERSPECTIVE,
    ),
    end: ref(''),
    filters: ref<RequestRecordFilterModel>(createDefaultRequestRecordFilters()),
    first: ref(0),
    mcpScope: ref<RequestRecordMcpScope>(DEFAULT_REQUEST_RECORD_MCP_SCOPE),
    requestCategory: ref<RequestRecordCategory>('ai'),
    rowsPerPage: useStoredPageSize(
      'request-records',
      25,
      REQUEST_RECORD_PAGE_SIZE_OPTIONS,
    ),
    sortField: ref('created_at'),
    sortOrder: ref<-1 | 0 | 1>(-1),
    start: ref(''),
    total: ref(0),
    range: ref<RequestRecordOverviewRange>('24h'),
  }
  const recordState = {
    facets: ref<RequestRecordFacets>({ dates: [], users: [], models: [] }),
    rows: ref<RequestRecordRowView[]>([]),
  }
  const detailState = createRequestRecordDetailState()
  const actionGuard = useAsyncAction()
  // Mirrors the overview's latest-request rule: only the newest query may
  // commit, clear the pending flag, or surface an error for its resource.
  let pageLoadVersion = 0
  let recordsQueryVersion = 0
  let facetsQueryVersion = 0

  const requestStateOptions = computed(() =>
    createUsageStateOptions(
      {
        received: t('requestStateReceived'),
        awaiting_approval: t('requestStateAwaitingApproval'),
        upstream_processing: t('requestStateUpstreamProcessing'),
        completed: t('requestStateCompleted'),
        failed: t('requestStateFailed'),
        aborted: t('requestStateAborted'),
      },
      recordState.facets.value.states ?? [],
    ),
  )
  const facetOptionsView = computed(() =>
    createUsageFacetOptionsView(
      recordState.facets.value,
      requestStateOptions.value,
    ),
  )
  const usageWorkspaceView = computed(() =>
    createUsageWorkspaceView({
      busy: loadingState.page.value,
      facets: facetOptionsView.value,
      mcpScope: queryState.mcpScope.value,
      detail: {
        affinityResetting: detailState.affinityResetting.value,
        conversationOverride: detailState.conversationOverride.value,
        detailLoading: detailState.detailLoading.value,
        detailRecord: detailState.detailRecord.value,
        overrideSaving: detailState.overrideSaving.value,
        requestFull: detailState.requestFull.value,
        requestFullLoading: detailState.requestFullLoading.value,
        requestFullLoadingMore: detailState.requestFullLoadingMore.value,
        routeOptionsLoading: detailState.routeOptionsLoading.value,
        sessionRouteOptions: detailState.sessionRouteOptions.value,
      },
      records: {
        first: queryState.first.value,
        items: recordState.rows.value,
        loading: loadingState.records.value,
        rowsPerPage: queryState.rowsPerPage.value,
        sortField: queryState.sortField.value,
        sortOrder: queryState.sortOrder.value,
        total: queryState.total.value,
      },
    }),
  )

  async function refreshPage(): Promise<void> {
    const pageVersion = ++pageLoadVersion
    loadingState.page.value = true
    try {
      await Promise.all([refreshRecords(), refreshFacets()])
    } finally {
      if (pageLoadVersion === pageVersion) loadingState.page.value = false
    }
  }

  async function refreshAll(): Promise<void> {
    const pageVersion = ++pageLoadVersion
    loadingState.page.value = true
    try {
      await Promise.all([refreshOverview(), refreshRecords(), refreshFacets()])
    } finally {
      if (pageLoadVersion === pageVersion) loadingState.page.value = false
    }
  }

  function createOverviewRequest(
    perspective: RequestOverviewPerspective,
  ): UsageOverviewRequest {
    return {
      requestCategory: queryState.requestCategory.value,
      perspective,
      range: queryState.range.value,
      start: queryState.start.value,
      end: queryState.end.value,
    }
  }

  const usageActions = createUsageActions({
    guard: actionGuard,
    currentPerspective: () => queryState.aiPerspective.value,
    createOverviewRequest,
    fetchOverview: (request: UsageOverviewRequest) =>
      fetchUsageOverview({ ...request, mcpScope: queryState.mcpScope.value }),
    setOverview: (overview) => {
      overviewState.overview.value = overview
    },
    setOverviewLoading: (loading) => {
      loadingState.overview.value = loading
    },
    setPerspective: (perspective) => {
      queryState.aiPerspective.value = perspective
    },
  })
  const { refreshOverview } = usageActions

  async function refreshRecords(
    nextFirst = queryState.first.value,
    nextRows = queryState.rowsPerPage.value,
  ): Promise<void> {
    queryState.first.value = nextFirst
    queryState.rowsPerPage.value = nextRows
    const queryVersion = ++recordsQueryVersion
    // The payload is captured here so a scope, category, filter or page change
    // during the request cannot retroactively change what this query asked for.
    const query = {
      filters: queryState.filters.value,
      first: queryState.first.value,
      rows: queryState.rowsPerPage.value,
      range: queryState.range.value,
      requestCategory: queryState.requestCategory.value,
      sortField: queryState.sortField.value,
      sortOrder: queryState.sortOrder.value,
      start: queryState.start.value,
      end: queryState.end.value,
      mcpScope: queryState.mcpScope.value,
    }
    loadingState.records.value = true
    let correction: { first: number; rows: number } | null = null
    try {
      const page = await fetchUsageRecords(query)
      if (recordsQueryVersion !== queryVersion) return
      recordState.rows.value = page.rows
      queryState.total.value = page.total
      queryState.first.value = page.first
      queryState.rowsPerPage.value = page.rowsPerPage
      if (
        page.rows.length === 0 &&
        page.total > 0 &&
        page.first >= page.total
      ) {
        correction = {
          first:
            Math.floor((page.total - 1) / page.rowsPerPage) * page.rowsPerPage,
          rows: page.rowsPerPage,
        }
      }
    } catch (cause) {
      // A superseded query reports nothing: its rows, total and error must not
      // replace the newer query's state.
      if (recordsQueryVersion === queryVersion) throw cause
      return
    } finally {
      if (recordsQueryVersion === queryVersion) {
        loadingState.records.value = false
      }
    }
    // The page correction is a fresh query, so a newer query that started in
    // the meantime wins and the retry reports its own failure.
    if (correction && recordsQueryVersion === queryVersion) {
      await refreshRecords(correction.first, correction.rows)
    }
  }

  async function refreshFacets(): Promise<void> {
    const queryVersion = ++facetsQueryVersion
    const window = resolveUsageRangeWindow(
      queryState.range.value,
      queryState.start.value,
      queryState.end.value,
    )
    const category = queryState.requestCategory.value
    const scope = queryState.mcpScope.value
    try {
      const facets = await fetchUsageFacets(
        category,
        window.start,
        window.end,
        scope,
      )
      if (facetsQueryVersion !== queryVersion) return
      recordState.facets.value = facets
    } catch (cause) {
      // Superseded facet options are dropped instead of replacing the newer
      // query's options or reporting a stale error.
      if (facetsQueryVersion === queryVersion) throw cause
    }
  }

  function setRequestCategory(nextCategory: RequestRecordCategory): void {
    if (queryState.requestCategory.value === nextCategory) return
    queryState.requestCategory.value = nextCategory
    queryState.mcpScope.value = DEFAULT_REQUEST_RECORD_MCP_SCOPE
    queryState.first.value = 0
    queryState.filters.value = createDefaultRequestRecordFilters()
    detailState.resetDetail()
  }

  /**
   * Drop the filters that cannot match the selected MCP scope.
   *
   * A gateway record carries no MCP server identity, so a filter that names a
   * service would hide every gateway row. The AI category keeps its filters:
   * the scope only applies to the MCP records.
   */
  function clearIncompatibleServiceFilters(scope: RequestRecordMcpScope): void {
    if (scope !== 'gateway') return
    if (queryState.requestCategory.value !== 'mcp') return
    queryState.filters.value.mcp_server_id!.value = null
    queryState.filters.value.mcp_bearer_token_slot!.value = null
    queryState.filters.value.model_key.value = null
  }

  async function setMcpScope(nextScope: RequestRecordMcpScope): Promise<void> {
    if (queryState.mcpScope.value === nextScope) return
    queryState.mcpScope.value = nextScope
    queryState.first.value = 0
    clearIncompatibleServiceFilters(nextScope)
    await refreshAll()
  }

  function applyDrilldown(filter: RequestOverviewDrilldown): void {
    queryState.filters.value.global.value = null
    queryState.filters.value.endpoint_id!.value = filter.endpoint_id ?? null
    queryState.filters.value.mcp_server_id!.value = filter.mcp_server_id ?? null
    queryState.filters.value.mcp_bearer_token_slot!.value =
      filter.mcp_bearer_token_slot ?? null
    queryState.filters.value.model_key.value = filter.model ?? null
    queryState.first.value = 0
    const nextScope = filter.mcp_scope
    if (!nextScope) return
    if (nextScope === 'gateway') {
      clearIncompatibleServiceFilters(nextScope)
    }
    if (nextScope === queryState.mcpScope.value) return
    queryState.mcpScope.value = nextScope
    // The drilldown opens the records view, so the overview is refreshed in
    // the background: it must not keep serving another scope. A failed
    // refresh keeps the previous overview, and the next explicit refresh
    // reports the error.
    void refreshOverview().catch(() => undefined)
  }

  const recordActions = createRequestRecordsActions({
    guard: actionGuard,
    refreshAll,
  })
  return {
    ...recordActions,
    ...usageActions,
    aiPerspective: queryState.aiPerspective,
    affinityResetting: detailState.affinityResetting,
    clearConversationOverride: detailState.clearConversationOverride,
    conversationAction: detailState.conversationAction,
    conversationOverride: detailState.conversationOverride,
    detailLoading: detailState.detailLoading,
    detailRecord: detailState.detailRecord,
    end: queryState.end,
    guardedResetSessionAffinity: detailState.guardedResetSessionAffinity,
    facetOptionsView,
    facets: recordState.facets,
    filters: queryState.filters,
    first: queryState.first,
    loadDetail: detailState.loadDetail,
    loadMoreRequestFull: detailState.loadMoreRequestFull,
    loadRequestFull: detailState.loadRequestFull,
    loadSessionRouteOptions: detailState.loadSessionRouteOptions,
    loading: loadingState.page,
    mcpScope: queryState.mcpScope,
    overview: overviewState.overview,
    overviewLoading: loadingState.overview,
    overrideSaving: detailState.overrideSaving,
    range: queryState.range,
    recordsLoading: loadingState.records,
    refreshAll,
    refreshFacets,
    refreshOverview,
    refreshPage,
    refreshRecords,
    requestCategory: queryState.requestCategory,
    requestFull: detailState.requestFull,
    requestFullLoading: detailState.requestFullLoading,
    requestFullLoadingMore: detailState.requestFullLoadingMore,
    requestStateOptions,
    resetDetail: detailState.resetDetail,
    resetSessionAffinity: detailState.resetSessionAffinity,
    applyDrilldown,
    routeOptionsLoading: detailState.routeOptionsLoading,
    rows: recordState.rows,
    rowsPerPage: queryState.rowsPerPage,
    saveConversationOverride: detailState.saveConversationOverride,
    sessionRouteOptions: detailState.sessionRouteOptions,
    setMcpScope,
    setRequestCategory,
    sortField: queryState.sortField,
    sortOrder: queryState.sortOrder,
    start: queryState.start,
    total: queryState.total,
    usageWorkspaceView,
  }
})
