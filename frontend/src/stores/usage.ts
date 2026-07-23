import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  REQUEST_RECORD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'
import type {
  RequestRecordCategory,
  RequestRecordBucket,
  RequestRecordFacets,
  RequestRecordSummary,
  RequestRecordsClearResponse,
} from '../generated/admin-api'
import { useLocale } from '../composables/useLocale'
import type {
  Option,
  RequestRecordBucketGranularity,
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
  fetchUsageSeries,
  fetchUsageSummary,
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
    series: ref<RequestRecordBucket[]>([]),
    summary: ref<RequestRecordSummary | null>(null),
  }
  const queryState = {
    bucket: ref<RequestRecordBucketGranularity>('hour'),
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

  const rangeOptions = computed<Option[]>(() => [
    { label: '24h', value: '24h' },
    { label: '7d', value: '7d' },
    { label: '30d', value: '30d' },
    { label: 'custom', value: 'custom' },
  ])

  const bucketOptions = computed<Option<RequestRecordBucketGranularity>[]>(
    () => [
      { label: 'minute', value: 'minute' },
      { label: 'hour', value: 'hour' },
      { label: 'day', value: 'day' },
    ],
  )
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
      await Promise.all([
        refreshOverview(),
        refreshRecords(),
        refreshFacets(),
      ])
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

  async function refreshSummary(): Promise<void> {
    overviewState.summary.value = await fetchUsageSummary(
      queryState.range.value,
    )
  }

  async function refreshSeries(): Promise<void> {
    overviewState.series.value = await fetchUsageSeries({
      bucket: queryState.bucket.value,
      range: queryState.range.value,
      start: queryState.start.value,
      end: queryState.end.value,
    })
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
    bucket: queryState.bucket,
    bucketOptions,
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
    rangeOptions,
    recordsLoading: loadingState.records,
    refreshAll,
    refreshOverview,
    refreshPage,
    refreshRecords,
    refreshSeries,
    refreshSummary,
    requestCategory: queryState.requestCategory,
    requestFull: detailState.requestFull,
    requestFullLoading: detailState.requestFullLoading,
    requestStateOptions,
    resetDetail: detailState.resetDetail,
    applyDrilldown,
    routeOptionsLoading: detailState.routeOptionsLoading,
    rows: recordState.rows,
    rowsPerPage: queryState.rowsPerPage,
    saveConversationOverride: detailState.saveConversationOverride,
    series: overviewState.series,
    sessionRouteOptions: detailState.sessionRouteOptions,
    setRequestCategory,
    sortField: queryState.sortField,
    sortOrder: queryState.sortOrder,
    start: queryState.start,
    summary: overviewState.summary,
    total: queryState.total,
    usageWorkspaceView,
  }
})
