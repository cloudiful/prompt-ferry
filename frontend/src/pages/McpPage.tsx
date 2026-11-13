import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { LoaderCircle, Plus, RefreshCw } from 'lucide-react'
import {
  createEmptyMcpForm,
  mcpFormToRequest,
  mcpServerToForm,
} from '@/admin-mappers/forms/mcp'
import type {
  McpCatalogResponse,
  McpProviderDescriptor,
  McpServer,
  McpTestResponse,
  User,
} from '@/generated/admin-api'
import {
  createMcpServerMutation,
  deleteMcpServerMutation,
  disconnectMcpOauthMutation,
  getMcpCatalogOptions,
  getMcpCatalogQueryKey,
  listMcpProvidersOptions,
  listMcpServersOptions,
  listUserOptionsOptions,
  startMcpOauthMutation,
  testMcpServerMutation,
  updateMcpServerMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import type { McpForm } from '@/models'
import { McpDialog } from '@/components/mcp/McpDialog'
import { McpTable } from '@/components/mcp/McpTable'
import {
  testResultKey,
  type TestResultSurface,
} from '@/components/shared/TestResultPopover'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import {
  EMPTY_MCP_CATALOG,
  mcpActionKey,
  mcpCatalogFromTest,
  mcpLastPageFirst,
  mcpOAuthStatusQueryKey,
  mcpTestKey,
  refreshMcpServerPage,
} from '@/lib/mcp'
import { useAsyncAction } from '@/lib/use-async-action'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import { capturePayload } from '@/stores/record-action-types'
import { createMcpWorkspaceView } from '@/models/mcp'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  type TablePageChange,
} from '@/models/pagination'
import { useStoredPageSize } from '@/components/shared/TablePagination'

const EMPTY_SERVERS: McpServer[] = []
const EMPTY_PROVIDERS: McpProviderDescriptor[] = []
const EMPTY_USERS: User[] = []

type OwnedOAuthPopup = {
  popup: Window
  serverId: string
  generation: number
}

type ConnectPreparation =
  | { kind: 'blocked' }
  | { kind: 'isolation-failed' }
  | { kind: 'opened'; popup: Window; url: string; entry: OwnedOAuthPopup }

function closeWindow(popup: Window): void {
  try {
    popup.close()
  } catch {}
}

function navigateOwnedOAuthPopup(popup: Window, url: string): void {
  popup.location.href = url
  popup.opener = null
}

export function McpPage() {
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const asyncAction = useAsyncAction()
  const { notifyApiError, notifyError, notifySuccess } = useNotifier()
  const me = useSessionStore((state) => state.me)
  const isAdmin = me?.is_admin === true
  const [first, setFirst] = useState(0)
  const [rows, setRows] = useStoredPageSize(
    'mcp-servers',
    10,
    STANDARD_PAGE_SIZE_OPTIONS,
  )
  const [testResults, setTestResults] = useState<
    Record<string, McpTestResponse>
  >({})
  const [openTestResultKey, setOpenTestResultKey] = useState<string | null>(
    null,
  )
  const [dialogOpen, setDialogOpen] = useState(false)
  const [dialogInitialForm, setDialogInitialForm] = useState<McpForm | null>(
    null,
  )
  const [dialogServer, setDialogServer] = useState<McpServer | null>(null)
  const firstRef = useRef(first)
  const rowsRef = useRef(rows)
  const dialogOpenRef = useRef(false)
  const dialogServerIdRef = useRef<string | null>(null)
  const dialogGenerationRef = useRef(0)
  const notifiedErrors = useRef(new WeakSet<object>())
  const oauthPopupsRef = useRef(new Map<string, OwnedOAuthPopup>())
  const mountedRef = useRef(true)

  firstRef.current = first
  rowsRef.current = rows

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
      for (const entry of oauthPopupsRef.current.values()) {
        closeWindow(entry.popup)
      }
      oauthPopupsRef.current.clear()
    }
  }, [])

  const serverPageQuery = useQuery({
    ...listMcpServersOptions({ query: { first, rows } }),
    enabled: me !== null,
  })
  const providersQuery = useQuery({
    ...listMcpProvidersOptions(),
    enabled: me !== null,
  })
  const usersQuery = useQuery({
    ...listUserOptionsOptions(),
    enabled: isAdmin && me !== null,
  })

  const catalogServerId = dialogServer?.server_id ?? ''
  const catalogEnabled =
    dialogOpen && dialogServer?.enabled === true && catalogServerId !== ''
  const catalogQuery = useQuery({
    ...getMcpCatalogOptions({ path: { server_id: catalogServerId } }),
    enabled: catalogEnabled,
    staleTime: Infinity,
  })

  const createServer = useMutation(createMcpServerMutation())
  const updateServer = useMutation(updateMcpServerMutation())
  const removeServer = useMutation(deleteMcpServerMutation())
  const probeServer = useMutation(testMcpServerMutation())
  const startOauth = useMutation(startMcpOauthMutation())
  const disconnectOauth = useMutation(disconnectMcpOauthMutation())

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
    if (serverPageQuery.error) reportApiError(serverPageQuery.error)
  }, [reportApiError, serverPageQuery.error])
  useEffect(() => {
    if (providersQuery.error) reportApiError(providersQuery.error)
  }, [providersQuery.error, reportApiError])
  useEffect(() => {
    if (usersQuery.error) reportApiError(usersQuery.error)
  }, [reportApiError, usersQuery.error])
  useEffect(() => {
    if (catalogQuery.error) reportApiError(catalogQuery.error)
  }, [catalogQuery.error, reportApiError])

  const page = serverPageQuery.data
  useEffect(() => {
    if (serverPageQuery.isFetching || !page || page.rows <= 0) return
    if (page.servers.length === 0 && page.first >= page.total) {
      const correctedFirst = mcpLastPageFirst(page.total, page.rows)
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
  }, [first, page, rows, serverPageQuery.isFetching, setRows])

  const servers = page?.servers ?? EMPTY_SERVERS
  const providers = providersQuery.data ?? EMPTY_PROVIDERS
  const users = usersQuery.data?.users ?? EMPTY_USERS
  const labels = useMemo(
    () => ({
      aggregateNamingModePassthroughPreferredShort: t(
        'aggregateNamingModePassthroughPreferredShort',
      ),
      aggregateNamingModeQualifiedOnlyShort: t(
        'aggregateNamingModeQualifiedOnlyShort',
      ),
      managedMinimax: t('minimaxManaged'),
      privateScope: t('privateScope'),
      publicScope: t('publicScope'),
      providerGenericLabel: t('providerGeneric'),
      providerLabels: Object.fromEntries(
        providers.map((provider) => [provider.id, provider.display_name]),
      ),
      hostedProviderIds: providers
        .filter((provider) => provider.default_url != null)
        .map((provider) => provider.id),
    }),
    [providers, t],
  )
  const workspace = useMemo(
    () =>
      createMcpWorkspaceView(servers, {
        labels,
        testResults,
        total: page?.total ?? 0,
      }),
    [labels, page?.total, servers, testResults],
  )
  const pendingActions = asyncAction.pendingActions

  const catalog: McpCatalogResponse = catalogEnabled
    ? (catalogQuery.data ?? EMPTY_MCP_CATALOG)
    : EMPTY_MCP_CATALOG
  const catalogLoading = catalogEnabled && catalogQuery.isFetching
  const catalogError = catalogEnabled && catalogQuery.isError

  function invalidateCatalog(serverId: string): void {
    void queryClient.invalidateQueries({
      queryKey: getMcpCatalogQueryKey({ path: { server_id: serverId } }),
      exact: true,
      refetchType: 'none',
    })
  }

  function openCreateDialog(): void {
    dialogGenerationRef.current += 1
    dialogServerIdRef.current = null
    dialogOpenRef.current = true
    setDialogServer(null)
    setDialogInitialForm(() => {
      const form = createEmptyMcpForm()
      if (!isAdmin) {
        form.scope = 'user'
        form.owner_user_id = me?.user_id ?? null
      }
      return form
    })
    setDialogOpen(true)
  }

  function openEditDialog(server: McpServer): void {
    dialogGenerationRef.current += 1
    dialogServerIdRef.current = server.server_id
    dialogOpenRef.current = true
    setDialogServer(server)
    setDialogInitialForm(mcpServerToForm(server))
    setDialogOpen(true)
  }

  function closeDialog(): void {
    const serverId = dialogServerIdRef.current
    if (asyncAction.isPending(mcpActionKey(serverId))) return
    dialogGenerationRef.current += 1
    dialogServerIdRef.current = null
    dialogOpenRef.current = false
    setDialogOpen(false)
    setDialogInitialForm(null)
    setDialogServer(null)
  }

  function changeDialogOpen(open: boolean): void {
    if (!open) {
      closeDialog()
      return
    }
    setDialogOpen(true)
    dialogOpenRef.current = true
  }

  const setPageAfterRefresh = useCallback(
    (
      refreshed: { first: number; rows: number },
      capturedFirst: number,
      capturedRows: number,
    ) => {
      if (
        firstRef.current !== capturedFirst ||
        rowsRef.current !== capturedRows
      )
        return
      firstRef.current = refreshed.first
      rowsRef.current = refreshed.rows
      setFirst(refreshed.first)
      setRows(refreshed.rows)
    },
    [setRows],
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

  function invalidateOAuthStatus(serverId: string): Promise<void> {
    return queryClient.invalidateQueries({
      queryKey: mcpOAuthStatusQueryKey(serverId),
    })
  }

  function closeOwnedOAuthPopup(
    serverId: string,
    entry?: OwnedOAuthPopup,
  ): void {
    const tracked = oauthPopupsRef.current.get(serverId)
    if (!tracked || (entry && tracked !== entry)) return
    oauthPopupsRef.current.delete(serverId)
    closeWindow(tracked.popup)
  }

  function oauthActionContextIsCurrent(
    serverId: string,
    generation: number,
  ): boolean {
    return (
      mountedRef.current &&
      dialogOpenRef.current &&
      dialogGenerationRef.current === generation &&
      dialogServerIdRef.current === serverId
    )
  }

  function ownedOAuthPopupIsCurrent(entry: OwnedOAuthPopup): boolean {
    return (
      oauthActionContextIsCurrent(entry.serverId, entry.generation) &&
      entry.popup.opener === null &&
      !entry.popup.closed
    )
  }

  async function connectOAuth(server: McpServer): Promise<void> {
    const serverId = server.server_id
    if (asyncAction.isPending(mcpActionKey(serverId))) return
    const generation = dialogGenerationRef.current
    try {
      const outcome = await asyncAction.run(
        mcpActionKey(serverId),
        async (): Promise<ConnectPreparation> => {
          const popup = window.open('about:blank', '_blank')
          if (!popup) return { kind: 'blocked' }
          try {
            popup.opener = null
            if (popup.opener !== null) throw new Error('opener retained')
          } catch {
            closeWindow(popup)
            return { kind: 'isolation-failed' }
          }
          closeOwnedOAuthPopup(serverId)
          const entry: OwnedOAuthPopup = { popup, serverId, generation }
          oauthPopupsRef.current.set(serverId, entry)
          try {
            const started = await startOauth.mutateAsync({
              path: { server_id: serverId },
            })
            await invalidateOAuthStatus(serverId)
            return {
              kind: 'opened',
              popup,
              url: started.authorization_url,
              entry,
            }
          } catch (cause) {
            closeOwnedOAuthPopup(serverId, entry)
            throw cause
          }
        },
        'oauth-connect',
      )
      if (!outcome.executed) return
      const prepared = outcome.value
      const contextCurrent = oauthActionContextIsCurrent(serverId, generation)
      if (prepared.kind === 'blocked') {
        if (contextCurrent) notifyError(t('oauthPopupBlocked'))
        return
      }
      if (prepared.kind === 'isolation-failed') {
        if (contextCurrent) notifyError(t('oauthPopupUnsafe'))
        return
      }
      if (!ownedOAuthPopupIsCurrent(prepared.entry)) {
        closeOwnedOAuthPopup(serverId, prepared.entry)
        return
      }
      try {
        navigateOwnedOAuthPopup(prepared.popup, prepared.url)
      } catch {
        closeOwnedOAuthPopup(serverId, prepared.entry)
        if (contextCurrent) notifyError(t('oauthPopupBlocked'))
      }
    } catch (cause) {
      if (oauthActionContextIsCurrent(serverId, generation)) {
        reportApiError(cause)
      }
    }
  }

  async function disconnectOAuth(server: McpServer): Promise<void> {
    const serverId = server.server_id
    const generation = dialogGenerationRef.current
    try {
      const outcome = await asyncAction.run(
        mcpActionKey(serverId),
        async () => {
          await disconnectOauth.mutateAsync({
            path: { server_id: serverId },
          })
          await invalidateOAuthStatus(serverId)
        },
        'oauth-disconnect',
      )
      if (!outcome.executed) return
      closeOwnedOAuthPopup(serverId)
      if (oauthActionContextIsCurrent(serverId, generation)) {
        notifySuccess(t('oauthDisconnect'))
      }
    } catch (cause) {
      if (oauthActionContextIsCurrent(serverId, generation)) {
        reportApiError(cause)
      }
    }
  }

  async function submitServer(currentForm: McpForm): Promise<void> {
    const serverId = currentForm.server_id || null
    const capturedServerId = serverId
    const generation = dialogGenerationRef.current
    const capturedFirst = firstRef.current
    const capturedRows = rowsRef.current
    try {
      const body = capturePayload(mcpFormToRequest(currentForm))
      const outcome = await asyncAction.run(
        mcpActionKey(capturedServerId),
        async () => {
          const saved = capturedServerId
            ? await updateServer.mutateAsync({
                path: { server_id: capturedServerId },
                body,
              })
            : await createServer.mutateAsync({ body })
          if (capturedServerId) {
            invalidateCatalog(capturedServerId)
            void invalidateOAuthStatus(capturedServerId)
          }
          const refreshed = await refreshMcpServerPage(
            queryClient,
            capturedFirst,
            capturedRows,
          )
          setPageAfterRefresh(refreshed, capturedFirst, capturedRows)
          return saved
        },
        'save',
      )
      if (!outcome.executed) return
      if (
        dialogOpenRef.current &&
        dialogGenerationRef.current === generation &&
        dialogServerIdRef.current === capturedServerId
      ) {
        closeDialog()
      }
      notifySuccess(t('saved'))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function toggleServer(server: McpServer): Promise<void> {
    const capturedServer = capturePayload(server)
    const capturedFirst = firstRef.current
    const capturedRows = rowsRef.current
    try {
      const form = mcpServerToForm(capturedServer)
      form.enabled = !form.enabled
      const body = capturePayload(mcpFormToRequest(form))
      const outcome = await asyncAction.run(
        mcpActionKey(capturedServer.server_id),
        async () => {
          const updated = await updateServer.mutateAsync({
            path: { server_id: capturedServer.server_id },
            body,
          })
          invalidateCatalog(capturedServer.server_id)
          const refreshed = await refreshMcpServerPage(
            queryClient,
            capturedFirst,
            capturedRows,
          )
          setPageAfterRefresh(refreshed, capturedFirst, capturedRows)
          return updated
        },
        'toggle',
      )
      if (outcome.executed) notifySuccess(t('saved'))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function deleteServer(server: McpServer): Promise<void> {
    if (!window.confirm(`${t('delete')} ${server.name}?`)) return
    const serverId = server.server_id
    const capturedFirst = firstRef.current
    const capturedRows = rowsRef.current
    try {
      const outcome = await asyncAction.run(
        mcpActionKey(serverId),
        async () => {
          await removeServer.mutateAsync({ path: { server_id: serverId } })
          invalidateCatalog(serverId)
          setTestResults((current) => {
            const next = { ...current }
            delete next[serverId]
            return next
          })
          const refreshed = await refreshMcpServerPage(
            queryClient,
            capturedFirst,
            capturedRows,
          )
          setPageAfterRefresh(refreshed, capturedFirst, capturedRows)
        },
        'delete',
      )
      if (outcome.executed) notifySuccess(t('delete'))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function testServer(
    server: McpServer,
    surface: TestResultSurface,
  ): Promise<void> {
    const serverId = server.server_id
    const capturedSurface = surface
    try {
      const outcome = await asyncAction.run(
        mcpTestKey(serverId),
        () => probeServer.mutateAsync({ path: { server_id: serverId } }),
        'test',
      )
      if (!outcome.executed) return
      const result = outcome.value
      const catalog = mcpCatalogFromTest(result)
      setTestResults((current) => ({ ...current, [serverId]: result }))
      setOpenTestResultKey(testResultKey(capturedSurface, serverId))
      queryClient.setQueryData(
        getMcpCatalogQueryKey({ path: { server_id: serverId } }),
        catalog,
      )
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function refreshPage(): Promise<void> {
    const reads: Promise<unknown>[] = [
      serverPageQuery.refetch(),
      providersQuery.refetch(),
    ]
    if (isAdmin) reads.push(usersQuery.refetch())
    await Promise.all(reads)
  }

  const pageError = serverPageQuery.error
  const currentPage = page

  return (
    <section className="grid min-w-0 gap-3" data-foundation-route-id="R-MCP">
      <HeaderActions>
        <Button
          disabled={asyncAction.isPending(mcpActionKey(null))}
          onClick={openCreateDialog}
          type="button"
        >
          <Plus aria-hidden="true" />
          {t('newMcpServer')}
        </Button>
        <Button
          aria-label={t('refresh')}
          disabled={
            serverPageQuery.isFetching ||
            providersQuery.isFetching ||
            (isAdmin && usersQuery.isFetching)
          }
          onClick={() => void refreshPage()}
          type="button"
          variant="outline"
        >
          {serverPageQuery.isFetching || providersQuery.isFetching ? (
            <LoaderCircle aria-hidden="true" className="animate-spin" />
          ) : (
            <RefreshCw aria-hidden="true" />
          )}
          {t('refresh')}
        </Button>
      </HeaderActions>
      {pageError ? (
        <div
          className="grid gap-2 rounded-lg border border-destructive/40 p-3"
          role="alert"
        >
          <span>{t('error')}</span>
          <Button
            onClick={() => void serverPageQuery.refetch()}
            size="sm"
            type="button"
            variant="outline"
          >
            {t('retry')}
          </Button>
        </div>
      ) : null}
      {providersQuery.error ? (
        <div
          className="grid gap-2 rounded-lg border border-destructive/40 p-3"
          role="alert"
        >
          <span>{t('error')}</span>
          <Button
            onClick={() => void providersQuery.refetch()}
            size="sm"
            type="button"
            variant="outline"
          >
            {t('retry')}
          </Button>
        </div>
      ) : null}
      <McpTable
        busy={serverPageQuery.isFetching}
        first={currentPage?.first ?? first}
        items={workspace.list_items}
        onDelete={(server) => void deleteServer(server)}
        onEdit={openEditDialog}
        onOAuth={openEditDialog}
        onPageChange={changePage}
        onResultOpenChange={changeTestResultOpen}
        onTest={(server, surface) => void testServer(server, surface)}
        onToggle={(server) => void toggleServer(server)}
        openResultKey={openTestResultKey}
        pendingActions={pendingActions}
        rows={currentPage?.rows ?? rows}
        total={currentPage?.total ?? 0}
      />
      {dialogInitialForm ? (
        <McpDialog
          catalog={catalog}
          catalogError={catalogError}
          catalogLoading={catalogLoading}
          header={dialogServer ? t('editMcpServer') : t('newMcpServer')}
          initialForm={dialogInitialForm}
          isAdmin={isAdmin}
          key={dialogGenerationRef.current}
          onOAuthConnect={connectOAuth}
          onOAuthDisconnect={disconnectOAuth}
          oauthPending={asyncAction.isPending(
            mcpActionKey(dialogServer?.server_id ?? null),
          )}
          oauthServer={dialogServer}
          onOpenChange={changeDialogOpen}
          onRefreshCatalog={() => void catalogQuery.refetch()}
          onSave={(form) => void submitServer(form)}
          open={dialogOpen}
          providers={providers}
          savePending={asyncAction.isPending(
            mcpActionKey(dialogServer?.server_id ?? null),
          )}
          users={users}
        />
      ) : null}
    </section>
  )
}
