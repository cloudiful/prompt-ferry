import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  REQUEST_RECORD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'
import type {
  RequestRecordCategory,
  RequestRecordFacets,
  RequestRecordsClearResponse,
} from '../generated/admin-api'
import { useLocale } from '../composables/useLocale'
import type {
  RequestRecordClearForm,
  RequestRecordFilterModel,
  RequestRecordRowView,
} from '../models'
import {
  createUsageFacetOptionsView,
  createUsageStateOptions,
  createUsageWorkspaceView,
} from '../models/usage'
import type {
  RequestOverviewDrilldown,
  RequestOverviewResponse,
} from '../request-overview'
import { createRequestRecordDetailState } from './request-record-detail'
import { createDefaultRequestRecordFilters } from './request-records-query'
import {
  clearUsageHistory,
  fetchUsageFacets,
  fetchUsageOverview,
  fetchUsageRecords,
  pruneUsageHistory,
} from './usage-api'

export const useRequestRecordsStore = defineStore('request-records', () => {
  const { t } = useLocale()
  const loadingState = {
    overview: ref(false),
    page: ref(false),
    records: ref(false),
  }
  const overviewState = {
    overview: ref<RequestOverviewResponse | null>(null),
  }
  const queryState = {
    end: ref(''),
    filters: ref<RequestRecordFilterModel>(createDefaultRequestRecordFilters()),
    first: ref(0),
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
    range: ref('24h'),
  }
  const recordState = {
    facets: ref<RequestRecordFacets>({ dates: [], users: [], models: [] }),
    rows: ref<RequestRecordRowView[]>([]),
  }
  const detailState = createRequestRecordDetailState()

  const requestStateOptions = computed(() =>
    createUsageStateOptions({
      received: t('requestStateReceived'),
      awaiting_approval: t('requestStateAwaitingApproval'),
      upstream_processing: t('requestStateUpstreamProcessing'),
      completed: t('requestStateCompleted'),
      failed: t('requestStateFailed'),
      aborted: t('requestStateAborted'),
    }),
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
      detail: {
        conversationOverride: detailState.conversationOverride.value,
        detailLoading: detailState.detailLoading.value,
        detailRecord: detailState.detailRecord.value,
        overrideSaving: detailState.overrideSaving.value,
        requestFull: detailState.requestFull.value,
        requestFullLoading: detailState.requestFullLoading.value,
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
    loadingState.page.value = true
    try {
      await Promise.all([refreshRecords(), refreshFacets()])
    } finally {
      loadingState.page.value = false
    }
  }

  async function refreshAll(): Promise<void> {
    loadingState.page.value = true
    try {
      await Promise.all([refreshOverview(), refreshRecords(), refreshFacets()])
    } finally {
      loadingState.page.value = false
    }
  }

  async function refreshOverview(): Promise<void> {
    loadingState.overview.value = true
    try {
      overviewState.overview.value = await fetchUsageOverview({
        requestCategory: queryState.requestCategory.value,
        range: queryState.range.value as '24h' | '7d' | '30d' | 'custom',
        start: queryState.start.value,
        end: queryState.end.value,
      })
    } finally {
      loadingState.overview.value = false
    }
  }

  async function refreshRecords(
    nextFirst = queryState.first.value,
    nextRows = queryState.rowsPerPage.value,
  ): Promise<void> {
    queryState.first.value = nextFirst
    queryState.rowsPerPage.value = nextRows
    loadingState.records.value = true
    try {
      const page = await fetchUsageRecords({
        filters: queryState.filters.value,
        first: queryState.first.value,
        rows: queryState.rowsPerPage.value,
        requestCategory: queryState.requestCategory.value,
        sortField: queryState.sortField.value,
        sortOrder: queryState.sortOrder.value,
      })
      recordState.rows.value = page.rows
      queryState.total.value = page.total
      queryState.first.value = page.first
      queryState.rowsPerPage.value = page.rowsPerPage
      if (
        page.rows.length === 0 &&
        page.total > 0 &&
        page.first >= page.total
      ) {
        const previousFirst =
          Math.floor((page.total - 1) / page.rowsPerPage) * page.rowsPerPage
        await refreshRecords(previousFirst, page.rowsPerPage)
      }
    } finally {
      loadingState.records.value = false
    }
  }

  async function refreshFacets(): Promise<void> {
    recordState.facets.value = await fetchUsageFacets(
      queryState.requestCategory.value,
    )
  }

  function setRequestCategory(nextCategory: RequestRecordCategory): void {
    if (queryState.requestCategory.value === nextCategory) return
    queryState.requestCategory.value = nextCategory
    queryState.first.value = 0
    queryState.filters.value = createDefaultRequestRecordFilters()
    detailState.resetDetail()
  }

  function applyDrilldown(filter: RequestOverviewDrilldown): void {
    queryState.filters.value.global.value = null
    queryState.filters.value.endpoint_id!.value = filter.endpoint_id ?? null
    queryState.filters.value.mcp_server_id!.value = filter.mcp_server_id ?? null
    queryState.filters.value.mcp_bearer_token_slot!.value =
      filter.mcp_bearer_token_slot ?? null
    queryState.filters.value.model_key.value = filter.model ?? null
    queryState.first.value = 0
  }

  async function clearHistory(
    form: RequestRecordClearForm,
  ): Promise<RequestRecordsClearResponse> {
    const result = await clearUsageHistory(form)
    await refreshAll()
    return result
  }

  async function pruneHistory(): Promise<number> {
    const deleted = await pruneUsageHistory()
    await refreshAll()
    return deleted
  }

  return {
    clearConversationOverride: detailState.clearConversationOverride,
    clearHistory,
    conversationOverride: detailState.conversationOverride,
    detailLoading: detailState.detailLoading,
    detailRecord: detailState.detailRecord,
    end: queryState.end,
    facetOptionsView,
    facets: recordState.facets,
    filters: queryState.filters,
    first: queryState.first,
    loadDetail: detailState.loadDetail,
    loadRequestFull: detailState.loadRequestFull,
    loadSessionRouteOptions: detailState.loadSessionRouteOptions,
    loading: loadingState.page,
    overview: overviewState.overview,
    overviewLoading: loadingState.overview,
    overrideSaving: detailState.overrideSaving,
    pruneHistory,
    range: queryState.range,
    recordsLoading: loadingState.records,
    refreshAll,
    refreshOverview,
    refreshPage,
    refreshRecords,
    requestCategory: queryState.requestCategory,
    requestFull: detailState.requestFull,
    requestFullLoading: detailState.requestFullLoading,
    requestStateOptions,
    resetDetail: detailState.resetDetail,
    resetSessionAffinity: detailState.resetSessionAffinity,
    applyDrilldown,
    routeOptionsLoading: detailState.routeOptionsLoading,
    rows: recordState.rows,
    rowsPerPage: queryState.rowsPerPage,
    saveConversationOverride: detailState.saveConversationOverride,
    sessionRouteOptions: detailState.sessionRouteOptions,
    setRequestCategory,
    sortField: queryState.sortField,
    sortOrder: queryState.sortOrder,
    start: queryState.start,
    total: queryState.total,
    usageWorkspaceView,
  }
})
