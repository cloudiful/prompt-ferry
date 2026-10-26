import { useCallback, useEffect, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Info, Pencil, Plus, RefreshCw, RotateCw, Trash2 } from 'lucide-react'
import type {
  ManagedRelay,
  ManagedRelayPatchRequest,
  ManagedRelayRequest,
} from '@/generated/admin-api'
import {
  createRelayPatchRequest,
  createRelayRequest,
  createEmptyRelayForm,
  relayToForm,
  type RelayForm,
} from '@/admin-mappers/forms/relay'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
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
import {
  createManagedRelay,
  deleteManagedRelay,
  loadRelayPageWithCorrection,
  reconnectManagedRelay,
  refreshRelayPage,
  relayActionKey,
  relayPageQueryKey,
  updateManagedRelay,
} from '@/lib/relays'
import type { TablePageChange } from '@/models/pagination'

const RELAY_COLUMNS = 8

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
  const [form, setForm] = useState<RelayForm>(createEmptyRelayForm)
  const dialogGeneration = useRef(0)
  const lastReportedError = useRef<unknown>(null)

  usePageTitle('relays')

  const query = useQuery({
    queryKey: relayPageQueryKey(first, rows),
    queryFn: () => loadRelayPageWithCorrection(first, rows),
    placeholderData: (previous) => previous,
  })

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
    if (query.data.first !== first) setFirst(query.data.first)
    if (query.data.rows !== rows) setRows(query.data.rows)
  }, [
    first,
    query.data,
    query.isPlaceholderData,
    query.isSuccess,
    rows,
    setRows,
  ])

  const rowsData = query.data?.relays ?? []
  const dialogKey = relayActionKey(form.relay_id || null)
  const dialogPendingAction = asyncAction.pendingOrigin(dialogKey)
  const dialogPending = dialogPendingAction !== null
  const currentRelay =
    rowsData.find((relay) => relay.relay_id === form.relay_id) ?? null
  const createPending = asyncAction.isPending(relayActionKey(null))

  function changePage(change: TablePageChange): void {
    setFirst(change.first)
    setRows(change.rows)
  }

  function openCreateDialog(): void {
    dialogGeneration.current += 1
    setForm(createEmptyRelayForm())
    setDialogOpen(true)
  }

  function openEditDialog(relay: ManagedRelay): void {
    if (asyncAction.isPending(relayActionKey(relay.relay_id))) return
    dialogGeneration.current += 1
    setForm(relayToForm(relay))
    setDialogOpen(true)
  }

  function changeDialogOpen(open: boolean): void {
    if (!open && dialogPending) return
    if (!open) dialogGeneration.current += 1
    setDialogOpen(open)
  }

  async function saveRelay(): Promise<void> {
    const capturedForm = { ...form }
    const relayId = capturedForm.relay_id || null
    const body = relayId
      ? createRelayPatchRequest(capturedForm)
      : createRelayRequest(capturedForm)
    const key = relayActionKey(relayId)
    const generation = dialogGeneration.current
    const actionFirst = first
    const actionRows = rows

    try {
      const outcome = await asyncAction.run(
        key,
        async () => {
          if (relayId) {
            await updateManagedRelay(relayId, body as ManagedRelayPatchRequest)
          } else {
            await createManagedRelay(body as ManagedRelayRequest)
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
        (form.relay_id || null) !== relayId
      ) {
        return
      }
      setFirst(outcome.value.first)
      setRows(outcome.value.page.rows)
      dialogGeneration.current += 1
      setDialogOpen(false)
      setForm(createEmptyRelayForm())
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
          await deleteManagedRelay(relayId)
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

  async function reconnectRelay(relay: ManagedRelay): Promise<void> {
    if (!relay.enabled) return
    const relayId = relay.relay_id
    const actionFirst = first
    const actionRows = rows

    try {
      const outcome = await asyncAction.run(
        relayActionKey(relayId),
        async () => {
          const result = await reconnectManagedRelay(relayId)
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

  return (
    <section
      className="grid min-w-0 max-w-full gap-3"
      data-foundation-route-id="R-RELAYS"
    >
      <div className="flex min-w-0 flex-wrap items-center justify-end gap-2">
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
      </div>

      {query.isError && (
        <Alert>
          <Info aria-hidden="true" />
          <AlertTitle>{t('relayWorkerDisconnected')}</AlertTitle>
          <AlertDescription>{t('relayWorkerUnavailable')}</AlertDescription>
        </Alert>
      )}

      <div className="min-w-0">
        <Table aria-busy={query.isFetching} className="min-w-[1040px]">
          <TableHeader>
            <TableRow>
              <TableHead>{t('name')}</TableHead>
              <TableHead>{t('relayUrl')}</TableHead>
              <TableHead>{t('status')}</TableHead>
              <TableHead>{t('relayTlsMode')}</TableHead>
              <TableHead>{t('relayBridgeMode')}</TableHead>
              <TableHead>{t('relaySecrets')}</TableHead>
              <TableHead>{t('relayLastError')}</TableHead>
              <TableHead className="text-right">{t('actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {query.isPending ? (
              <TableRow>
                <TableCell colSpan={RELAY_COLUMNS}>
                  <div
                    aria-label={t('loading')}
                    className="h-8 animate-pulse rounded-md bg-accent"
                    role="status"
                  />
                </TableCell>
              </TableRow>
            ) : rowsData.length > 0 ? (
              rowsData.map((relay) => {
                const key = relayActionKey(relay.relay_id)
                const pendingAction = asyncAction.pendingOrigin(key)
                const pending = pendingAction !== null
                return (
                  <TableRow key={relay.relay_id}>
                    <TableCell
                      className="max-w-48 truncate font-medium"
                      title={relay.name}
                    >
                      {relay.name}
                    </TableCell>
                    <TableCell
                      className="max-w-56 truncate"
                      title={relay.relay_url}
                    >
                      {relay.relay_url}
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-2 whitespace-nowrap">
                        <Badge
                          variant={relay.connected ? 'default' : 'outline'}
                        >
                          {relay.connected
                            ? t('relayStatusConnected')
                            : t('relayStatusDisconnected')}
                        </Badge>
                        <span className="text-xs text-muted-foreground">
                          {relay.enabled ? t('active') : t('disabled')}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell>{relay.tls_mode}</TableCell>
                    <TableCell>{relay.bridge_encryption_mode}</TableCell>
                    <TableCell>
                      <div className="grid gap-1 whitespace-nowrap text-xs text-muted-foreground">
                        <span>
                          {t('relaySecretCaLabel')}:{' '}
                          {secretStatus(relay.has_relay_ca)}
                        </span>
                        <span>
                          {t('relaySecretCertLabel')}:{' '}
                          {secretStatus(relay.has_client_cert)}
                        </span>
                        <span>
                          {t('relaySecretKeyLabel')}:{' '}
                          {secretStatus(relay.has_client_key)}
                        </span>
                        <span>
                          {t('relaySecretBridgeLabel')}:{' '}
                          {secretStatus(relay.has_bridge_key)}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell>
                      <div className="grid max-w-56 gap-1 text-xs">
                        <span
                          className="truncate"
                          title={relay.last_error ?? ''}
                        >
                          {relay.last_error || '—'}
                        </span>
                        <span className="whitespace-nowrap text-muted-foreground">
                          {t('relaySnapshotVersion')}{' '}
                          {relay.last_snapshot_version ?? '—'}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell>
                      <div className="flex justify-end gap-1 whitespace-nowrap">
                        <Button
                          aria-label={`${t('relayReconnect')} ${relay.name}`}
                          disabled={pending || !relay.enabled}
                          onClick={() => void reconnectRelay(relay)}
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
                          onClick={() => openEditDialog(relay)}
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
                          onClick={() => void deleteRelay(relay)}
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
                    </TableCell>
                  </TableRow>
                )
              })
            ) : query.isSuccess && query.data?.total === 0 ? (
              <TableRow>
                <TableCell
                  className="py-8 text-center text-muted-foreground"
                  colSpan={RELAY_COLUMNS}
                >
                  {t('relayEmpty')}
                </TableCell>
              </TableRow>
            ) : null}
          </TableBody>
        </Table>
      </div>

      <TablePagination
        first={first}
        onChange={changePage}
        pageSizeOptions={STANDARD_PAGE_SIZE_OPTIONS}
        rows={rows}
        total={query.data?.total ?? 0}
      />

      <RelayDialog
        currentRelay={currentRelay}
        form={form}
        header={form.relay_id ? t('relayEdit') : t('relayNew')}
        onChange={(patch) => setForm((current) => ({ ...current, ...patch }))}
        onOpenChange={changeDialogOpen}
        onSave={() => void saveRelay()}
        open={dialogOpen}
        pending={dialogPending}
        savePending={dialogPendingAction === 'save'}
      />
    </section>
  )
}
