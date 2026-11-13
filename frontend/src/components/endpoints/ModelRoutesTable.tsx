import { useMemo } from 'react'
import { LoaderCircle, Pencil, RefreshCw, Trash2 } from 'lucide-react'
import {
  TestResultPopover,
  testResultKey,
  type TestResultSurface,
} from '@/components/shared/TestResultPopover'
import { TablePagination } from '@/components/shared/TablePagination'
import {
  DataTable,
  type DataTableColumnDef,
} from '@/components/shared/DataTable'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useLocale } from '@/lib/i18n'
import {
  modelRouteActionKey,
  type ModelRouteListItemView,
} from '@/models/endpoints'
import {
  modelRouteTargetToggleOriginTarget,
  modelRouteTestCounts,
} from '@/models/endpoints/model-route-item'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  type TablePageChange,
} from '@/models/pagination'
import { ModelRouteMobileCard } from './ModelRouteMobileCard'
import { ModelRouteTargetsSummary } from './ModelRouteTargetsSummary'

type ModelRoutesTableProps = {
  first: number
  items: ModelRouteListItemView[]
  loading: boolean
  onDelete: (ruleId: string) => void
  onEdit: (ruleId: string) => void
  onPageChange: (change: TablePageChange) => void
  onProbe: (ruleId: string, surface: TestResultSurface) => void
  onResultOpenChange: (key: string, open: boolean) => void
  onShowDetails: (ruleId: string) => void
  onToggle: (ruleId: string, enabled: boolean) => void
  onToggleTarget: (ruleId: string, targetId: string, enabled: boolean) => void
  openResultKey: string | null
  pendingActions: Readonly<Record<string, string>>
  rows: number
  total: number
}

export function ModelRoutesTable({
  first,
  items,
  loading,
  onDelete,
  onEdit,
  onPageChange,
  onProbe,
  onResultOpenChange,
  onShowDetails,
  onToggle,
  onToggleTarget,
  openResultKey,
  pendingActions,
  rows,
  total,
}: ModelRoutesTableProps) {
  const { t } = useLocale()

  const columns = useMemo<
    Array<DataTableColumnDef<ModelRouteListItemView>>
  >(() => {
    return [
      {
        id: 'model_pattern',
        header: t('modelPattern'),
        cell: ({ row }) => (
          <span
            aria-label={row.original.model_pattern}
            className="block max-w-[22rem] truncate font-medium"
            title={row.original.model_pattern}
          >
            {row.original.model_pattern}
          </span>
        ),
      },
      {
        id: 'endpoint',
        header: t('endpoint'),
        cell: ({ row }) => {
          const item = row.original
          const pending = pendingActions[modelRouteActionKey(item.rule_id)]
          return (
            <div className="grid min-w-0 max-w-[28rem] gap-1">
              <ModelRouteTargetsSummary
                onToggleTarget={(targetId, enabled) =>
                  onToggleTarget(item.rule_id, targetId, enabled)
                }
                pending={Boolean(pending)}
                pendingTargetId={modelRouteTargetToggleOriginTarget(pending)}
                targets={item.targets}
              />
              {item.owner_label ? (
                <span className="text-xs text-muted-foreground">
                  {item.owner_label}
                </span>
              ) : null}
            </div>
          )
        },
      },
      {
        id: 'status',
        header: t('status'),
        cell: ({ row }) => {
          const item = row.original
          const pending = pendingActions[modelRouteActionKey(item.rule_id)]
          return (
            <div className="flex items-center gap-2 whitespace-nowrap">
              <Switch
                aria-label={`${t('status')} ${item.model_pattern}`}
                checked={item.enabled}
                disabled={Boolean(pending)}
                onCheckedChange={(enabled) => onToggle(item.rule_id, enabled)}
              />
              {pending === 'toggle' ? (
                <LoaderCircle
                  aria-label={t('loading')}
                  className="size-4 animate-spin text-muted-foreground"
                />
              ) : null}
            </div>
          )
        },
      },
      {
        id: 'actions',
        header: () => <span className="sr-only">{t('edit')}</span>,
        cell: ({ row }) => {
          const item = row.original
          const pending = pendingActions[modelRouteActionKey(item.rule_id)]
          const counts = modelRouteTestCounts(item.test_results)
          const resultKey = testResultKey('desktop', item.rule_id)
          return (
            <div className="flex justify-end gap-1">
              <TestResultPopover
                detailLabel={
                  counts.total ? `${counts.passed}/${counts.total}` : undefined
                }
                message={item.test_message}
                onOpenChange={(open) => onResultOpenChange(resultKey, open)}
                onSelect={() => onShowDetails(item.rule_id)}
                open={openResultKey === resultKey}
                severity={item.test_severity}
              >
                <Button
                  aria-label={`${t('test')} ${item.model_pattern}`}
                  disabled={item.testing}
                  onClick={() => onProbe(item.rule_id, 'desktop')}
                  size="icon-sm"
                  title={t('test')}
                  type="button"
                  variant="ghost"
                >
                  {item.testing ? (
                    <LoaderCircle className="animate-spin" />
                  ) : (
                    <RefreshCw />
                  )}
                </Button>
              </TestResultPopover>
              <Button
                aria-label={`${t('edit')} ${item.model_pattern}`}
                disabled={Boolean(pending)}
                onClick={() => onEdit(item.rule_id)}
                size="icon-sm"
                type="button"
                variant="ghost"
              >
                <Pencil />
              </Button>
              <Button
                aria-label={`${t('delete')} ${item.model_pattern}`}
                disabled={Boolean(pending)}
                onClick={() => onDelete(item.rule_id)}
                size="icon-sm"
                type="button"
                variant="ghost"
              >
                {pending === 'delete' ? (
                  <LoaderCircle className="animate-spin" />
                ) : (
                  <Trash2 />
                )}
              </Button>
            </div>
          )
        },
      },
    ]
  }, [
    onDelete,
    onEdit,
    onProbe,
    onResultOpenChange,
    onShowDetails,
    onToggle,
    onToggleTarget,
    openResultKey,
    pendingActions,
    t,
  ])

  return (
    <section className="grid min-w-0 gap-3">
      <DataTable
        columns={columns}
        data={items}
        emptyLabel={t('noModelRoutes')}
        getRowId={(item) => item.rule_id}
        label={t('modelPattern')}
        loading={loading}
        renderCard={(item) => (
          <ModelRouteMobileCard
            item={item}
            onDelete={onDelete}
            onEdit={onEdit}
            onProbe={onProbe}
            onResultOpenChange={onResultOpenChange}
            onShowDetails={onShowDetails}
            onToggle={onToggle}
            onToggleTarget={onToggleTarget}
            openResultKey={openResultKey}
            pendingActions={pendingActions}
          />
        )}
        rowAttributes={(item) => ({
          'data-model-route-row': item.rule_id,
        })}
      />
      <TablePagination
        first={first}
        onChange={onPageChange}
        pageSizeOptions={STANDARD_PAGE_SIZE_OPTIONS}
        rows={rows}
        total={total}
      />
    </section>
  )
}
