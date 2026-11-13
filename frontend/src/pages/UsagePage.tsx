import { useEffect, useMemo, useRef, useState } from 'react'
import { HeaderActions } from '@/components/layout/HeaderActions'
import {
  keepPreviousData,
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'
import { ChevronDown, RefreshCw, SlidersHorizontal, Trash2 } from 'lucide-react'
import { useLocation } from 'react-router'
import type {
  RequestRecordFullResponse,
  RequestRecordListRow,
} from '@/generated/admin-api'
import { requestRecordFull } from '@/generated/admin-api'
import {
  clearRequestRecordsMutation,
  deleteConversationEndpointOverrideMutation,
  listRequestRecordsOptions,
  listRequestRecordsQueryKey,
  listUserOptionsOptions,
  requestRecordDetailOptions,
  requestRecordDetailQueryKey,
  requestRecordFacetsOptions,
  requestRecordFacetsQueryKey,
  requestRecordFullQueryKey,
  requestRecordOverviewOptions,
  requestRecordOverviewQueryKey,
  requestRecordResetSessionAffinityMutation,
  requestRecordSessionRouteOptionsOptions,
  requestRecordSessionRouteOptionsQueryKey,
  setConversationEndpointOverrideMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { RequestRecordsOverviewPanel } from '@/components/usage/RequestRecordsOverviewPanel'
import { RequestRecordsTable } from '@/components/usage/RequestRecordsTable'
import { RequestRecordsToolbar } from '@/components/usage/RequestRecordsToolbar'
import { McpUsageDetailDialog } from '@/components/usage/McpUsageDetailDialog'
import { UsageClearDialog } from '@/components/usage/UsageClearDialog'
import { UsageDetailDialog } from '@/components/usage/UsageDetailDialog'
import { UsageModeSwitch } from '@/components/usage/UsageModeSwitch'
import {
  UsageRangePicker,
  type UsageRangeApply,
} from '@/components/usage/UsageRangePicker'
import { useStoredPageSize } from '@/components/shared/TablePagination'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useLocale } from '@/lib/i18n'
import { cn } from '@/lib/utils'
import { useNotifier } from '@/lib/notifier'
import { usePageTitle } from '@/lib/router'
import { useSessionStore } from '@/lib/session'
import { useAsyncAction } from '@/lib/use-async-action'
import {
  refreshUsageFacets,
  refreshUsageOverview,
  refreshUsageRecords,
  refreshUsageUserOptions,
  type UsageDrilldown,
} from '@/lib/usage'
import {
  USAGE_MAINTENANCE_KEY,
  USAGE_REQUEST_FULL_ORDER,
  USAGE_REQUEST_FULL_PAGE_SIZE,
  usageConversationActionKey,
  usageDetailActionKey,
} from '@/lib/usage-detail'
import {
  createUsageDetailView,
  createUsageOverrideView,
  createUsageRouteOptionsView,
  mergeUsageRequestFull,
  usageAffinityNoticeKey,
  type UsageConversationOverrideView,
  type UsageDetailView,
  type UsageRouteOptionsView,
} from '@/lib/usage-detail-model'
import {
  usageClearPayload,
  usageClearToastMessage,
  type UsageClearForm,
} from '@/lib/usage-clear'
import {
  DEFAULT_USAGE_PERSPECTIVE,
  DEFAULT_USAGE_RANGE,
  DEFAULT_USAGE_ROWS,
  DEFAULT_USAGE_SCOPE,
  DEFAULT_USAGE_SORT_FIELD,
  DEFAULT_USAGE_SORT_ORDER,
  USAGE_PAGE_SIZE_KEY,
  USAGE_PAGE_SIZE_OPTIONS,
  USAGE_PERSPECTIVE_KEY,
  createDefaultUsageFilters,
  emptyUsageFacets,
  isUsageEmptyTail,
  loadUsageMode,
  nextUsageSortState,
  saveUsageMode,
  usageFacetsQuery,
  usageFiltersForScope,
  usageIdentityKey,
  usageLastPageFirst,
  usageListQuery,
  usageOverviewQuery,
  type UsageCategory,
  type UsageFilters,
  type UsageMode,
  type UsageModeStorage,
  type UsagePerspective,
  type UsageQueryState,
  type UsageScope,
} from '@/lib/usage-model'
import { slidingUsageWindow, type LocalUsageRange } from '@/lib/usage-range'

function categoryForPath(pathname: string): UsageCategory {
  return pathname.endsWith('/mcp') ? 'mcp' : 'ai'
}

function modeStorage(): UsageModeStorage | null {
  if (typeof window === 'undefined') return null
  try {
    return window.localStorage
  } catch {
    return null
  }
}

function countActiveFilters(
  filters: UsageFilters,
  scope: UsageScope,
  category: UsageCategory,
): number {
  let count = 0
  if (filters.search && filters.search.trim() !== '') count += 1
  if (filters.client_key_id !== null) count += 1
  if (filters.user !== null) count += 1
  if (filters.model !== null) count += 1
  if (filters.request_state !== null) count += 1
  if (filters.redaction_applied !== null) count += 1
  if (filters.endpoint_id !== null) count += 1
  if (filters.mcp_server_id !== null) count += 1
  if (filters.mcp_bearer_token_slot !== null) count += 1
  if (category === 'mcp' && scope !== 'service') count += 1
  return count
}

/**
 * The mounted AI/MCP request-record page.
 *
 * Both routes share this page and derive the category from the path. Server
 * reads are native queries keyed by the generated query keys; the remaining
 * store-free controls own only page, sort, filter, range, mode and dialog UI
 * state. The session epoch owns identity isolation.
 */
export function UsagePage() {
  const { t } = useLocale()
  const location = useLocation()
  const queryClient = useQueryClient()
  const { notifyApiError, notifySuccess } = useNotifier()
  const asyncAction = useAsyncAction()
  const me = useSessionStore((state) => state.me)
  const isAdmin = me?.is_admin === true
  const category = categoryForPath(location.pathname)

  usePageTitle(category === 'ai' ? 'aiRequests' : 'mcpCalls')

  const [mode, setMode] = useState<UsageMode>(() =>
    loadUsageMode(modeStorage(), category),
  )
  const [scope, setScope] = useState<UsageScope>(DEFAULT_USAGE_SCOPE)
  const [range, setRangeState] = useState<LocalUsageRange>(DEFAULT_USAGE_RANGE)
  const [start, setStart] = useState('')
  const [end, setEnd] = useState('')
  const [windowAnchor, setWindowAnchor] = useState(() => new Date())
  const [filters, setFilters] = useState<UsageFilters>(
    createDefaultUsageFilters,
  )
  const [first, setFirst] = useState(0)
  const [rows, setRows] = useStoredPageSize(
    USAGE_PAGE_SIZE_KEY,
    DEFAULT_USAGE_ROWS,
    USAGE_PAGE_SIZE_OPTIONS,
  )
  const [sortField, setSortField] = useState(DEFAULT_USAGE_SORT_FIELD)
  const [sortOrder, setSortOrder] = useState(DEFAULT_USAGE_SORT_ORDER)
  const [perspective, setPerspective] = useState<UsagePerspective>(
    DEFAULT_USAGE_PERSPECTIVE,
  )
  const [detailRecordId, setDetailRecordId] = useState<number | null>(null)
  const [detailOpen, setDetailOpen] = useState(false)
  const [transcriptWanted, setTranscriptWanted] = useState(false)
  const [filtersOpen, setFiltersOpen] = useState(false)
  const [clearOpen, setClearOpen] = useState(false)
  const identityRef = useRef(
    usageIdentityKey({
      user_id: me?.user_id ?? null,
      is_admin: isAdmin,
    }),
  )
  const categoryRef = useRef(category)
  const detailRecordIdRef = useRef(detailRecordId)
  const notifiedErrors = useRef(new WeakSet<object>())
  detailRecordIdRef.current = detailRecordId

  const identityKey = usageIdentityKey({
    user_id: me?.user_id ?? null,
    is_admin: isAdmin,
  })

  const queryState = useMemo<UsageQueryState>(
    () => ({
      category,
      scope,
      range: range === '1h' || range === '5h' ? 'custom' : range,
      start,
      end,
      first,
      rows,
      sortField,
      sortOrder,
      filters,
    }),
    [
      category,
      scope,
      range,
      start,
      end,
      first,
      rows,
      sortField,
      sortOrder,
      filters,
    ],
  )
  const listQuery = useMemo(() => usageListQuery(queryState), [queryState])
  const facetsQueryParams = useMemo(
    () => usageFacetsQuery(queryState, windowAnchor),
    [queryState, windowAnchor],
  )
  const overviewQueryParams = useMemo(
    () => usageOverviewQuery(queryState, perspective, windowAnchor),
    [queryState, perspective, windowAnchor],
  )

  const recordsQuery = useQuery({
    ...listRequestRecordsOptions({ query: listQuery }),
    enabled: me !== null,
    placeholderData: keepPreviousData,
  })
  const facetsQuery = useQuery({
    ...requestRecordFacetsOptions({ query: facetsQueryParams }),
    enabled: me !== null,
    placeholderData: keepPreviousData,
  })
  const overviewQuery = useQuery({
    ...requestRecordOverviewOptions({ query: overviewQueryParams }),
    enabled: me !== null,
    placeholderData: keepPreviousData,
  })
  const userOptionsQuery = useQuery({
    ...listUserOptionsOptions(),
    enabled: me !== null && isAdmin,
  })

  const records = recordsQuery.data?.records ?? []
  const total = recordsQuery.data?.total ?? 0
  const currentFirst = recordsQuery.data?.first ?? first
  const currentRows = recordsQuery.data?.rows ?? rows
  const facets = facetsQuery.data ?? emptyUsageFacets()
  const overview = overviewQuery.data ?? null
  const userOptions = userOptionsQuery.data?.users ?? []
  const recordsLoading = recordsQuery.isFetching
  const overviewLoading = overviewQuery.isFetching

  const detailQuery = useQuery({
    ...requestRecordDetailOptions({ path: { record_id: detailRecordId ?? 0 } }),
    enabled: detailOpen && detailRecordId !== null,
  })
  const detailCount = detailQuery.data?.conversation_id
  const routeOptionsQuery = useQuery({
    ...requestRecordSessionRouteOptionsOptions({
      path: { record_id: detailRecordId ?? 0 },
    }),
    enabled: detailOpen && detailRecordId !== null && Boolean(detailCount),
  })
  const transcriptQuery = useInfiniteQuery({
    queryKey: requestRecordFullQueryKey({
      path: { record_id: detailRecordId ?? 0 },
    }),
    queryFn: ({ pageParam, signal }) =>
      requestRecordFull({
        path: { record_id: detailRecordId ?? 0 },
        query: {
          limit: USAGE_REQUEST_FULL_PAGE_SIZE,
          order: USAGE_REQUEST_FULL_ORDER,
          ...(pageParam ? { cursor: pageParam } : {}),
        },
        signal,
        throwOnError: true,
      }),
    initialPageParam: null as string | null,
    getNextPageParam: (last) => last.next_cursor ?? null,
    enabled:
      detailOpen &&
      transcriptWanted &&
      detailRecordId !== null &&
      detailQuery.data?.has_full_request === true,
  })

  const detail: UsageDetailView | null = useMemo(
    () => (detailQuery.data ? createUsageDetailView(detailQuery.data) : null),
    [detailQuery.data],
  )
  const routeOptions: UsageRouteOptionsView | null = useMemo(
    () =>
      routeOptionsQuery.data
        ? createUsageRouteOptionsView(routeOptionsQuery.data)
        : null,
    [routeOptionsQuery.data],
  )
  const override: UsageConversationOverrideView | null = useMemo(
    () =>
      routeOptionsQuery.data
        ? createUsageOverrideView(routeOptionsQuery.data)
        : null,
    [routeOptionsQuery.data],
  )
  const requestFull: RequestRecordFullResponse | null = useMemo(() => {
    const pages = transcriptQuery.data?.pages
    if (!pages || pages.length === 0) return null
    return pages.reduce<RequestRecordFullResponse | null>(
      (merged, page) => (merged ? mergeUsageRequestFull(merged, page) : page),
      null,
    )
  }, [transcriptQuery.data])

  const saveOverrideMutation = useMutation(
    setConversationEndpointOverrideMutation(),
  )
  const clearOverrideMutation = useMutation(
    deleteConversationEndpointOverrideMutation(),
  )
  const resetAffinityMutation = useMutation(
    requestRecordResetSessionAffinityMutation(),
  )
  const clearMutation = useMutation(clearRequestRecordsMutation())

  function reportApiError(cause: unknown): void {
    if (cause && typeof cause === 'object') {
      if (notifiedErrors.current.has(cause)) return
      notifiedErrors.current.add(cause)
    }
    notifyApiError(cause)
  }

  useEffect(() => {
    if (!me) return
    const identityChanged = identityRef.current !== identityKey
    const categoryChanged = categoryRef.current !== category
    if (!identityChanged && !categoryChanged) return
    identityRef.current = identityKey
    categoryRef.current = category
    setScope(DEFAULT_USAGE_SCOPE)
    setFilters(createDefaultUsageFilters())
    setFirst(0)
    setMode(loadUsageMode(modeStorage(), category))
    setDetailOpen(false)
    setDetailRecordId(null)
    setTranscriptWanted(false)
    if (identityChanged) {
      setRangeState(DEFAULT_USAGE_RANGE)
      setStart('')
      setEnd('')
      setSortField(DEFAULT_USAGE_SORT_FIELD)
      setSortOrder(DEFAULT_USAGE_SORT_ORDER)
      setPerspective(DEFAULT_USAGE_PERSPECTIVE)
      setWindowAnchor(new Date())
      return
    }
    if (range === '1h' || range === '5h') {
      const window = slidingUsageWindow(range, Date.now())
      setStart(window.start)
      setEnd(window.end)
    }
    setWindowAnchor(new Date())
  }, [me, identityKey, category, range])

  useEffect(() => {
    const page = recordsQuery.data
    if (!page || recordsQuery.isPlaceholderData) return
    if (isUsageEmptyTail(page)) {
      const corrected = usageLastPageFirst(page.total, page.rows)
      if (corrected !== first) setFirst(corrected)
      return
    }
    if (page.first !== first) setFirst(page.first)
    if (page.rows !== rows) setRows(page.rows)
  }, [recordsQuery.data, recordsQuery.isPlaceholderData, first, rows, setRows])

  useEffect(() => {
    if (detailQuery.error) reportApiError(detailQuery.error)
  }, [detailQuery.error])
  useEffect(() => {
    if (transcriptQuery.error) reportApiError(transcriptQuery.error)
  }, [transcriptQuery.error])

  useEffect(() => {
    if (!detailOpen || !detailQuery.error) return
    setDetailOpen(false)
    setDetailRecordId(null)
  }, [detailOpen, detailQuery.error])

  useEffect(() => {
    return () => {
      void queryClient.cancelQueries({
        queryKey: listRequestRecordsQueryKey({ query: listQuery }),
        exact: true,
      })
    }
  }, [queryClient, listQuery])

  useEffect(() => {
    return () => {
      void queryClient.cancelQueries({
        queryKey: requestRecordFacetsQueryKey({ query: facetsQueryParams }),
        exact: true,
      })
    }
  }, [queryClient, facetsQueryParams])

  useEffect(() => {
    return () => {
      void queryClient.cancelQueries({
        queryKey: requestRecordOverviewQueryKey({ query: overviewQueryParams }),
        exact: true,
      })
    }
  }, [queryClient, overviewQueryParams])

  useEffect(() => {
    if (detailRecordId === null) return
    const recordId = detailRecordId
    return () => {
      void queryClient.cancelQueries({
        queryKey: requestRecordDetailQueryKey({
          path: { record_id: recordId },
        }),
      })
      void queryClient.cancelQueries({
        queryKey: requestRecordSessionRouteOptionsQueryKey({
          path: { record_id: recordId },
        }),
      })
      void queryClient.cancelQueries({
        queryKey: requestRecordFullQueryKey({
          path: { record_id: recordId },
        }),
      })
    }
  }, [queryClient, detailRecordId])

  const conversationId = detail?.conversation_id ?? null
  const detailActionKey =
    detailRecordId === null
      ? null
      : usageDetailActionKey(conversationId, detailRecordId)
  const detailPendingOrigin = detailActionKey
    ? (asyncAction.pendingOrigin(detailActionKey) ?? null)
    : null
  const detailPending = detailPendingOrigin !== null
  const activeFilterCount = countActiveFilters(filters, scope, category)

  async function refreshAll(): Promise<void> {
    const anchor = new Date()
    let nextStart = start
    let nextEnd = end
    if (range === '1h' || range === '5h') {
      const window = slidingUsageWindow(range, anchor.getTime())
      nextStart = window.start
      nextEnd = window.end
      setStart(nextStart)
      setEnd(nextEnd)
    }
    setWindowAnchor(anchor)
    const state: UsageQueryState = {
      ...queryState,
      range: range === '1h' || range === '5h' ? 'custom' : range,
      start: nextStart,
      end: nextEnd,
    }
    const page = await refreshUsageRecords(queryClient, usageListQuery(state))
    if (page.first !== first) setFirst(page.first)
    if (page.rows !== rows) setRows(page.rows)
    await Promise.all([
      refreshUsageFacets(queryClient, usageFacetsQuery(state, anchor)),
      refreshUsageOverview(
        queryClient,
        usageOverviewQuery(state, perspective, anchor),
      ),
      isAdmin ? refreshUsageUserOptions(queryClient) : Promise.resolve([]),
    ])
  }

  function refresh(): void {
    void refreshAll().catch((cause: unknown) => reportApiError(cause))
  }

  function changeMode(next: UsageMode): void {
    saveUsageMode(modeStorage(), category, next)
    setMode(next)
    if (range === '1h' || range === '5h') refresh()
  }

  function applyRange(input: UsageRangeApply): void {
    const nextRange = input.range
    let nextStart = input.start ?? ''
    let nextEnd = input.end ?? ''
    if (nextRange === '1h' || nextRange === '5h') {
      const window = slidingUsageWindow(nextRange, Date.now())
      nextStart = window.start
      nextEnd = window.end
    }
    setRangeState(nextRange)
    setStart(nextStart)
    setEnd(nextEnd)
    setWindowAnchor(new Date())
  }

  function changeFilters(patch: Partial<UsageFilters>): void {
    setFilters((current) => ({ ...current, ...patch }))
  }

  function changeSearch(search: string | null): void {
    changeFilters({ search })
  }

  function changeScope(next: UsageScope): void {
    if (scope === next) return
    setScope(next)
    setFirst(0)
    setFilters((current) => usageFiltersForScope(current, category, next))
  }

  function changeSort(field: string): void {
    const next = nextUsageSortState(
      { field: sortField, order: sortOrder },
      field,
    )
    setSortField(next.field)
    setSortOrder(next.order)
  }

  function changePage(nextFirst: number, nextRows: number): void {
    setRows(nextRows)
    setFirst(Math.max(0, nextFirst))
  }

  function changePerspective(next: UsagePerspective): void {
    if (perspective === next) return
    void (async () => {
      try {
        await asyncAction.run(
          USAGE_PERSPECTIVE_KEY,
          async () => {
            setPerspective(next)
            await refreshUsageOverview(
              queryClient,
              usageOverviewQuery(queryState, next, windowAnchor),
            )
          },
          'perspective',
        )
      } catch (cause) {
        reportApiError(cause)
      }
    })()
  }

  function drilldown(next: UsageDrilldown): void {
    const nextFilters = {
      ...filters,
      search: null,
      endpoint_id: next.endpoint_id ?? null,
      mcp_server_id: next.mcp_server_id ?? null,
      mcp_bearer_token_slot: next.mcp_bearer_token_slot ?? null,
      model: next.model ?? null,
    }
    const nextScope = next.mcp_scope ?? null
    setFilters(
      nextScope !== null && nextScope !== scope
        ? usageFiltersForScope(nextFilters, category, nextScope)
        : nextFilters,
    )
    setFirst(0)
    if (nextScope !== null) setScope(nextScope)
    setMode('records')
    saveUsageMode(modeStorage(), category, 'records')
  }

  function openDetail(record: RequestRecordListRow): void {
    setTranscriptWanted(false)
    setDetailRecordId(record.record_id)
    setDetailOpen(true)
  }

  function changeDetailOpen(open: boolean): void {
    if (!open && detailPending) return
    setDetailOpen(open)
    if (!open) {
      setDetailRecordId(null)
      setTranscriptWanted(false)
    }
  }

  async function runRoutingWrite(
    write: () => Promise<{ executed: boolean }>,
    successKey: 'overrideSaved' | 'overrideCleared',
  ): Promise<void> {
    try {
      const outcome = await write()
      if (outcome.executed) notifySuccess(t(successKey))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  function saveOverride(
    endpointId: string,
    endpointKeyId: string | null,
  ): void {
    if (!conversationId) return
    const recordId = detailRecordId
    const capturedEndpointId = endpointId
    const capturedEndpointKeyId = endpointKeyId
    void runRoutingWrite(
      () =>
        asyncAction.run(
          usageConversationActionKey(conversationId),
          async () => {
            await saveOverrideMutation.mutateAsync({
              path: { conversation_id: conversationId },
              body: {
                endpoint_id: capturedEndpointId,
                endpoint_key_id: capturedEndpointKeyId,
              },
            })
            if (recordId !== null) {
              await queryClient.invalidateQueries({
                queryKey: requestRecordSessionRouteOptionsQueryKey({
                  path: { record_id: recordId },
                }),
              })
            }
          },
          'save',
        ),
      'overrideSaved',
    )
  }

  function clearOverride(): void {
    if (!conversationId) return
    const recordId = detailRecordId
    void runRoutingWrite(
      () =>
        asyncAction.run(
          usageConversationActionKey(conversationId),
          async () => {
            await clearOverrideMutation.mutateAsync({
              path: { conversation_id: conversationId },
            })
            if (recordId !== null) {
              await queryClient.invalidateQueries({
                queryKey: requestRecordSessionRouteOptionsQueryKey({
                  path: { record_id: recordId },
                }),
              })
            }
          },
          'clear',
        ),
      'overrideCleared',
    )
  }

  async function resetAffinity(): Promise<void> {
    if (detailRecordId === null) return
    const recordId = detailRecordId
    const conversation = conversationId
    const key = usageDetailActionKey(conversation, recordId)
    try {
      const outcome = await asyncAction.run(
        key,
        async () => {
          const response = await resetAffinityMutation.mutateAsync({
            path: { record_id: recordId },
          })
          if (detailRecordIdRef.current === recordId) {
            await queryClient.invalidateQueries({
              queryKey: requestRecordSessionRouteOptionsQueryKey({
                path: { record_id: recordId },
              }),
            })
          }
          return response
        },
        'reset',
      )
      if (!outcome.executed) return
      notifySuccess(t(usageAffinityNoticeKey(outcome.value)))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  function loadFirstTranscript(): void {
    setTranscriptWanted(true)
  }

  function loadMoreTranscript(): void {
    if (!transcriptQuery.hasNextPage || transcriptQuery.isFetchingNextPage) {
      return
    }
    void transcriptQuery.fetchNextPage().catch((cause: unknown) => {
      reportApiError(cause)
    })
  }

  function openClearDialog(): void {
    if (asyncAction.isPending(USAGE_MAINTENANCE_KEY)) return
    setClearOpen(true)
  }

  function changeClearOpen(open: boolean): void {
    if (!open && asyncAction.isPending(USAGE_MAINTENANCE_KEY)) return
    setClearOpen(open)
  }

  async function submitClear(submitted: UsageClearForm): Promise<void> {
    try {
      const body = usageClearPayload(submitted)
      const outcome = await asyncAction.run(
        USAGE_MAINTENANCE_KEY,
        async () => {
          const result = await clearMutation.mutateAsync({ body })
          await refreshAll()
          return result
        },
        'clear',
      )
      if (!outcome.executed) return
      notifySuccess(usageClearToastMessage(t, outcome.value.deleted))
      setClearOpen(false)
    } catch (cause) {
      reportApiError(cause)
    }
  }

  return (
    <section
      aria-label={t(category === 'ai' ? 'aiRequests' : 'mcpCalls')}
      className="grid min-w-0 grid-cols-1 gap-3"
      data-foundation-route-id={
        category === 'ai' ? 'R-USAGE-AI' : 'R-USAGE-MCP'
      }
      data-usage-category={category}
    >
      <HeaderActions>
        {mode === 'records' && (
          <Button
            aria-controls="usage-advanced-filters"
            aria-expanded={filtersOpen}
            aria-label={
              activeFilterCount > 0
                ? t('advancedFiltersActive', { count: activeFilterCount })
                : t('advancedFilters')
            }
            data-usage-advanced-filters
            onClick={() => setFiltersOpen((open) => !open)}
            size="sm"
            type="button"
            variant="outline"
          >
            <SlidersHorizontal aria-hidden="true" className="size-4" />
            <span>{t('advancedFilters')}</span>
            {activeFilterCount > 0 && (
              <Badge
                className="h-4 min-w-4 px-1 text-[10px]"
                data-usage-active-filters={activeFilterCount}
                variant="secondary"
              >
                {activeFilterCount}
              </Badge>
            )}
            <ChevronDown
              aria-hidden="true"
              className={cn(
                'size-4 transition-transform',
                filtersOpen && 'rotate-180',
              )}
            />
          </Button>
        )}
        <UsageRangePicker
          end={end}
          onApply={applyRange}
          range={range}
          start={start}
        />
        <UsageModeSwitch onChange={changeMode} value={mode} />
        {mode === 'records' && (
          <Button
            onClick={openClearDialog}
            size="sm"
            type="button"
            variant="outline"
          >
            <Trash2 aria-hidden="true" />
            {t('clearHistory')}
          </Button>
        )}
        <Button
          aria-label={t('refresh')}
          disabled={recordsLoading || overviewLoading}
          onClick={refresh}
          size="icon-sm"
          type="button"
          variant="outline"
        >
          <RefreshCw aria-hidden="true" className="size-4" />
        </Button>
      </HeaderActions>

      {mode === 'overview' ? (
        <RequestRecordsOverviewPanel
          category={category}
          loading={overviewLoading}
          onChangePerspective={changePerspective}
          onChangeScope={changeScope}
          onDrilldown={drilldown}
          overview={overview}
          perspective={perspective}
          perspectivePending={asyncAction.isPending(USAGE_PERSPECTIVE_KEY)}
          scope={scope}
        />
      ) : (
        <div className="grid min-w-0 grid-cols-1 gap-3">
          <div
            data-usage-filter-panel
            hidden={!filtersOpen}
            id="usage-advanced-filters"
          >
            <RequestRecordsToolbar
              category={category}
              facets={facets}
              filters={filters}
              isAdmin={isAdmin}
              onFilters={changeFilters}
              onScope={changeScope}
              onSearch={changeSearch}
              scope={scope}
            />
          </div>
          <RequestRecordsTable
            category={category}
            first={currentFirst}
            isAdmin={isAdmin}
            loading={recordsLoading}
            onOpenDetail={openDetail}
            onPage={changePage}
            onSort={changeSort}
            records={records}
            rows={currentRows}
            sortField={sortField}
            sortOrder={sortOrder}
            total={total}
          />
        </div>
      )}

      {category === 'ai' ? (
        <UsageDetailDialog
          detail={detail}
          detailLoading={detailQuery.isFetching && !detail}
          isAdmin={isAdmin}
          onClearOverride={clearOverride}
          onLoadFirst={loadFirstTranscript}
          onLoadMore={loadMoreTranscript}
          onOpenChange={changeDetailOpen}
          onResetAffinity={() => void resetAffinity()}
          onSaveOverride={saveOverride}
          open={detailOpen}
          override={override}
          pending={detailPending}
          pendingOrigin={detailPendingOrigin}
          requestFull={requestFull}
          requestFullLoading={transcriptQuery.isLoading}
          requestFullLoadingMore={transcriptQuery.isFetchingNextPage}
          routeOptions={routeOptions}
          routeOptionsLoading={
            routeOptionsQuery.isFetching && !routeOptionsQuery.data
          }
        />
      ) : (
        <McpUsageDetailDialog
          detail={detail}
          detailLoading={detailQuery.isFetching && !detail}
          onOpenChange={changeDetailOpen}
          open={detailOpen}
        />
      )}

      <UsageClearDialog
        currentUserLabel={me ? `${me.display_name} / ${me.login_name}` : ''}
        isAdmin={isAdmin}
        onOpenChange={changeClearOpen}
        onSubmit={(submitted) => void submitClear(submitted)}
        open={clearOpen}
        pending={asyncAction.isPending(USAGE_MAINTENANCE_KEY)}
        pendingOrigin={asyncAction.pendingOrigin(USAGE_MAINTENANCE_KEY)}
        users={userOptions}
      />
    </section>
  )
}
