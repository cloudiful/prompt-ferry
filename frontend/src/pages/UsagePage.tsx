import { useEffect, useRef, useState } from 'react'
import { HeaderActions } from '@/components/layout/HeaderActions'
import { useQueryClient } from '@tanstack/react-query'
import { ChevronDown, RefreshCw, SlidersHorizontal, Trash2 } from 'lucide-react'
import { useLocation } from 'react-router'
import type { RequestRecordListRow } from '@/generated/admin-api'
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
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useLocale } from '@/lib/i18n'
import { cn } from '@/lib/utils'
import { useNotifier } from '@/lib/notifier'
import { usePageTitle } from '@/lib/router'
import { useSessionStore } from '@/lib/session'
import { useAsyncAction } from '@/lib/use-async-action'
import {
  USAGE_MAINTENANCE_KEY,
  useUsageDetailStore,
  usageDetailActionKey,
} from '@/lib/usage-detail'
import { usageAffinityNoticeKey } from '@/lib/usage-detail-model'
import {
  createDefaultUsageClearForm,
  isSameUsageClearForm,
  usageClearToastMessage,
  type UsageClearForm,
} from '@/lib/usage-clear'
import { useUsageStore, type UsageDrilldown } from '@/lib/usage'
import {
  USAGE_PERSPECTIVE_KEY,
  type UsageCategory,
  type UsageFilters,
  type UsageMode,
  type UsagePerspective,
  type UsageScope,
} from '@/lib/usage-model'

function categoryForPath(pathname: string): UsageCategory {
  return pathname.endsWith('/mcp') ? 'mcp' : 'ai'
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
 * Both routes share this page and derive the category from the path. The legacy
 * usage store owns the list/overview reads and the separate detail store owns
 * the selected record, so nothing is duplicated here: the page only wires the
 * controlled surfaces to the stores, the notifier and the query client.
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

  const mode = useUsageStore((state) => state.mode)
  const scope = useUsageStore((state) => state.scope)
  const range = useUsageStore((state) => state.range)
  const start = useUsageStore((state) => state.start)
  const end = useUsageStore((state) => state.end)
  const filters = useUsageStore((state) => state.filters)
  const first = useUsageStore((state) => state.first)
  const rows = useUsageStore((state) => state.rows)
  const total = useUsageStore((state) => state.total)
  const sortField = useUsageStore((state) => state.sortField)
  const sortOrder = useUsageStore((state) => state.sortOrder)
  const perspective = useUsageStore((state) => state.perspective)
  const records = useUsageStore((state) => state.records)
  const facets = useUsageStore((state) => state.facets)
  const overview = useUsageStore((state) => state.overview)
  const userOptions = useUsageStore((state) => state.userOptions)
  const recordsLoading = useUsageStore((state) => state.recordsLoading)
  const overviewLoading = useUsageStore((state) => state.overviewLoading)

  const activeFilterCount = countActiveFilters(filters, scope, category)

  const detail = useUsageDetailStore((state) => state.detail)
  const detailLoading = useUsageDetailStore((state) => state.detailLoading)
  const detailRecordId = useUsageDetailStore((state) => state.recordId)
  const requestFull = useUsageDetailStore((state) => state.requestFull)
  const requestFullLoading = useUsageDetailStore(
    (state) => state.requestFullLoading,
  )
  const requestFullLoadingMore = useUsageDetailStore(
    (state) => state.requestFullLoadingMore,
  )
  const routeOptions = useUsageDetailStore((state) => state.routeOptions)
  const routeOptionsLoading = useUsageDetailStore(
    (state) => state.routeOptionsLoading,
  )
  const override = useUsageDetailStore((state) => state.override)

  const [detailOpen, setDetailOpen] = useState(false)
  const [filtersOpen, setFiltersOpen] = useState(false)
  const [clearOpen, setClearOpen] = useState(false)
  const [clearForm, setClearForm] = useState(createDefaultUsageClearForm)
  const clearFormRef = useRef(clearForm)
  clearFormRef.current = clearForm
  const initializedFor = useRef<string | null>(null)

  useEffect(() => {
    if (!me) return
    const identity = { user_id: me.user_id, is_admin: me.is_admin === true }
    const usage = useUsageStore.getState()
    usage.initializeIdentity(identity)
    useUsageDetailStore.getState().initializeIdentity(identity)
    const key = `${useUsageStore.getState().identityKey}:${category}`
    if (initializedFor.current === key) return
    initializedFor.current = key
    const load =
      useUsageStore.getState().category === category
        ? useUsageStore.getState().refreshAll(queryClient)
        : useUsageStore.getState().setCategory(queryClient, category)
    void load.catch((cause: unknown) => notifyApiError(cause))
    // One category+refresh per identity/category, never per render.
  }, [me, category, queryClient, notifyApiError])

  useEffect(() => {
    return () => {
      useUsageDetailStore.getState().reset()
    }
  }, [])

  const conversationId = detail?.conversation_id ?? null
  const detailActionKey =
    detailRecordId === null
      ? null
      : usageDetailActionKey(conversationId, detailRecordId)
  const detailPendingOrigin = detailActionKey
    ? (asyncAction.pendingOrigin(detailActionKey) ?? null)
    : null
  const detailPending = detailPendingOrigin !== null

  function refresh(): void {
    void useUsageStore
      .getState()
      .refreshAll(queryClient)
      .catch((cause: unknown) => notifyApiError(cause))
  }

  function applyRange(input: UsageRangeApply): void {
    void useUsageStore
      .getState()
      .setRange(queryClient, input.range, input.start, input.end)
      .catch((cause: unknown) => notifyApiError(cause))
  }

  function changeMode(next: UsageMode): void {
    void useUsageStore
      .getState()
      .setMode(next, queryClient)
      .catch((cause: unknown) => notifyApiError(cause))
  }

  function changeFilters(patch: Partial<UsageFilters>): void {
    void useUsageStore
      .getState()
      .setFilters(queryClient, patch)
      .catch((cause: unknown) => notifyApiError(cause))
  }

  function changeSearch(search: string | null): void {
    changeFilters({ search })
  }

  function changeScope(next: UsageScope): void {
    void useUsageStore
      .getState()
      .setScope(queryClient, next)
      .catch((cause: unknown) => notifyApiError(cause))
  }

  function changeSort(field: string): void {
    void useUsageStore
      .getState()
      .setSort(queryClient, field)
      .catch((cause: unknown) => notifyApiError(cause))
  }

  function changePage(nextFirst: number, nextRows: number): void {
    void useUsageStore
      .getState()
      .setPage(queryClient, nextFirst, nextRows)
      .catch((cause: unknown) => notifyApiError(cause))
  }

  function changePerspective(next: UsagePerspective): void {
    void useUsageStore
      .getState()
      .setPerspective(queryClient, next)
      .catch((cause: unknown) => notifyApiError(cause))
  }

  function drilldown(next: UsageDrilldown): void {
    void useUsageStore
      .getState()
      .applyDrilldown(queryClient, next)
      .catch((cause: unknown) => notifyApiError(cause))
  }

  async function openDetail(record: RequestRecordListRow): Promise<void> {
    const recordId = record.record_id
    setDetailOpen(true)
    try {
      await useUsageDetailStore.getState().loadDetail(recordId)
    } catch (cause) {
      if (useUsageDetailStore.getState().recordId === recordId) {
        useUsageDetailStore.getState().reset()
        setDetailOpen(false)
      }
      notifyApiError(cause)
    }
  }

  function changeDetailOpen(open: boolean): void {
    if (!open && detailPending) return
    setDetailOpen(open)
    if (!open) useUsageDetailStore.getState().reset()
  }

  function loadFirstTranscript(): void {
    if (detailRecordId === null) return
    void useUsageDetailStore
      .getState()
      .loadRequestFull(detailRecordId)
      .catch((cause: unknown) => notifyApiError(cause))
  }

  function loadMoreTranscript(): void {
    if (detailRecordId === null) return
    void useUsageDetailStore
      .getState()
      .loadMoreRequestFull(detailRecordId)
      .catch((cause: unknown) => notifyApiError(cause))
  }

  async function runRoutingWrite(
    write: () => Promise<{ executed: boolean; value?: unknown }>,
    successKey: 'overrideSaved' | 'overrideCleared',
  ): Promise<void> {
    try {
      const outcome = await write()
      if (outcome.executed) notifySuccess(t(successKey))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  function saveOverride(
    endpointId: string,
    endpointKeyId: string | null,
  ): void {
    if (!conversationId) return
    void runRoutingWrite(
      () =>
        useUsageDetailStore
          .getState()
          .saveOverride(conversationId, endpointId, endpointKeyId),
      'overrideSaved',
    )
  }

  function clearOverride(): void {
    if (!conversationId) return
    void runRoutingWrite(
      () => useUsageDetailStore.getState().clearOverride(conversationId),
      'overrideCleared',
    )
  }

  async function resetAffinity(): Promise<void> {
    if (detailRecordId === null) return
    try {
      const outcome = await useUsageDetailStore
        .getState()
        .resetAffinity(detailRecordId)
      if (!outcome.executed) return
      notifySuccess(t(usageAffinityNoticeKey(outcome.value)))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  function openClearDialog(): void {
    if (asyncAction.isPending(USAGE_MAINTENANCE_KEY)) return
    const next = createDefaultUsageClearForm()
    clearFormRef.current = next
    setClearForm(next)
    setClearOpen(true)
  }

  function changeClearOpen(open: boolean): void {
    if (!open && asyncAction.isPending(USAGE_MAINTENANCE_KEY)) return
    setClearOpen(open)
  }

  async function submitClear(submitted: UsageClearForm): Promise<void> {
    try {
      const outcome = await useUsageStore
        .getState()
        .clearHistory(queryClient, submitted)
      if (!outcome.executed) return
      notifySuccess(usageClearToastMessage(t, outcome.value.deleted))
      if (isSameUsageClearForm(submitted, clearFormRef.current)) {
        setClearOpen(false)
        const reset = createDefaultUsageClearForm()
        clearFormRef.current = reset
        setClearForm(reset)
      }
    } catch (cause) {
      notifyApiError(cause)
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
            first={first}
            isAdmin={isAdmin}
            loading={recordsLoading}
            onOpenDetail={(record) => void openDetail(record)}
            onPage={changePage}
            onSort={changeSort}
            records={records}
            rows={rows}
            sortField={sortField}
            sortOrder={sortOrder}
            total={total}
          />
        </div>
      )}

      {category === 'ai' ? (
        <UsageDetailDialog
          detail={detail}
          detailLoading={detailLoading}
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
          requestFullLoading={requestFullLoading}
          requestFullLoadingMore={requestFullLoadingMore}
          routeOptions={routeOptions}
          routeOptionsLoading={routeOptionsLoading}
        />
      ) : (
        <McpUsageDetailDialog
          detail={detail}
          detailLoading={detailLoading}
          onOpenChange={changeDetailOpen}
          open={detailOpen}
        />
      )}

      <UsageClearDialog
        currentUserLabel={me ? `${me.display_name} / ${me.login_name}` : ''}
        form={clearForm}
        isAdmin={isAdmin}
        onFormChange={(next) => {
          clearFormRef.current = next
          setClearForm(next)
        }}
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
