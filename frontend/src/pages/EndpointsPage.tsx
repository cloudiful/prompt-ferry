import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
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
  createEndpointMutation,
  deleteEndpointMutation,
  listEndpointsOptions,
  listUserOptionsOptions,
  testEndpointMutation,
  updateEndpointMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import {
  createEmptyEndpointForm,
  endpointFormToRequest,
  endpointToForm,
} from '@/admin-mappers/forms/endpoint'
import { capturePayload } from '@/stores/record-action-types'
import {
  buildEndpointEnabledRequest,
  endpointRecordsQueryKey,
  modelRoutePagesQueryKey,
  patchEndpointPages,
  refreshEndpointsPage,
} from '@/lib/endpoints'
import {
  prefetchTokenPlanUsageBatch,
  refreshTokenPlanUsageBatch,
} from '@/lib/quota-cache'
import { useAsyncAction } from '@/lib/use-async-action'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import { isQuotaEligible } from '@/models/endpoints/quota'
import { createEndpointListItemView } from '@/models/endpoints/endpoint-item'
import {
  ENDPOINTS_REFRESH_KEY,
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
  plan: ProviderEndpoint['plan']
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
  const [dialogInitialForm, setDialogInitialForm] =
    useState<EndpointForm | null>(null)
  const [tokenPlanDialogTarget, setTokenPlanDialogTarget] =
    useState<TokenPlanDialogTarget | null>(null)
  const firstRef = useRef(first)
  const rowsRef = useRef(rows)
  const dialogOpenRef = useRef(false)
  const dialogEndpointIdRef = useRef<string | null>(null)
  const dialogGenerationRef = useRef(0)
  const notifiedErrors = useRef(new WeakSet<object>())

  firstRef.current = first
  rowsRef.current = rows

  const endpointPageQuery = useQuery({
    ...listEndpointsOptions({ query: { first, rows } }),
    enabled: isAdmin,
  })
  const userOptionsQuery = useQuery({
    ...listUserOptionsOptions(),
    enabled: isAdmin && me !== null,
  })

  const createEndpoint = useMutation(createEndpointMutation())
  const updateEndpoint = useMutation(updateEndpointMutation())
  const deleteEndpointRecord = useMutation(deleteEndpointMutation())
  const testEndpoint = useMutation(testEndpointMutation())

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
  const users = userOptionsQuery.data?.users ?? []
  const eligibleQuotaIds = useMemo(
    () =>
      endpoints.filter(isQuotaEligible).map((endpoint) => endpoint.endpoint_id),
    [endpoints],
  )
  useEffect(() => {
    if (!isAdmin || eligibleQuotaIds.length === 0) return
    let active = true
    void prefetchTokenPlanUsageBatch(
      queryClient,
      eligibleQuotaIds,
      4,
      () => active,
    )
    return () => {
      active = false
    }
  }, [eligibleQuotaIds, isAdmin, queryClient])
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
      plan: endpoint.plan,
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
    setDialogInitialForm(createEmptyEndpointForm())
    setDialogOpen(true)
  }

  function openEditDialog(endpointId: string): void {
    const endpoint = endpoints.find((item) => item.endpoint_id === endpointId)
    if (!endpoint) return
    dialogGenerationRef.current += 1
    dialogEndpointIdRef.current = endpointId
    dialogOpenRef.current = true
    setDialogEndpointId(endpointId)
    setDialogInitialForm(endpointToForm(endpoint))
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
      setDialogInitialForm(null)
    }
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

  async function submitEndpoint(currentForm: EndpointForm): Promise<void> {
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
          const saved = capturedId
            ? await updateEndpoint.mutateAsync({
                path: { endpoint_id: capturedId },
                body,
              })
            : await createEndpoint.mutateAsync({ body })
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
        setDialogInitialForm(null)
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
          await deleteEndpointRecord.mutateAsync({
            path: { endpoint_id: capturedId },
          })
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
          const saved = await updateEndpoint.mutateAsync({
            path: { endpoint_id: capturedEndpoint.endpoint_id },
            body: buildEndpointEnabledRequest(capturedEndpoint, enabled),
          })
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

  async function probeEndpoint(
    endpointId: string,
    surface: TestResultSurface,
  ): Promise<void> {
    const capturedId = endpointId
    const capturedSurface = surface
    try {
      const outcome = await asyncAction.run(
        networkTestKey('endpoint', capturedId),
        () => testEndpoint.mutateAsync({ path: { endpoint_id: capturedId } }),
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
    const capturedQuotaIds = endpoints
      .filter(isQuotaEligible)
      .map((endpoint) => endpoint.endpoint_id)
    try {
      const outcome = await asyncAction.run(
        ENDPOINTS_REFRESH_KEY,
        async () => {
          const reads: Promise<unknown>[] = [endpointPageQuery.refetch()]
          if (isAdmin) reads.push(userOptionsQuery.refetch())
          const [readResults, quotaOutcomes] = await Promise.all([
            Promise.allSettled(reads),
            refreshTokenPlanUsageBatch(queryClient, capturedQuotaIds),
          ])
          return { readResults, quotaOutcomes }
        },
        'refresh',
      )
      if (!outcome.executed) return
      for (const result of outcome.value.readResults) {
        if (result.status === 'rejected') reportApiError(result.reason)
      }
      for (const result of outcome.value.quotaOutcomes) {
        if (result.failed) reportApiError(result.cause)
      }
    } catch (cause) {
      reportApiError(cause)
    }
  }

  const dialogPending = asyncAction.isPending(
    endpointActionKey(dialogEndpointId),
  )
  const refreshing = asyncAction.isPending(ENDPOINTS_REFRESH_KEY)

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
          disabled={refreshing}
          onClick={() => void refresh()}
          size="sm"
          type="button"
          variant="outline"
        >
          {refreshing ? (
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
        onTest={(endpointId, surface) =>
          void probeEndpoint(endpointId, surface)
        }
        onTokenPlanUsage={openTokenPlanUsage}
        onToggle={(endpointId, enabled) =>
          void toggleEndpoint(endpointId, enabled)
        }
        openResultKey={openTestResultKey}
        pendingActions={asyncAction.pendingActions}
        rows={endpointPageQuery.data?.rows ?? rows}
        total={endpointPageQuery.data?.total ?? 0}
      />
      {dialogInitialForm ? (
        <EndpointDialog
          dialogGeneration={dialogGenerationRef.current}
          endpointId={dialogEndpointId}
          initialForm={dialogInitialForm}
          key={dialogGenerationRef.current}
          onOpenChange={changeDialogOpen}
          onSubmit={(form) => submitEndpoint(form)}
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
          plan={tokenPlanDialogTarget.plan}
        />
      ) : null}
    </section>
  )
}
