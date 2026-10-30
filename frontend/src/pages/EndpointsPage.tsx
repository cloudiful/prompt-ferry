import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import { EndpointDialog } from '@/components/endpoints/EndpointDialog'
import { EndpointsTable } from '@/components/endpoints/EndpointsTable'
import { TokenPlanUsageDialog } from '@/components/endpoints/TokenPlanUsageDialog'
import {
  testResultKey,
  type TestResultSurface,
} from '@/components/shared/TestResultPopover'
import type {
  EndpointTestResponse,
  ProviderEndpoint,
} from '@/generated/admin-api'
import {
  createEmptyEndpointForm,
  endpointFormToRequest,
  endpointToForm,
} from '@/admin-mappers/forms/endpoint'
import { capturePayload } from '@/stores/record-action-types'
import {
  endpointRecordsQueryKey,
  endpointUserOptionsQueryKey,
  endpointsPageQueryKey,
  listEndpointUsers,
  listEndpointsPage,
  modelRoutePagesQueryKey,
  patchEndpointPages,
  refreshEndpointsPage,
  removeEndpointRecord,
  saveEndpointRecord,
  setEndpointEnabled,
  testEndpointRecord,
} from '@/lib/endpoints'
import { prefetchTokenPlanBatch } from '@/lib/quota-cache'
import { registerTokenPlanForegroundIds } from '@/lib/quota-foreground'
import { useAsyncAction } from '@/lib/use-async-action'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import { isQuotaEligible } from '@/models/endpoints/quota'
import { createEndpointListItemView } from '@/models/endpoints/endpoint-item'
import {
  endpointActionKey,
  networkTestKey,
  type EndpointListItemView,
} from '@/models/endpoints'
import type { EndpointForm } from '@/models'
import type { RunOAuthAction } from '@/models/endpoints/oauth'
import type { TablePageChange } from '@/models/pagination'
import { useStoredPageSize } from '@/components/shared/TablePagination'

type TokenPlanDialogTarget = {
  endpointId: string
  endpointName: string
  provider: ProviderEndpoint['provider']
}

const EMPTY_ENDPOINTS: ProviderEndpoint[] = []

export function EndpointsPage() {
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const asyncAction = useAsyncAction()
  const { notifyApiError, notifySuccess } = useNotifier()
  const me = useSessionStore((state) => state.me)
  const isAdmin = me?.is_admin === true
  const [first, setFirst] = useState(0)
  const [rows, setRows] = useStoredPageSize('endpoints')
  const [testResults, setTestResults] = useState<
    Record<string, EndpointTestResponse>
  >({})
  const [openTestResultKey, setOpenTestResultKey] = useState<string | null>(
    null,
  )
  const [dialogOpen, setDialogOpen] = useState(false)
  const [dialogEndpointId, setDialogEndpointId] = useState<string | null>(null)
  const [dialogForm, setDialogForm] = useState<EndpointForm | null>(null)
  const [tokenPlanDialogTarget, setTokenPlanDialogTarget] =
    useState<TokenPlanDialogTarget | null>(null)
  const firstRef = useRef(first)
  const rowsRef = useRef(rows)
  const dialogOpenRef = useRef(false)
  const dialogEndpointIdRef = useRef<string | null>(null)
  const dialogGenerationRef = useRef(0)
  const quotaPageGenerationRef = useRef(0)
  const notifiedErrors = useRef(new WeakSet<object>())

  firstRef.current = first
  rowsRef.current = rows

  const endpointPageQuery = useQuery({
    enabled: isAdmin,
    queryKey: endpointsPageQueryKey(first, rows),
    queryFn: () => listEndpointsPage(first, rows),
  })
  const userOptionsQuery = useQuery({
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
  const runOAuthAction = useCallback<RunOAuthAction>(
    (endpointId, action, label) =>
      asyncAction.run(endpointActionKey(endpointId), action, label),
    [asyncAction.run],
  )

  useEffect(() => {
    if (endpointPageQuery.error) reportApiError(endpointPageQuery.error)
  }, [endpointPageQuery.error, reportApiError])

  useEffect(() => {
    if (userOptionsQuery.error) reportApiError(userOptionsQuery.error)
  }, [reportApiError, userOptionsQuery.error])

  useEffect(() => {
    const page = endpointPageQuery.data
    if (!page || page.rows <= 0) return
    if (page.endpoints.length === 0 && page.first >= page.total) {
      const correctedFirst =
        page.total > 0
          ? Math.floor((page.total - 1) / page.rows) * page.rows
          : 0
      if (correctedFirst !== first) {
        firstRef.current = correctedFirst
        setFirst(correctedFirst)
      }
      return
    }
    if (page.first !== first) {
      firstRef.current = page.first
      setFirst(page.first)
    }
    if (page.rows !== rows) {
      rowsRef.current = page.rows
      setRows(page.rows)
    }
  }, [endpointPageQuery.data, first, rows, setRows])

  const endpoints = endpointPageQuery.data?.endpoints ?? EMPTY_ENDPOINTS
  const users = userOptionsQuery.data ?? []
  const eligibleQuotaIds = useMemo(
    () =>
      endpoints.filter(isQuotaEligible).map((endpoint) => endpoint.endpoint_id),
    [endpoints],
  )
  const eligibleQuotaIdsKey = JSON.stringify(eligibleQuotaIds)

  useEffect(() => {
    if (!isAdmin) return
    const ids = JSON.parse(eligibleQuotaIdsKey) as string[]
    if (ids.length === 0) return
    const generation = ++quotaPageGenerationRef.current
    const eligible = new Set(ids)
    const release = registerTokenPlanForegroundIds(ids)
    void prefetchTokenPlanBatch(
      ids,
      4,
      (endpointId) =>
        generation === quotaPageGenerationRef.current &&
        eligible.has(endpointId),
    )
    return () => {
      if (generation === quotaPageGenerationRef.current) {
        quotaPageGenerationRef.current += 1
      }
      release()
    }
  }, [eligibleQuotaIdsKey, isAdmin])
  const probeInFlightId =
    endpoints.find((endpoint) =>
      asyncAction.isPending(networkTestKey('endpoint', endpoint.endpoint_id)),
    )?.endpoint_id ?? ''
  const toggleInFlightId =
    endpoints.find(
      (endpoint) =>
        asyncAction.pendingOrigin(endpointActionKey(endpoint.endpoint_id)) ===
        'toggle',
    )?.endpoint_id ?? ''
  const items = useMemo<EndpointListItemView[]>(
    () =>
      endpoints.map((endpoint) =>
        createEndpointListItemView(endpoint, {
          endpointTestIdle: t('endpointTestIdle'),
          endpointSourceAuto: t('endpointSourceAuto'),
          endpointSourceDetected: t('endpointSourceDetected'),
          endpointSourceManual: t('endpointSourceManual'),
          nativeApiChat: t('nativeApiChat'),
          nativeApiAnthropicMessages: t('nativeApiAnthropicMessages'),
          nativeApiResponses: t('nativeApiResponses'),
          nativeApiRealtime: t('nativeApiRealtime'),
          ownerLabel: t('owner'),
          scopeAdmin: t('scopeAdmin'),
          scopeUser: t('scopeUser'),
          testResult: testResults[endpoint.endpoint_id] ?? null,
          testingEndpointId: probeInFlightId,
          togglingEndpointId: toggleInFlightId,
        }),
      ),
    [endpoints, probeInFlightId, t, testResults, toggleInFlightId],
  )

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

  function openTokenPlanUsage(endpointId: string): void {
    const endpoint = endpoints.find((item) => item.endpoint_id === endpointId)
    if (!endpoint || !isQuotaEligible(endpoint)) return
    setTokenPlanDialogTarget({
      endpointId,
      endpointName: endpoint.name,
      provider: endpoint.provider,
    })
  }

  function changeTokenPlanDialogOpen(open: boolean): void {
    if (!open) setTokenPlanDialogTarget(null)
  }

  function setPageAfterRefresh(
    page: { first: number; rows: number },
    capturedFirst: number,
    capturedRows: number,
  ): void {
    if (firstRef.current !== capturedFirst || rowsRef.current !== capturedRows)
      return
    const nextRows = page.rows > 0 ? page.rows : capturedRows
    firstRef.current = page.first
    rowsRef.current = nextRows
    setFirst(page.first)
    setRows(nextRows)
    setOpenTestResultKey(null)
  }

  function openCreateDialog(): void {
    dialogGenerationRef.current += 1
    dialogEndpointIdRef.current = null
    dialogOpenRef.current = true
    setDialogEndpointId(null)
    setDialogForm(createEmptyEndpointForm())
    setDialogOpen(true)
  }

  function openEditDialog(endpointId: string): void {
    const endpoint = endpoints.find((item) => item.endpoint_id === endpointId)
    if (!endpoint) return
    dialogGenerationRef.current += 1
    dialogEndpointIdRef.current = endpointId
    dialogOpenRef.current = true
    setDialogEndpointId(endpointId)
    setDialogForm(endpointToForm(endpoint))
    setDialogOpen(true)
  }

  function changeDialogOpen(open: boolean): void {
    const endpointId = dialogEndpointIdRef.current
    if (!open && asyncAction.isPending(endpointActionKey(endpointId))) return
    dialogOpenRef.current = open
    setDialogOpen(open)
    if (!open) {
      dialogGenerationRef.current += 1
      dialogEndpointIdRef.current = null
      setDialogEndpointId(null)
      setDialogForm(null)
    }
  }

  function updateDialogForm(
    update: (current: EndpointForm) => EndpointForm,
  ): void {
    setDialogForm((current) => (current ? update(current) : current))
  }

  async function invalidateModelRoutePages(): Promise<void> {
    await Promise.all([
      queryClient.invalidateQueries({
        queryKey: modelRoutePagesQueryKey(),
        refetchType: 'none',
      }),
      queryClient.invalidateQueries({
        queryKey: endpointRecordsQueryKey(),
        refetchType: 'none',
      }),
    ])
  }

  async function submitEndpoint(): Promise<void> {
    const currentForm = dialogForm
    if (!currentForm) return
    const endpointId = dialogEndpointIdRef.current
    const capturedId = endpointId
    const body = capturePayload(endpointFormToRequest(currentForm))
    const capturedFirst = firstRef.current
    const capturedRows = rowsRef.current
    const generation = dialogGenerationRef.current

    try {
      const outcome = await asyncAction.run(
        endpointActionKey(capturedId),
        async () => {
          const saved = await saveEndpointRecord(capturedId, body)
          await invalidateModelRoutePages()
          const refreshedPage = await refreshEndpointsPage(
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
        dialogEndpointIdRef.current === capturedId
      ) {
        dialogOpenRef.current = false
        dialogEndpointIdRef.current = null
        setDialogOpen(false)
        setDialogEndpointId(null)
        setDialogForm(null)
        dialogGenerationRef.current += 1
      }
      notifySuccess(t('endpointSaved'))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function deleteEndpoint(endpointId: string): Promise<void> {
    const endpoint = endpoints.find((item) => item.endpoint_id === endpointId)
    if (!endpoint || !window.confirm(`${t('delete')} ${endpoint.name}?`)) return
    const capturedId = endpoint.endpoint_id
    const capturedFirst = firstRef.current
    const capturedRows = rowsRef.current

    try {
      const outcome = await asyncAction.run(
        endpointActionKey(capturedId),
        async () => {
          await removeEndpointRecord(capturedId)
          await invalidateModelRoutePages()
          const refreshedPage = await refreshEndpointsPage(
            queryClient,
            capturedFirst,
            capturedRows,
          )
          setPageAfterRefresh(refreshedPage, capturedFirst, capturedRows)
        },
        'delete',
      )
      if (outcome.executed) notifySuccess(t('endpointDeleted'))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function toggleEndpoint(
    endpointId: string,
    enabled: boolean,
  ): Promise<void> {
    const endpoint = endpoints.find((item) => item.endpoint_id === endpointId)
    if (!endpoint) return
    const capturedEndpoint = capturePayload(endpoint) as ProviderEndpoint
    try {
      await asyncAction.run(
        endpointActionKey(endpointId),
        async () => {
          const saved = await setEndpointEnabled(capturedEndpoint, enabled)
          patchEndpointPages(queryClient, saved)
          await invalidateModelRoutePages()
          return saved
        },
        'toggle',
      )
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function testEndpoint(
    endpointId: string,
    surface: TestResultSurface,
  ): Promise<void> {
    const capturedId = endpointId
    const capturedSurface = surface
    try {
      const outcome = await asyncAction.run(
        networkTestKey('endpoint', capturedId),
        () => testEndpointRecord(capturedId),
        'test',
      )
      if (!outcome.executed) return
      setTestResults((current) => ({ ...current, [capturedId]: outcome.value }))
      setOpenTestResultKey(testResultKey(capturedSurface, capturedId))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function refresh(): Promise<void> {
    const reads: Promise<unknown>[] = [endpointPageQuery.refetch()]
    if (isAdmin) reads.push(userOptionsQuery.refetch())
    await Promise.all(reads)
  }

  const dialogPending = asyncAction.isPending(
    endpointActionKey(dialogEndpointId),
  )

  if (!isAdmin) return null

  return (
    <section
      className="grid min-w-0 gap-3"
      data-endpoint-building-block="upstreams"
      data-foundation-route-id="R-ENDPOINTS-UPSTREAMS"
    >
      <HeaderActions>
        <Button
          aria-label={t('refresh')}
          onClick={() => void refresh()}
          size="sm"
          type="button"
          variant="outline"
        >
          {endpointPageQuery.isFetching || userOptionsQuery.isFetching ? (
            <span
              aria-hidden="true"
              className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent"
            />
          ) : null}
          {t('refresh')}
        </Button>
        <Button
          onClick={openCreateDialog}
          size="sm"
          type="button"
          disabled={asyncAction.isPending(endpointActionKey(null))}
        >
          {t('newEndpoint')}
        </Button>
      </HeaderActions>
      <EndpointsTable
        first={endpointPageQuery.data?.first ?? first}
        items={items}
        loading={endpointPageQuery.isPending}
        onDelete={(endpointId) => void deleteEndpoint(endpointId)}
        onEdit={openEditDialog}
        onPage={changePage}
        onResultOpenChange={changeTestResultOpen}
        onTest={(endpointId, surface) => void testEndpoint(endpointId, surface)}
        onTokenPlanUsage={openTokenPlanUsage}
        onToggle={(endpointId, enabled) =>
          void toggleEndpoint(endpointId, enabled)
        }
        openResultKey={openTestResultKey}
        pendingActions={asyncAction.pendingActions}
        rows={endpointPageQuery.data?.rows ?? rows}
        total={endpointPageQuery.data?.total ?? 0}
      />
      {dialogForm ? (
        <EndpointDialog
          dialogGeneration={dialogGenerationRef.current}
          endpointId={dialogEndpointId}
          form={dialogForm}
          onChange={updateDialogForm}
          onOpenChange={changeDialogOpen}
          onSubmit={() => void submitEndpoint()}
          open={dialogOpen}
          pending={dialogPending}
          runOAuthAction={runOAuthAction}
          users={users}
        />
      ) : null}
      {tokenPlanDialogTarget ? (
        <TokenPlanUsageDialog
          endpointId={tokenPlanDialogTarget.endpointId}
          endpointName={tokenPlanDialogTarget.endpointName}
          onError={reportApiError}
          onOpenChange={changeTokenPlanDialogOpen}
          open
          provider={tokenPlanDialogTarget.provider}
        />
      ) : null}
    </section>
  )
}
