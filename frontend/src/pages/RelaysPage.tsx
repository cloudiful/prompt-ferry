import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Info, Pencil, Plus, RefreshCw, RotateCw, Trash2 } from 'lucide-react'
import type { ManagedRelay } from '@/generated/admin-api'
import {
  createRelayPatchRequest,
  createRelayRequest,
  type RelayForm,
} from '@/admin-mappers/forms/relay'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import {
  DataTable,
  type DataTableColumnDef,
} from '@/components/shared/DataTable'
import {
  TablePagination,
  useStoredPageSize,
} from '@/components/shared/TablePagination'
import { RelayDialog } from '@/components/relays/RelayDialog'
import { STANDARD_PAGE_SIZE_OPTIONS } from '@/models/pagination'
import { useAsyncAction } from '@/lib/use-async-action'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { usePageTitle } from '@/lib/router'
import { refreshRelayPage, relayActionKey } from '@/lib/relays'
import type { TablePageChange } from '@/models/pagination'
import {
  createRelayMutation,
  deleteRelayMutation,
  listRelaysOptions,
  reconnectRelayMutation,
  updateRelayMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'

export function RelaysPage() {
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const asyncAction = useAsyncAction()
  const { notifyApiError, notifySuccess } = useNotifier()
  const [first, setFirst] = useState(0)
  const [rows, setRows] = useStoredPageSize(
    'relays',
    10,
    STANDARD_PAGE_SIZE_OPTIONS,
  )
  const [dialogOpen, setDialogOpen] = useState(false)
  const [editingRelay, setEditingRelay] = useState<ManagedRelay | null>(null)
  const editingRelayRef = useRef<ManagedRelay | null>(null)
  const dialogGeneration = useRef(0)
  const lastReportedError = useRef<unknown>(null)
  const rowActions = useRef({
    deleteRelay: async (_relay: ManagedRelay) => {},
    openEditDialog: (_relay: ManagedRelay) => {},
    reconnectRelay: async (_relay: ManagedRelay) => {},
  })

  usePageTitle('relays')

  const query = useQuery({
    ...listRelaysOptions({ query: { first, rows } }),
    placeholderData: (previous) => previous,
  })

  const createRelay = useMutation(createRelayMutation())
  const updateRelay = useMutation(updateRelayMutation())
  const removeRelay = useMutation(deleteRelayMutation())
  const reconnectRelay = useMutation(reconnectRelayMutation())

  const reportError = useCallback(
    (cause: unknown) => {
      if (lastReportedError.current === cause) return
      lastReportedError.current = cause
      notifyApiError(cause)
    },
    [notifyApiError],
  )

  useEffect(() => {
    if (query.error) reportError(query.error)
    if (query.isSuccess) lastReportedError.current = null
  }, [query.error, query.isSuccess, reportError])

  useEffect(() => {
    if (!query.isSuccess || query.isPlaceholderData || !query.data) return
    const page = query.data
    if (page.rows > 0 && page.relays.length === 0 && page.first >= page.total) {
      const correctedFirst =
        page.total > 0
          ? Math.floor((page.total - 1) / page.rows) * page.rows
          : 0
      if (correctedFirst !== first) setFirst(correctedFirst)
      return
    }
    if (page.first !== first) setFirst(page.first)
    if (page.rows !== rows) setRows(page.rows)
  }, [
    first,
    query.data,
    query.isPlaceholderData,
    query.isSuccess,
    rows,
    setRows,
  ])

  const rowsData = query.data?.relays ?? []
  const dialogKey = relayActionKey(editingRelay?.relay_id ?? null)
  const dialogPendingAction = asyncAction.pendingOrigin(dialogKey)
  const dialogPending = dialogPendingAction !== null
  const createPending = asyncAction.isPending(relayActionKey(null))

  function changePage(change: TablePageChange): void {
    setFirst(change.first)
    setRows(change.rows)
  }

  function openCreateDialog(): void {
    dialogGeneration.current += 1
    editingRelayRef.current = null
    setEditingRelay(null)
    setDialogOpen(true)
  }

  function openEditDialog(relay: ManagedRelay): void {
    if (asyncAction.isPending(relayActionKey(relay.relay_id))) return
    dialogGeneration.current += 1
    editingRelayRef.current = relay
    setEditingRelay(relay)
    setDialogOpen(true)
  }

  function changeDialogOpen(open: boolean): void {
    if (!open && dialogPending) return
    if (!open) dialogGeneration.current += 1
    setDialogOpen(open)
  }

  async function saveRelay(form: RelayForm): Promise<void> {
    const capturedForm = { ...form }
    const relayId = capturedForm.relay_id || null
    const key = relayActionKey(relayId)
    const generation = dialogGeneration.current
    const actionFirst = first
    const actionRows = rows

    try {
      const outcome = await asyncAction.run(
        key,
        async () => {
          if (relayId) {
            await updateRelay.mutateAsync({
              path: { relay_id: relayId },
              body: createRelayPatchRequest(capturedForm),
            })
          } else {
            await createRelay.mutateAsync({
              body: createRelayRequest(capturedForm),
            })
          }
          const refreshed = await refreshRelayPage(
            queryClient,
            actionFirst,
            actionRows,
          )
          return refreshed
        },
        'save',
      )
      if (
        !outcome.executed ||
        !dialogOpen ||
        dialogGeneration.current !== generation ||
        (editingRelayRef.current?.relay_id || null) !== relayId
      ) {
        return
      }
      setFirst(outcome.value.first)
      setRows(outcome.value.page.rows)
      dialogGeneration.current += 1
      setDialogOpen(false)
      setEditingRelay(null)
      editingRelayRef.current = null
      notifySuccess(t('saved'))
    } catch (cause) {
      if (dialogOpen && dialogGeneration.current === generation) {
        reportError(cause)
      }
    }
  }

  async function deleteRelay(relay: ManagedRelay): Promise<void> {
    const relayId = relay.relay_id
    const relayName = relay.name
    if (!window.confirm(t('relayDeleteConfirm', { name: relayName }))) return
    const key = relayActionKey(relayId)
    const actionFirst = first
    const actionRows = rows

    try {
      const outcome = await asyncAction.run(
        key,
        async () => {
          await removeRelay.mutateAsync({ path: { relay_id: relayId } })
          return refreshRelayPage(queryClient, actionFirst, actionRows)
        },
        'delete',
      )
      if (!outcome.executed) return
      setFirst(outcome.value.first)
      setRows(outcome.value.page.rows)
      notifySuccess(t('delete'))
    } catch (cause) {
      reportError(cause)
    }
  }

  async function reconnectRelayAction(relay: ManagedRelay): Promise<void> {
    if (!relay.enabled) return
    const relayId = relay.relay_id
    const actionFirst = first
    const actionRows = rows

    try {
      const outcome = await asyncAction.run(
        relayActionKey(relayId),
        async () => {
          const result = await reconnectRelay.mutateAsync({
            path: { relay_id: relayId },
          })
          await refreshRelayPage(queryClient, actionFirst, actionRows)
          return result
        },
        'reconnect',
      )
      if (outcome.executed) notifySuccess(t('relayReconnectRequested'))
    } catch (cause) {
      reportError(cause)
    }
  }

  function secretStatus(stored: boolean): string {
    return stored ? t('relaySecretStored') : t('relaySecretMissing')
  }

  rowActions.current = {
    deleteRelay,
    openEditDialog,
    reconnectRelay: reconnectRelayAction,
  }

  const columns = useMemo<Array<DataTableColumnDef<ManagedRelay>>>(
    () => [
      {
        id: 'name',
        header: t('name'),
        cell: ({ row }) => (
          <span
            className="block max-w-48 truncate font-medium"
            title={row.original.name}
          >
            {row.original.name}
          </span>
        ),
      },
      {
        id: 'relay_url',
        header: t('relayUrl'),
        cell: ({ row }) => (
          <span
            className="block max-w-56 truncate"
            title={row.original.relay_url}
          >
            {row.original.relay_url}
          </span>
        ),
      },
      {
        id: 'status',
        header: t('status'),
        cell: ({ row }) => (
          <div className="flex items-center gap-2 whitespace-nowrap">
            <Badge variant={row.original.connected ? 'default' : 'outline'}>
              {row.original.connected
                ? t('relayStatusConnected')
                : t('relayStatusDisconnected')}
            </Badge>
            <span className="text-xs text-muted-foreground">
              {row.original.enabled ? t('active') : t('disabled')}
            </span>
          </div>
        ),
      },
      {
        id: 'tls_mode',
        header: t('relayTlsMode'),
        cell: ({ row }) => row.original.tls_mode,
      },
      {
        id: 'bridge_encryption_mode',
        header: t('relayBridgeMode'),
        cell: ({ row }) => row.original.bridge_encryption_mode,
      },
      {
        id: 'secrets',
        header: t('relaySecrets'),
        cell: ({ row }) => (
          <div className="grid gap-1 whitespace-nowrap text-xs text-muted-foreground">
            <span>
              {t('relaySecretCaLabel')}:{' '}
              {secretStatus(row.original.has_relay_ca)}
            </span>
            <span>
              {t('relaySecretCertLabel')}:{' '}
              {secretStatus(row.original.has_client_cert)}
            </span>
            <span>
              {t('relaySecretKeyLabel')}:{' '}
              {secretStatus(row.original.has_client_key)}
            </span>
            <span>
              {t('relaySecretBridgeLabel')}:{' '}
              {secretStatus(row.original.has_bridge_key)}
            </span>
          </div>
        ),
      },
      {
        id: 'last_error',
        header: t('relayLastError'),
        cell: ({ row }) => (
          <div className="grid max-w-56 gap-1 text-xs">
            <span className="truncate" title={row.original.last_error ?? ''}>
              {row.original.last_error || '—'}
            </span>
            <span className="whitespace-nowrap text-muted-foreground">
              {t('relaySnapshotVersion')}{' '}
              {row.original.last_snapshot_version ?? '—'}
            </span>
          </div>
        ),
      },
      {
        id: 'actions',
        header: t('actions'),
        cell: ({ row }) => {
          const relay = row.original
          const pendingAction = asyncAction.pendingOrigin(
            relayActionKey(relay.relay_id),
          )
          const pending = pendingAction !== null
          return (
            <div className="flex justify-end gap-1 whitespace-nowrap">
              <Button
                aria-label={`${t('relayReconnect')} ${relay.name}`}
                disabled={pending || !relay.enabled}
                onClick={() => void rowActions.current.reconnectRelay(relay)}
                size="sm"
                type="button"
                variant="ghost"
              >
                {pendingAction === 'reconnect' ? (
                  <span className="size-3 animate-spin rounded-full border-2 border-current border-r-transparent" />
                ) : (
                  <RotateCw aria-hidden="true" />
                )}
                <span>{t('relayReconnect')}</span>
              </Button>
              <Button
                aria-label={`${t('edit')} ${relay.name}`}
                disabled={pending}
                onClick={() => rowActions.current.openEditDialog(relay)}
                size="sm"
                type="button"
                variant="ghost"
              >
                <Pencil aria-hidden="true" />
                <span>{t('edit')}</span>
              </Button>
              <Button
                aria-label={`${t('delete')} ${relay.name}`}
                disabled={pending}
                onClick={() => void rowActions.current.deleteRelay(relay)}
                size="sm"
                type="button"
                variant="ghost"
              >
                {pendingAction === 'delete' ? (
                  <span className="size-3 animate-spin rounded-full border-2 border-current border-r-transparent" />
                ) : (
                  <Trash2 aria-hidden="true" />
                )}
                <span>{t('delete')}</span>
              </Button>
            </div>
          )
        },
      },
    ],
    // The pending actions drive the cell content.
    [asyncAction.pendingActions, t],
  )

  return (
    <section
      className="grid min-w-0 max-w-full gap-3"
      data-foundation-route-id="R-RELAYS"
    >
      <HeaderActions>
        <Badge variant="secondary">
          {query.data?.connected_count ?? 0}/{query.data?.enabled_count ?? 0}
        </Badge>
        <Button
          disabled={createPending}
          onClick={openCreateDialog}
          size="sm"
          type="button"
        >
          <Plus aria-hidden="true" />
          <span>{t('relayNew')}</span>
        </Button>
        <Button
          aria-label={t('refresh')}
          onClick={() => void query.refetch()}
          size="sm"
          type="button"
          variant="outline"
        >
          {query.isFetching ? (
            <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />
          ) : (
            <RefreshCw aria-hidden="true" />
          )}
          <span>{t('refresh')}</span>
        </Button>
      </HeaderActions>

      {query.isError && (
        <Alert>
          <Info aria-hidden="true" />
          <AlertTitle>{t('relayWorkerDisconnected')}</AlertTitle>
          <AlertDescription>{t('relayWorkerUnavailable')}</AlertDescription>
        </Alert>
      )}

      <div className="min-w-0">
        <DataTable
          className="min-w-[1040px]"
          columns={columns}
          data={rowsData}
          getRowId={(relay) => relay.relay_id}
          label={t('relays')}
          loading={query.isPending}
          emptyLabel={t('relayEmpty')}
        />
      </div>

      <TablePagination
        first={first}
        onChange={changePage}
        pageSizeOptions={STANDARD_PAGE_SIZE_OPTIONS}
        rows={rows}
        total={query.data?.total ?? 0}
      />

      <RelayDialog
        currentRelay={editingRelay}
        header={editingRelay ? t('relayEdit') : t('relayNew')}
        key={`${editingRelay?.relay_id ?? 'new'}:${dialogGeneration.current}`}
        onOpenChange={changeDialogOpen}
        onSave={(form) => void saveRelay(form)}
        open={dialogOpen}
        pending={dialogPending}
        relay={editingRelay}
        savePending={dialogPendingAction === 'save'}
      />
    </section>
  )
}
