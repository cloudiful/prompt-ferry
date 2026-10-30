import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
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
  fetchMcpCatalog,
  fetchMcpProviders,
  fetchMcpServerPage,
  fetchMcpUsers,
  mcpActionKey,
  mcpCatalogFromTest,
  mcpLastPageFirst,
  mcpProvidersQueryKey,
  mcpServerPageQueryKey,
  mcpTestKey,
  mcpUserOptionsQueryKey,
  refreshMcpServerPage,
  removeMcpServerRecord,
  saveMcpServerRecord,
  runMcpServerTest,
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

type CatalogCache = Record<string, McpCatalogResponse>

export function McpPage() {
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const asyncAction = useAsyncAction()
  const { notifyApiError, notifySuccess } = useNotifier()
  const me = useSessionStore((state) => state.me)
  const userId = me?.user_id ?? 0
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
  const [, setCatalogs] = useState<CatalogCache>({})
  const catalogsRef = useRef<CatalogCache>({})
  const catalogRevisionRef = useRef<Record<string, number>>({})
  const [dialogOpen, setDialogOpen] = useState(false)
  const [dialogForm, setDialogForm] = useState<McpForm | null>(null)
  const [dialogCatalog, setDialogCatalog] =
    useState<McpCatalogResponse>(EMPTY_MCP_CATALOG)
  const [dialogCatalogLoading, setDialogCatalogLoading] = useState(false)
  const [dialogCatalogError, setDialogCatalogError] = useState(false)
  const firstRef = useRef(first)
  const rowsRef = useRef(rows)
  const dialogOpenRef = useRef(false)
  const dialogServerIdRef = useRef<string | null>(null)
  const dialogGenerationRef = useRef(0)
  const notifiedErrors = useRef(new WeakSet<object>())

  firstRef.current = first
  rowsRef.current = rows

  const serverPageQuery = useQuery({
    enabled: me !== null,
    queryKey: mcpServerPageQueryKey(first, rows, userId, isAdmin),
    queryFn: () => fetchMcpServerPage(first, rows),
  })
  const providersQuery = useQuery({
    enabled: me !== null,
    queryKey: mcpProvidersQueryKey(),
    queryFn: fetchMcpProviders,
  })
  const usersQuery = useQuery({
    enabled: isAdmin && me !== null,
    queryKey: mcpUserOptionsQueryKey(userId),
    queryFn: fetchMcpUsers,
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
    if (serverPageQuery.error) reportApiError(serverPageQuery.error)
  }, [reportApiError, serverPageQuery.error])
  useEffect(() => {
    if (providersQuery.error) reportApiError(providersQuery.error)
  }, [providersQuery.error, reportApiError])
  useEffect(() => {
    if (usersQuery.error) reportApiError(usersQuery.error)
  }, [reportApiError, usersQuery.error])

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
  const users = usersQuery.data ?? EMPTY_USERS
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

  function updateCatalogCache(
    serverId: string,
    catalog: McpCatalogResponse,
  ): void {
    const next = { ...catalogsRef.current, [serverId]: catalog }
    catalogsRef.current = next
    setCatalogs(next)
  }

  function invalidateCatalog(serverId: string): void {
    const next = { ...catalogsRef.current }
    delete next[serverId]
    catalogsRef.current = next
    setCatalogs(next)
    catalogRevisionRef.current[serverId] =
      (catalogRevisionRef.current[serverId] ?? 0) + 1
  }

  function setDialogCatalogFor(
    serverId: string,
    generation: number,
    catalog: McpCatalogResponse,
  ): void {
    if (
      dialogOpenRef.current &&
      dialogGenerationRef.current === generation &&
      dialogServerIdRef.current === serverId
    ) {
      setDialogCatalog(catalog)
      setDialogCatalogLoading(false)
      setDialogCatalogError(false)
    }
  }

  function loadDialogCatalog(
    server: McpServer,
    generation: number,
    force = false,
  ): void {
    const serverId = server.server_id
    if (!server.enabled) {
      setDialogCatalog(EMPTY_MCP_CATALOG)
      setDialogCatalogLoading(false)
      setDialogCatalogError(false)
      return
    }
    if (!force && catalogsRef.current[serverId]) {
      setDialogCatalog(catalogsRef.current[serverId]!)
      setDialogCatalogLoading(false)
      setDialogCatalogError(false)
      return
    }

    const revision = (catalogRevisionRef.current[serverId] ?? 0) + 1
    catalogRevisionRef.current[serverId] = revision
    setDialogCatalog(EMPTY_MCP_CATALOG)
    setDialogCatalogLoading(true)
    setDialogCatalogError(false)
    void fetchMcpCatalog(serverId)
      .then((catalog) => {
        if (catalogRevisionRef.current[serverId] !== revision) return
        updateCatalogCache(serverId, catalog)
        setDialogCatalogFor(serverId, generation, catalog)
      })
      .catch((cause) => {
        if (catalogRevisionRef.current[serverId] !== revision) return
        if (
          dialogOpenRef.current &&
          dialogGenerationRef.current === generation &&
          dialogServerIdRef.current === serverId
        ) {
          setDialogCatalogLoading(false)
          setDialogCatalogError(true)
          reportApiError(cause)
        }
      })
  }

  function openCreateDialog(): void {
    dialogGenerationRef.current += 1
    dialogServerIdRef.current = null
    dialogOpenRef.current = true
    setDialogCatalog(EMPTY_MCP_CATALOG)
    setDialogCatalogLoading(false)
    setDialogCatalogError(false)
    setDialogForm(() => {
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
    const generation = dialogGenerationRef.current + 1
    dialogGenerationRef.current = generation
    dialogServerIdRef.current = server.server_id
    dialogOpenRef.current = true
    setDialogForm(mcpServerToForm(server))
    setDialogCatalog(catalogsRef.current[server.server_id] ?? EMPTY_MCP_CATALOG)
    setDialogCatalogError(false)
    setDialogCatalogLoading(
      server.enabled && !catalogsRef.current[server.server_id],
    )
    setDialogOpen(true)
    loadDialogCatalog(server, generation)
  }

  function closeDialog(): void {
    const serverId = dialogServerIdRef.current
    if (asyncAction.isPending(mcpActionKey(serverId))) return
    dialogGenerationRef.current += 1
    dialogServerIdRef.current = null
    dialogOpenRef.current = false
    setDialogOpen(false)
    setDialogForm(null)
    setDialogCatalog(EMPTY_MCP_CATALOG)
    setDialogCatalogLoading(false)
    setDialogCatalogError(false)
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

  async function submitServer(): Promise<void> {
    const currentForm = dialogForm
    if (!currentForm) return
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
          const saved = await saveMcpServerRecord(capturedServerId, body)
          if (capturedServerId) invalidateCatalog(capturedServerId)
          const refreshed = await refreshMcpServerPage(
            queryClient,
            capturedFirst,
            capturedRows,
            userId,
            isAdmin,
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
          const updated = await saveMcpServerRecord(
            capturedServer.server_id,
            body,
          )
          invalidateCatalog(capturedServer.server_id)
          const refreshed = await refreshMcpServerPage(
            queryClient,
            capturedFirst,
            capturedRows,
            userId,
            isAdmin,
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
          await removeMcpServerRecord(serverId)
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
            userId,
            isAdmin,
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
        () => runMcpServerTest(serverId),
        'test',
      )
      if (!outcome.executed) return
      const result = outcome.value
      const catalog = mcpCatalogFromTest(result)
      setTestResults((current) => ({ ...current, [serverId]: result }))
      setOpenTestResultKey(testResultKey(capturedSurface, serverId))
      updateCatalogCache(serverId, catalog)
      catalogRevisionRef.current[serverId] =
        (catalogRevisionRef.current[serverId] ?? 0) + 1
      if (dialogOpenRef.current && dialogServerIdRef.current === serverId) {
        setDialogCatalog(catalog)
        setDialogCatalogLoading(false)
        setDialogCatalogError(false)
      }
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
        onPageChange={changePage}
        onResultOpenChange={changeTestResultOpen}
        onTest={(server, surface) => void testServer(server, surface)}
        onToggle={(server) => void toggleServer(server)}
        openResultKey={openTestResultKey}
        pendingActions={pendingActions}
        rows={currentPage?.rows ?? rows}
        total={currentPage?.total ?? 0}
      />
      {dialogForm ? (
        <McpDialog
          catalog={dialogCatalog}
          catalogError={dialogCatalogError}
          catalogLoading={dialogCatalogLoading}
          form={dialogForm}
          header={dialogForm.server_id ? t('editMcpServer') : t('newMcpServer')}
          isAdmin={isAdmin}
          onChange={setDialogForm}
          onOpenChange={changeDialogOpen}
          onRefreshCatalog={() => {
            const serverId = dialogServerIdRef.current
            if (!serverId) return
            const server = servers.find((item) => item.server_id === serverId)
            if (server)
              loadDialogCatalog(server, dialogGenerationRef.current, true)
          }}
          onSave={() => void submitServer()}
          open={dialogOpen}
          providers={providers}
          savePending={asyncAction.isPending(
            mcpActionKey(dialogForm.server_id || null),
          )}
          users={users}
        />
      ) : null}
    </section>
  )
}
