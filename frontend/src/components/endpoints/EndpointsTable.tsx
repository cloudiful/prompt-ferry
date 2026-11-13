import { useMemo } from 'react'
import { Gauge, LoaderCircle, Pencil, RefreshCw, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { TablePagination } from '@/components/shared/TablePagination'
import {
  DataTable,
  type DataTableColumnDef,
} from '@/components/shared/DataTable'
import { EndpointMobileCard } from '@/components/endpoints/EndpointMobileCard'
import { QuotaBadges } from '@/components/endpoints/QuotaBadges'
import { ProviderIcon } from '@/components/providers/ProviderIcon'
import {
  TestResultPopover,
  testResultKey,
  type TestResultSurface,
} from '@/components/shared/TestResultPopover'
import { useLocale } from '@/lib/i18n'
import type { TablePageChange } from '@/models/pagination'
import { STANDARD_PAGE_SIZE_OPTIONS } from '@/models/pagination'
import { isQuotaEligible } from '@/models/endpoints/quota'
import {
  endpointActionKey,
  networkTestKey,
  type EndpointListItemView,
} from '@/models/endpoints'

type EndpointsTableProps = {
  first: number
  items: EndpointListItemView[]
  loading: boolean
  onDelete: (endpointId: string) => void
  onEdit: (endpointId: string) => void
  onPage: (change: TablePageChange) => void
  onResultOpenChange: (key: string, open: boolean) => void
  onTest: (endpointId: string, surface: TestResultSurface) => void
  onTokenPlanUsage: (endpointId: string) => void
  onToggle: (endpointId: string, enabled: boolean) => void
  openResultKey: string | null
  pendingActions: Readonly<Record<string, string>>
  rows: number
  total: number
}

export function EndpointsTable({
  first,
  items,
  loading,
  onDelete,
  onEdit,
  onPage,
  onResultOpenChange,
  onTest,
  onTokenPlanUsage,
  onToggle,
  openResultKey,
  pendingActions,
  rows,
  total,
}: EndpointsTableProps) {
  const { t } = useLocale()
  const showQuotaColumn = items.some(isQuotaEligible)

  const columns = useMemo<
    Array<DataTableColumnDef<EndpointListItemView>>
  >(() => {
    const result: Array<DataTableColumnDef<EndpointListItemView>> = [
      {
        id: 'endpoint',
        header: t('endpoint'),
        cell: ({ row }) => {
          const item = row.original
          return (
            <div className="flex min-w-0 items-start gap-2">
              <ProviderIcon provider={item.provider} />
              <div className="min-w-0">
                <div
                  className="break-words font-medium"
                  data-endpoint-name
                  title={item.name}
                >
                  {item.name}
                </div>
                {item.owner_label ? (
                  <div
                    className="text-xs text-muted-foreground"
                    data-endpoint-owner
                  >
                    {item.owner_label}
                  </div>
                ) : null}
              </div>
            </div>
          )
        },
      },
      {
        id: 'status',
        header: t('status'),
        cell: ({ row }) => {
          const item = row.original
          const rowKey = endpointActionKey(item.endpoint_id)
          const rowPending = Object.hasOwn(pendingActions, rowKey)
          const rowOrigin = pendingActions[rowKey]
          return (
            <div className="flex items-center gap-2">
              <Switch
                aria-label={t('endpointEnabledToggle', { name: item.name })}
                checked={item.enabled}
                disabled={rowPending}
                onCheckedChange={(enabled) =>
                  onToggle(item.endpoint_id, enabled)
                }
              />
              {rowOrigin === 'toggle' ? (
                <LoaderCircle
                  aria-label={t('loading')}
                  className="size-4 animate-spin"
                />
              ) : null}
            </div>
          )
        },
      },
    ]

    if (showQuotaColumn) {
      result.push({
        id: 'quota',
        header: t('tokenPlanUsageColumn'),
        cell: ({ row }) =>
          isQuotaEligible(row.original) ? (
            <QuotaBadges
              density="table"
              eligible
              endpointId={row.original.endpoint_id}
            />
          ) : null,
      })
    }

    result.push({
      id: 'actions',
      header: () => <span className="sr-only">{t('actions')}</span>,
      cell: ({ row }) => {
        const item = row.original
        const rowKey = endpointActionKey(item.endpoint_id)
        const probeKey = networkTestKey('endpoint', item.endpoint_id)
        const rowPending = Object.hasOwn(pendingActions, rowKey)
        const rowOrigin = pendingActions[rowKey]
        const probePending = Object.hasOwn(pendingActions, probeKey)
        const resultKey = testResultKey('desktop', item.endpoint_id)
        return (
          <div className="flex justify-end gap-1">
            <TestResultPopover
              message={item.test_message}
              onOpenChange={(open) => onResultOpenChange(resultKey, open)}
              open={openResultKey === resultKey}
              severity={item.test_severity}
            >
              <Button
                aria-label={t('test')}
                disabled={probePending}
                onClick={() => onTest(item.endpoint_id, 'desktop')}
                size="icon-sm"
                type="button"
                variant="ghost"
              >
                {probePending ? (
                  <LoaderCircle aria-hidden="true" className="animate-spin" />
                ) : (
                  <RefreshCw aria-hidden="true" />
                )}
              </Button>
            </TestResultPopover>
            {isQuotaEligible(item) ? (
              <Button
                aria-label={t('tokenPlanUsage')}
                onClick={() => onTokenPlanUsage(item.endpoint_id)}
                size="icon-sm"
                title={t('tokenPlanUsage')}
                type="button"
                variant="ghost"
              >
                <Gauge aria-hidden="true" />
              </Button>
            ) : null}
            <Button
              aria-label={t('edit')}
              disabled={rowPending}
              onClick={() => onEdit(item.endpoint_id)}
              size="icon-sm"
              type="button"
              variant="ghost"
            >
              <Pencil aria-hidden="true" />
            </Button>
            <Button
              aria-label={t('delete')}
              disabled={rowPending}
              onClick={() => onDelete(item.endpoint_id)}
              size="icon-sm"
              type="button"
              variant="ghost"
            >
              {rowOrigin === 'delete' ? (
                <LoaderCircle aria-hidden="true" className="animate-spin" />
              ) : (
                <Trash2 aria-hidden="true" />
              )}
            </Button>
          </div>
        )
      },
    })

    return result
  }, [
    onDelete,
    onEdit,
    onResultOpenChange,
    onTest,
    onTokenPlanUsage,
    onToggle,
    openResultKey,
    pendingActions,
    showQuotaColumn,
    t,
  ])

  return (
    <section className="grid min-w-0 gap-3" aria-label={t('endpoint')}>
      <DataTable
        columns={columns}
        data={items}
        emptyLabel={t('noData')}
        getRowId={(item) => item.endpoint_id}
        label={t('endpoint')}
        loading={loading}
        renderCard={(item) => (
          <EndpointMobileCard
            item={item}
            onDelete={onDelete}
            onEdit={onEdit}
            onResultOpenChange={onResultOpenChange}
            onTest={onTest}
            onTokenPlanUsage={onTokenPlanUsage}
            onToggle={onToggle}
            openResultKey={openResultKey}
            pendingActions={pendingActions}
          />
        )}
        rowAttributes={(item) => ({
          'data-endpoint-row': item.endpoint_id,
        })}
      />
      <TablePagination
        first={first}
        onChange={onPage}
        pageSizeOptions={STANDARD_PAGE_SIZE_OPTIONS}
        rows={rows}
        total={total}
      />
    </section>
  )
}
