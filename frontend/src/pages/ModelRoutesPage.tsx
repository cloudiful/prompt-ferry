import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { LoaderCircle, Plus, RefreshCw } from 'lucide-react'
import {
  modelRouteFormToRequest,
  modelRouteToForm,
  createEmptyModelRouteForm,
} from '@/admin-mappers/forms/model-route'
import { capturePayload } from '@/stores/record-action-types'
import type {
  ModelEndpointRule,
  ModelRouteTestResponse,
  ProviderEndpoint,
} from '@/generated/admin-api'
import { ModelRouteDialog } from '@/components/endpoints/ModelRouteDialog'
import { ModelRouteMobileCard } from '@/components/endpoints/ModelRouteMobileCard'
import { ModelRouteTestResultsDialog } from '@/components/endpoints/ModelRouteTestResultsDialog'
import { ModelRoutesTable } from '@/components/endpoints/ModelRoutesTable'
import { TablePagination } from '@/components/shared/TablePagination'
import {
  testResultKey,
  type TestResultSurface,
} from '@/components/shared/TestResultPopover'
import { Button } from '@/components/ui/button'
import {
  endpointRecordsQueryKey,
  endpointUserOptionsQueryKey,
  listAllEndpointRecords,
  listEndpointUsers,
} from '@/lib/endpoints'
import {
  modelRoutePageQueryKey,
  listModelRoutePage,
  patchModelRoutePages,
  refreshModelRoutePage,
  removeModelRoute,
  saveModelRoute,
  setModelRouteEnabled,
  setModelRouteTargetEnabled,
  testModelRoute,
} from '@/lib/model-routes'
import { useAsyncAction } from '@/lib/use-async-action'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import { modelRouteActionKey, networkTestKey } from '@/models/endpoints'
import {
  createModelRouteListItemView,
  modelRouteTargetToggleOrigin,
} from '@/models/endpoints/model-route-item'
import type { ModelRouteForm } from '@/models'
import type { TablePageChange } from '@/models/pagination'
import { STANDARD_PAGE_SIZE_OPTIONS } from '@/models/pagination'
import { useStoredPageSize } from '@/components/shared/TablePagination'

const EMPTY_ENDPOINTS: ProviderEndpoint[] = []
const EMPTY_ROUTES: ModelEndpointRule[] = []

export function ModelRoutesPage() {
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const asyncAction = useAsyncAction()
  const { notifyApiError, notifySuccess } = useNotifier()
  const me = useSessionStore((state) => state.me)
  const isAdmin = me?.is_admin === true
  const [first, setFirst] = useState(0)
  const [rows, setRows] = useStoredPageSize(
    'model-routes',
    10,
    STANDARD_PAGE_SIZE_OPTIONS,
  )
  const [testResults, setTestResults] = useState<
    Record<string, ModelRouteTestResponse>
  >({})
  const [openTestResultKey, setOpenTestResultKey] = useState<string | null>(
    null,
  )
  const [dialogForm, setDialogForm] = useState<ModelRouteForm | null>(null)
  const [detailsRuleId, setDetailsRuleId] = useState<string | null>(null)
  const firstRef = useRef(first)
  const rowsRef = useRef(rows)
  const dialogOpenRef = useRef(false)
  const dialogRuleIdRef = useRef<string | null>(null)
  const dialogGenerationRef = useRef(0)
  const notifiedErrors = useRef(new WeakSet<object>())

  firstRef.current = first
  rowsRef.current = rows

  const routePageQuery = useQuery({
    enabled: isAdmin,
    queryKey: modelRoutePageQueryKey(first, rows),
    queryFn: () => listModelRoutePage(first, rows),
  })
  const endpointsQuery = useQuery({
    enabled: isAdmin,
    queryKey: endpointRecordsQueryKey(),
    queryFn: listAllEndpointRecords,
    staleTime: 0,
  })
  const usersQuery = useQuery({
    enabled: isAdmin && me !== null,
    queryKey: endpointUserOptionsQueryKey(me?.user_id ?? 0),
    queryFn: listEndpointUsers,
  })

  const reportApiError = useCallback(
    (cause: unknown) => {
      if (cause && typeof cause === 'object') {
        if (notifiedErrors.current.has(cause)) return
        notifiedErrors.current.add(cause)
      }
      notifyApiError(cause)
    },
    [notifyApiError],
  )

  useEffect(() => {
    if (routePageQuery.error) reportApiError(routePageQuery.error)
  }, [reportApiError, routePageQuery.error])
  useEffect(() => {
    if (endpointsQuery.error) reportApiError(endpointsQuery.error)
  }, [endpointsQuery.error, reportApiError])
  useEffect(() => {
    if (usersQuery.error) reportApiError(usersQuery.error)
  }, [reportApiError, usersQuery.error])

  const page = routePageQuery.data
  useEffect(() => {
    if (!page || page.rows <= 0) return
    let correctedFirst = page.first
    if (page.routes.length === 0 && page.first >= page.total) {
      correctedFirst =
        page.total > 0
          ? Math.floor((page.total - 1) / page.rows) * page.rows
          : 0
    }
    if (correctedFirst !== first) {
      firstRef.current = correctedFirst
      setFirst(correctedFirst)
    } else if (page.first !== first) {
      firstRef.current = page.first
      setFirst(page.first)
    }
    if (page.rows !== rows) {
      rowsRef.current = page.rows
      setRows(page.rows)
    }
  }, [first, page, rows, setRows])

  useEffect(() => {
    setOpenTestResultKey(null)
  }, [first])

  const routes = page?.routes ?? EMPTY_ROUTES
  const endpoints = endpointsQuery.data ?? EMPTY_ENDPOINTS
  const users = usersQuery.data ?? []
  const pendingActions = asyncAction.pendingActions
  const items = useMemo(
    () =>
      routes.map((route) =>
        createModelRouteListItemView(route, {
          endpointTestIdle: t('endpointTestIdle'),
          modelRouteTestNoneEligible: t('modelRouteTestNoneEligible'),
          modelRouteTestPassed: (passed, total) =>
            t('modelRouteTestPassed', { passed, total }),
          ownerLabel: t('owner'),
          scopeAdmin: t('scopeAdmin'),
          scopeUser: t('scopeUser'),
          testResult: testResults[route.rule_id] ?? null,
          testingModelRouteId: asyncAction.isPending(
            networkTestKey('modelroute', route.rule_id),
          )
            ? route.rule_id
            : '',
          togglingModelRouteId:
            asyncAction.pendingOrigin(modelRouteActionKey(route.rule_id)) ===
            'toggle'
              ? route.rule_id
              : '',
        }),
      ),
    [asyncAction, routes, t, testResults],
  )

  function setPageAfterRefresh(
    refreshed: { first: number; rows: number },
    capturedFirst: number,
    capturedRows: number,
  ): void {
    if (firstRef.current !== capturedFirst || rowsRef.current !== capturedRows)
      return
    const nextRows = refreshed.rows > 0 ? refreshed.rows : capturedRows
    firstRef.current = refreshed.first
    rowsRef.current = nextRows
    setFirst(refreshed.first)
    setRows(nextRows)
    setOpenTestResultKey(null)
  }

  function changePage(change: TablePageChange): void {
    firstRef.current = change.first
    rowsRef.current = change.rows
    setFirst(change.first)
    setRows(change.rows)
    setOpenTestResultKey(null)
  }

  function changeTestResultOpen(key: string, open: boolean): void {
    if (open) return
    setOpenTestResultKey((current) => (current === key ? null : current))
  }

  function showModelRouteDetails(ruleId: string): void {
    setOpenTestResultKey(null)
    setDetailsRuleId(ruleId)
  }

  function openCreateDialog(): void {
    dialogGenerationRef.current += 1
    dialogRuleIdRef.current = null
    dialogOpenRef.current = true
    setDialogForm(createEmptyModelRouteForm())
  }

  function openEditDialog(ruleId: string): void {
    const route = routes.find((item) => item.rule_id === ruleId)
    if (!route) return
    dialogGenerationRef.current += 1
    dialogRuleIdRef.current = ruleId
    dialogOpenRef.current = true
    setDialogForm(modelRouteToForm(route))
  }

  function changeDialogOpen(open: boolean): void {
    const ruleId = dialogRuleIdRef.current
    if (!open && asyncAction.isPending(modelRouteActionKey(ruleId))) return
    dialogOpenRef.current = open
    if (!open) {
      dialogGenerationRef.current += 1
      dialogRuleIdRef.current = null
      setDialogForm(null)
    }
  }

  async function submitModelRoute(): Promise<void> {
    const currentForm = dialogForm
    if (!currentForm) return
    const ruleId = currentForm.rule_id || null
    const capturedRuleId = ruleId
    const body = capturePayload(modelRouteFormToRequest(currentForm))
    const capturedFirst = firstRef.current
    const capturedRows = rowsRef.current
    const generation = dialogGenerationRef.current

    try {
      const outcome = await asyncAction.run(
        modelRouteActionKey(capturedRuleId),
        async () => {
          const saved = await saveModelRoute(capturedRuleId, body)
          const refreshedPage = await refreshModelRoutePage(
            queryClient,
            capturedFirst,
            capturedRows,
          )
          setPageAfterRefresh(refreshedPage, capturedFirst, capturedRows)
          return saved
        },
        'save',
      )
      if (!outcome.executed) return
      if (
        dialogOpenRef.current &&
        dialogGenerationRef.current === generation &&
        dialogRuleIdRef.current === capturedRuleId
      ) {
        dialogOpenRef.current = false
        dialogRuleIdRef.current = null
        dialogGenerationRef.current += 1
        setDialogForm(null)
      }
      notifySuccess(t('saved'))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function deleteModelRoute(ruleId: string): Promise<void> {
    const route = routes.find((item) => item.rule_id === ruleId)
    if (!route || !window.confirm(`${t('delete')} ${route.model_pattern}?`))
      return
    const capturedRuleId = route.rule_id
    const capturedFirst = firstRef.current
    const capturedRows = rowsRef.current
    try {
      const outcome = await asyncAction.run(
        modelRouteActionKey(capturedRuleId),
        async () => {
          await removeModelRoute(capturedRuleId)
          const refreshedPage = await refreshModelRoutePage(
            queryClient,
            capturedFirst,
            capturedRows,
          )
          setPageAfterRefresh(refreshedPage, capturedFirst, capturedRows)
        },
        'delete',
      )
      if (outcome.executed) notifySuccess(t('delete'))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function toggleModelRoute(
    ruleId: string,
    enabled: boolean,
  ): Promise<void> {
    const route = routes.find((item) => item.rule_id === ruleId)
    if (!route) return
    const capturedRoute = capturePayload(route)
    try {
      await asyncAction.run(
        modelRouteActionKey(ruleId),
        async () => {
          const saved = await setModelRouteEnabled(capturedRoute, enabled)
          patchModelRoutePages(queryClient, saved)
        },
        'toggle',
      )
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function toggleModelRouteTarget(
    ruleId: string,
    targetId: string,
    enabled: boolean,
  ): Promise<void> {
    const route = routes.find((item) => item.rule_id === ruleId)
    if (!route) return
    const capturedRoute = capturePayload(route)
    const capturedTargetId = targetId
    try {
      const outcome = await asyncAction.run(
        modelRouteActionKey(ruleId),
        async () => {
          const saved = await setModelRouteTargetEnabled(
            capturedRoute,
            capturedTargetId,
            enabled,
          )
          patchModelRoutePages(queryClient, saved)
          return saved
        },
        modelRouteTargetToggleOrigin(capturedTargetId),
      )
      if (outcome.executed) notifySuccess(t('saved'))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function probeModelRoute(
    ruleId: string,
    surface: TestResultSurface,
  ): Promise<void> {
    const capturedSurface = surface
    try {
      const outcome = await asyncAction.run(
        networkTestKey('modelroute', ruleId),
        () => testModelRoute(ruleId),
        'test',
      )
      if (!outcome.executed) return
      setTestResults((current) => ({ ...current, [ruleId]: outcome.value }))
      setOpenTestResultKey(testResultKey(capturedSurface, ruleId))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function refresh(): Promise<void> {
    const reads: Promise<unknown>[] = [
      routePageQuery.refetch(),
      endpointsQuery.refetch(),
    ]
    if (isAdmin) reads.push(usersQuery.refetch())
    await Promise.all(reads)
  }

  const details = detailsRuleId ? testResults[detailsRuleId] : undefined

  return (
    <section
      className="grid min-w-0 gap-3"
      data-foundation-route-id="R-ENDPOINTS-ROUTES"
    >
      <div className="flex flex-wrap items-center justify-end gap-2">
        <Button onClick={openCreateDialog} type="button">
          <Plus />
          {t('newModelRoute')}
        </Button>
        <Button
          disabled={routePageQuery.isFetching || endpointsQuery.isFetching}
          onClick={() => void refresh()}
          type="button"
          variant="outline"
        >
          {routePageQuery.isFetching || endpointsQuery.isFetching ? (
            <LoaderCircle className="animate-spin" />
          ) : (
            <RefreshCw />
          )}
          {t('refresh')}
        </Button>
      </div>
      {routePageQuery.error ? (
        <div
          className="grid gap-2 rounded-lg border border-destructive/40 p-3"
          role="alert"
        >
          <span>
            {t('error')}:{' '}
            {String(
              routePageQuery.error instanceof Error
                ? routePageQuery.error.message
                : routePageQuery.error,
            )}
          </span>
          <Button
            onClick={() => void routePageQuery.refetch()}
            size="sm"
            type="button"
            variant="outline"
          >
            {t('retry')}
          </Button>
        </div>
      ) : null}
      {endpointsQuery.error ? (
        <div
          className="grid gap-2 rounded-lg border border-destructive/40 p-3"
          role="alert"
        >
          <span>
            {t('error')}:{' '}
            {String(
              endpointsQuery.error instanceof Error
                ? endpointsQuery.error.message
                : endpointsQuery.error,
            )}
          </span>
          <Button
            onClick={() => void endpointsQuery.refetch()}
            size="sm"
            type="button"
            variant="outline"
          >
            {t('retry')}
          </Button>
        </div>
      ) : null}
      {routePageQuery.isLoading ? (
        <div
          aria-label={t('loading')}
          className="grid min-h-32 place-items-center"
          role="status"
        >
          <LoaderCircle className="animate-spin" />
        </div>
      ) : (
        <>
          <ModelRoutesTable
            first={page?.first ?? first}
            items={items}
            onDelete={(id) => void deleteModelRoute(id)}
            onEdit={openEditDialog}
            onPageChange={changePage}
            onProbe={(id, surface) => void probeModelRoute(id, surface)}
            onResultOpenChange={changeTestResultOpen}
            onShowDetails={showModelRouteDetails}
            onToggle={(id, enabled) => void toggleModelRoute(id, enabled)}
            onToggleTarget={(id, targetId, enabled) =>
              void toggleModelRouteTarget(id, targetId, enabled)
            }
            openResultKey={openTestResultKey}
            pendingActions={pendingActions}
            rows={page?.rows ?? rows}
            total={page?.total ?? 0}
          />
          <div className="grid gap-2 md:hidden">
            {items.length === 0 ? (
              <div className="rounded-xl border p-6 text-center text-sm text-muted-foreground">
                {t('noModelRoutes')}
              </div>
            ) : (
              items.map((item) => (
                <ModelRouteMobileCard
                  item={item}
                  key={item.rule_id}
                  onDelete={(id) => void deleteModelRoute(id)}
                  onEdit={openEditDialog}
                  onProbe={(id, surface) => void probeModelRoute(id, surface)}
                  onResultOpenChange={changeTestResultOpen}
                  onShowDetails={showModelRouteDetails}
                  onToggle={(id, enabled) => void toggleModelRoute(id, enabled)}
                  onToggleTarget={(id, targetId, enabled) =>
                    void toggleModelRouteTarget(id, targetId, enabled)
                  }
                  openResultKey={openTestResultKey}
                  pendingActions={pendingActions}
                />
              ))
            )}
            <div className="rounded-xl border bg-card">
              <TablePagination
                first={page?.first ?? first}
                onChange={changePage}
                pageSizeOptions={STANDARD_PAGE_SIZE_OPTIONS}
                rows={page?.rows ?? rows}
                total={page?.total ?? 0}
              />
            </div>
          </div>
        </>
      )}
      {dialogForm ? (
        <ModelRouteDialog
          endpoints={endpoints}
          endpointOptions={endpoints.map((endpoint) => ({
            label: endpoint.name,
            value: endpoint.endpoint_id,
            provider: endpoint.provider,
          }))}
          form={dialogForm}
          onChange={setDialogForm}
          onOpenChange={changeDialogOpen}
          onSave={() => void submitModelRoute()}
          open={dialogOpenRef.current}
          ownerOptionsLoading={usersQuery.isLoading}
          savePending={asyncAction.isPending(
            modelRouteActionKey(dialogForm.rule_id || null),
          )}
          users={users}
        />
      ) : null}
      <ModelRouteTestResultsDialog
        onOpenChange={(open) => {
          if (!open) setDetailsRuleId(null)
        }}
        open={detailsRuleId !== null}
        results={details?.results ?? []}
      />
    </section>
  )
}
