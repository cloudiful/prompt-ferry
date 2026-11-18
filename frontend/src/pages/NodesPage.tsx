import { useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import { DataTable, type DataTableColumnDef } from '@/components/shared/DataTable'
import { TablePagination, useStoredPageSize } from '@/components/shared/TablePagination'
import type { NodeSummary } from '@/generated/admin-api'
import { listNodesOptions } from '@/generated/admin-api/@tanstack/react-query.gen'
import { formatApiError } from '@/api'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import type { TablePageChange } from '@/models/pagination'

export function NodesPage() {
  const { t } = useLocale()
  const { notifyApiError } = useNotifier()
  const [first, setFirst] = useState(0)
  const [rows, setRows] = useStoredPageSize('nodes')
  const query = useQuery({ ...listNodesOptions({ query: { first, rows } }), placeholderData: (previous) => previous })
  const unavailable = query.error ? formatApiError(query.error).includes('sqlite_nodes_unavailable') : false
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
  function timestamp(value: string | null | undefined): string {
    if (!value) return '—'
    const date = new Date(value)
    return Number.isNaN(date.getTime()) ? '—' : date.toLocaleString()
  }
  const columns: Array<DataTableColumnDef<NodeSummary>> = [
    { id: 'node_id', header: t('nodeGroupId'), cell: ({ row }) => <span className="block max-w-64 truncate font-medium">{row.original.node_id}</span> },
    { id: 'status', header: t('nodeGroupStatus'), cell: ({ row }) => t(row.original.status === 'online' ? 'nodeGroupOnline' : row.original.status === 'offline' ? 'nodeGroupOffline' : 'nodeGroupStopped') },
    { id: 'version', header: t('nodeGroupVersion'), cell: ({ row }) => row.original.version },
    { id: 'config_revision', header: t('nodeGroupRevision'), cell: ({ row }) => row.original.config_restart_required ? `${row.original.config_applied_revision ?? '—'} · ${t('nodeGroupRestart')}` : (row.original.config_applied_revision ?? '—') },
    { id: 'last_heartbeat_at', header: t('nodeGroupHeartbeat'), cell: ({ row }) => timestamp(row.original.last_heartbeat_at) },
    { id: 'started_at', header: t('nodeGroupStarted'), cell: ({ row }) => timestamp(row.original.started_at) },
  ]
  const coordination = query.data?.coordination
  return <section className="grid min-w-0 max-w-full gap-3">
    <HeaderActions><Button aria-label={t('refresh')} onClick={() => void query.refetch()} size="sm" type="button" variant="outline">{query.isFetching && <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />}<span>{t('refresh')}</span></Button></HeaderActions>
    <p className="text-muted-foreground text-sm">{t('nodeGroupExplanation')}</p>
    <p className="text-muted-foreground text-sm">{t('nodeGroupOnlineCount', { count: query.data?.online_count ?? 0 })}</p>
    {coordination && <p className="text-muted-foreground text-sm">{coordination.degraded ? t('nodeGroupDegraded', { count: coordination.live_node_count, reason: coordination.degraded_reason ?? '' }) : t('nodeGroupShared', { count: coordination.live_node_count, mode: coordination.mode })}</p>}
    {unavailable ? <p>{t('nodeGroupUnavailable')}</p> : <DataTable columns={columns} data={query.data?.nodes ?? []} emptyLabel={t('nodeGroupEmpty')} getRowId={(node) => node.node_id} label={t('nodeGroups')} loading={query.isPending} />}
    <TablePagination first={query.data?.first ?? first} onChange={changePage} pageSizeOptions={[10, 20, 50]} rows={query.data?.rows ?? rows} total={query.data?.total ?? 0} />
  </section>
}
