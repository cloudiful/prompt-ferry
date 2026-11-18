import { useQuery } from '@tanstack/react-query'
import { RefreshCw } from 'lucide-react'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { HeaderActions } from '@/components/layout/HeaderActions'
import { Skeleton } from '@/components/ui/skeleton'
import {
  DataTable,
  type DataTableColumnDef,
} from '@/components/shared/DataTable'
import type { NodePeerSummary } from '@/generated/admin-api'
import { nodeStatusOptions } from '@/generated/admin-api/@tanstack/react-query.gen'
import { formatApiError } from '@/api'
import { useLocale } from '@/lib/i18n'
import { usePageTitle } from '@/lib/router'

export function NodeStatusPage() {
  const { t } = useLocale()
  const query = useQuery({
    ...nodeStatusOptions(),
    refetchInterval: (query) => (query.state.data ? 5000 : false),
  })

  usePageTitle('nodeInfo')

  const status = query.data
  const errorMessage = query.error ? formatApiError(query.error) : ''
  const unavailable = errorMessage.includes('service_unavailable')

  const columns: Array<DataTableColumnDef<NodePeerSummary>> = [
    {
      id: 'node_id',
      header: t('nodeId'),
      cell: ({ row }) => (
        <span className="block max-w-64 truncate font-medium">
          {row.original.node_id}
        </span>
      ),
    },
    {
      id: 'database',
      header: t('nodeDatabaseCapability'),
      cell: ({ row }) =>
        t(
          row.original.has_database
            ? 'nodeDatabasePresent'
            : 'nodeDatabaseAbsent',
        ),
    },
    {
      id: 'runtime_version',
      header: t('nodeRuntimeVersion'),
      cell: ({ row }) => row.original.runtime_version,
    },
    {
      id: 'protocol_version',
      header: t('nodeProtocolVersion'),
      cell: ({ row }) => row.original.protocol_version,
    },
    {
      id: 'connected_at',
      header: t('nodeConnectedAt'),
      cell: ({ row }) => formatTimestamp(row.original.connected_at),
    },
  ]

  return (
    <section
      className="grid min-w-0 max-w-full gap-3"
      data-foundation-route-id="R-NODE"
    >
      <HeaderActions>
        <Button
          aria-label={t('refresh')}
          disabled={query.isFetching}
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

      <p className="text-sm text-muted-foreground">{t('nodeDescription')}</p>
      <p className="text-sm text-muted-foreground">{t('nodeLiveView')}</p>

      {query.isError && (
        <Alert variant="destructive">
          <AlertTitle>
            {t(unavailable ? 'nodeUnavailable' : 'nodeLoadFailed')}
          </AlertTitle>
          <AlertDescription className="grid gap-3">
            <span>
              {unavailable
                ? t('nodeUnavailableDesc')
                : errorMessage || t('nodeUnavailableDesc')}
            </span>
            <Button
              className="w-fit"
              disabled={query.isFetching}
              onClick={() => void query.refetch()}
              size="sm"
              type="button"
              variant="outline"
            >
              {t('retry')}
            </Button>
          </AlertDescription>
        </Alert>
      )}

      {query.isPending ? (
        <div aria-label={t('loading')} className="grid gap-3" role="status">
          <Skeleton className="h-8 w-48" />
          <Skeleton className="h-28 w-full" />
        </div>
      ) : (
        status && (
          <>
            <Card>
              <CardHeader>
                <CardTitle className="text-base">
                  {t('nodeProcessingTitle')}
                </CardTitle>
              </CardHeader>
              <CardContent className="grid gap-2 text-sm">
                <NodeFact label={t('nodeId')} value={status.node_id} />
                <NodeFact label={t('nodeGroupId')} value={status.group_id} />
                <NodeFact
                  label={t('nodeRuntimeVersion')}
                  value={status.runtime_version}
                />
                <NodeFact
                  label={t('nodeProtocolVersion')}
                  value={status.protocol_version}
                />
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-muted-foreground">
                    {t('nodeDatabaseCapability')}
                  </span>
                  <Badge
                    variant={status.has_database ? 'default' : 'secondary'}
                  >
                    {t(
                      status.has_database
                        ? 'nodeDatabasePresent'
                        : 'nodeDatabaseAbsent',
                    )}
                  </Badge>
                </div>
              </CardContent>
            </Card>

            <section className="grid min-w-0 gap-2">
              <h2 className="text-sm font-semibold">{t('nodePeersTitle')}</h2>
              <DataTable
                columns={columns}
                data={status.peers}
                emptyLabel={t('nodePeersEmpty')}
                getRowId={(peer) => peer.node_id}
                label={t('nodePeersTitle')}
                loading={query.isPending}
              />
            </section>
          </>
        )
      )}
    </section>
  )
}

function NodeFact({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <span className="text-muted-foreground">{label}</span>
      <span className="break-all font-mono text-xs">{value}</span>
    </div>
  )
}

function formatTimestamp(value: string | null | undefined): string {
  if (!value) return '—'
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? '—' : date.toLocaleString()
}
