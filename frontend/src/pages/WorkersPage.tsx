import { useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
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
import type { WorkerSummary } from '@/generated/admin-api'
import { listWorkersOptions } from '@/generated/admin-api/@tanstack/react-query.gen'
import { formatApiError } from '@/api'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import type { TablePageChange } from '@/models/pagination'

export function WorkersPage() {
  const { t } = useLocale()
  const { notifyApiError } = useNotifier()
  const [first, setFirst] = useState(0)
  const [rows, setRows] = useStoredPageSize('workers')

  const query = useQuery({
    ...listWorkersOptions({ query: { first, rows } }),
    placeholderData: (previous) => previous,
  })

  const queryError = query.error ? formatApiError(query.error) : ''
  const unavailable = queryError.includes('sqlite_workers_unavailable')

  useEffect(() => {
    if (query.error && !unavailable) notifyApiError(query.error)
  }, [notifyApiError, query.error, unavailable])

  useEffect(() => {
    if (!query.data) return
    if (query.data.first !== first) setFirst(query.data.first)
    if (query.data.rows !== rows) setRows(query.data.rows)
  }, [first, query.data, rows, setRows])

  function changePage(change: TablePageChange): void {
    setFirst(change.first)
    setRows(change.rows)
  }

  function formatTimestamp(value: string | null | undefined): string {
    if (!value) return '—'
    const date = new Date(value)
    return Number.isNaN(date.getTime()) ? '—' : date.toLocaleString()
  }

  function statusLabel(status: string): string {
    switch (status) {
      case 'online':
        return t('workerStatusOnline')
      case 'offline':
        return t('workerStatusOffline')
      default:
        return t('workerStatusStopped')
    }
  }

  const columns: Array<DataTableColumnDef<WorkerSummary>> = [
    {
      id: 'worker_id',
      header: t('workerId'),
      cell: ({ row }) => (
        <span className="block max-w-64 truncate font-medium">
          {row.original.worker_id}
        </span>
      ),
    },
    {
      id: 'role',
      header: t('workerRole'),
      cell: ({ row }) => row.original.role,
    },
    {
      id: 'status',
      header: t('workerStatus'),
      cell: ({ row }) => (
        <span
          data-worker-status={row.original.status}
          className={
            row.original.status === 'online'
              ? 'text-emerald-600 dark:text-emerald-400'
              : 'text-muted-foreground'
          }
        >
          {statusLabel(row.original.status)}
        </span>
      ),
    },
    {
      id: 'version',
      header: t('workerVersion'),
      cell: ({ row }) => row.original.version,
    },
    {
      id: 'config_revision',
      header: t('workerConfigRevision'),
      cell: ({ row }) =>
        row.original.config_restart_required
          ? `${row.original.config_applied_revision ?? '—'} · ${t('workerRestartRequired')}`
          : (row.original.config_applied_revision ?? '—'),
    },
    {
      id: 'last_heartbeat_at',
      header: t('workerHeartbeat'),
      cell: ({ row }) => formatTimestamp(row.original.last_heartbeat_at),
    },
    {
      id: 'started_at',
      header: t('workerStarted'),
      cell: ({ row }) => formatTimestamp(row.original.started_at),
    },
  ]

  const workers = query.data?.workers ?? []
  const coordination = query.data?.coordination

  return (
    <section
      className="grid min-w-0 max-w-full gap-3"
      data-foundation-route-id="R-WORKERS"
    >
      <HeaderActions>
        <Button
          aria-label={t('refresh')}
          onClick={() => void query.refetch()}
          size="sm"
          type="button"
          variant="outline"
        >
          {query.isFetching && (
            <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />
          )}
          <span>{t('refresh')}</span>
        </Button>
      </HeaderActions>

      <p className="text-muted-foreground text-sm">
        {t('workerOnlineCount', { count: query.data?.online_count ?? 0 })}
      </p>

      {coordination && (
        <p
          className={
            coordination.degraded
              ? 'text-amber-600 dark:text-amber-400 text-sm'
              : 'text-muted-foreground text-sm'
          }
          data-worker-coordination={coordination.mode}
          data-worker-degraded={coordination.degraded ? 'true' : 'false'}
        >
          {coordination.degraded
            ? t('workerCoordinationDegraded', {
                count: coordination.live_worker_count,
                reason: coordination.degraded_reason ?? '',
              })
            : t('workerCoordinationShared', {
                count: coordination.live_worker_count,
                mode: coordination.mode,
              })}
        </p>
      )}

      {unavailable ? (
        <p className="text-muted-foreground text-sm">{t('workerUnavailable')}</p>
      ) : (
        <DataTable
          columns={columns}
          data={workers}
          emptyLabel={t('workerEmpty')}
          getRowId={(worker) => worker.worker_id}
          label={t('workers')}
          loading={query.isPending}
        />
      )}

      <TablePagination
        first={query.data?.first ?? first}
        onChange={changePage}
        pageSizeOptions={[10, 20, 50]}
        rows={query.data?.rows ?? rows}
        total={query.data?.total ?? 0}
      />
    </section>
  )
}
