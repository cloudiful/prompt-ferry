import { Gauge, LoaderCircle, Pencil, RefreshCw, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { TablePagination } from '@/components/shared/TablePagination'
import { EndpointMobileCard } from '@/components/endpoints/EndpointMobileCard'
import { QuotaBadges } from '@/components/endpoints/QuotaBadges'
import { ProviderIcon } from '@/components/providers/ProviderIcon'
import { TestResultPopover } from '@/components/shared/TestResultPopover'
import { WorkspacePagerBar } from '@/components/shared/WorkspacePagerBar'
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
  onTest: (endpointId: string) => void
  onTokenPlanUsage: (endpointId: string) => void
  onToggle: (endpointId: string, enabled: boolean) => void
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
  onTest,
  onTokenPlanUsage,
  onToggle,
  pendingActions,
  rows,
  total,
}: EndpointsTableProps) {
  const { t } = useLocale()
  const start = total === 0 ? 0 : first + 1
  const end = Math.min(first + rows, total)
  const showQuotaColumn = items.some(isQuotaEligible)

  return (
    <>
      <div className="grid gap-3 md:hidden">
        {loading && items.length === 0 ? (
          <p
            aria-live="polite"
            className="rounded-xl border p-4 text-sm text-muted-foreground"
          >
            {t('loading')}
          </p>
        ) : items.length === 0 ? (
          <p className="rounded-xl border p-4 text-sm text-muted-foreground">
            {t('noData')}
          </p>
        ) : (
          items.map((item) => (
            <EndpointMobileCard
              item={item}
              key={item.endpoint_id}
              onDelete={onDelete}
              onEdit={onEdit}
              onTest={onTest}
              onTokenPlanUsage={onTokenPlanUsage}
              onToggle={onToggle}
              pendingActions={pendingActions}
            />
          ))
        )}
        {total > rows ? (
          <WorkspacePagerBar
            canNext={first + rows < total}
            canPrev={first > 0}
            currentPage={Math.floor(first / rows) + 1}
            end={end}
            onNext={() =>
              onPage({
                first: first + rows,
                page: Math.floor(first / rows) + 1,
                rows,
              })
            }
            onPrev={() =>
              onPage({
                first: Math.max(0, first - rows),
                page: Math.max(0, Math.floor(first / rows) - 1),
                rows,
              })
            }
            start={start}
            total={total}
          />
        ) : null}
      </div>

      <section aria-label={t('endpoint')} className="hidden min-w-0 md:block">
        {loading && items.length === 0 ? (
          <p
            aria-live="polite"
            className="rounded-xl border p-4 text-sm text-muted-foreground"
          >
            {t('loading')}
          </p>
        ) : items.length === 0 ? (
          <p className="rounded-xl border p-4 text-sm text-muted-foreground">
            {t('noData')}
          </p>
        ) : (
          <Table className="table-fixed">
            <colgroup>
              {showQuotaColumn ? (
                <>
                  <col className="w-[42%]" />
                  <col className="w-[12%]" />
                  <col className="w-[24%]" />
                  <col className="w-[10%]" />
                  <col className="w-[12%]" />
                </>
              ) : (
                <>
                  <col className="w-[56%]" />
                  <col className="w-[14%]" />
                  <col className="w-[14%]" />
                  <col className="w-[16%]" />
                </>
              )}
            </colgroup>
            <TableHeader>
              <TableRow>
                <TableHead>{t('endpoint')}</TableHead>
                <TableHead>{t('status')}</TableHead>
                {showQuotaColumn ? (
                  <TableHead>{t('tokenPlanUsageColumn')}</TableHead>
                ) : null}
                <TableHead>{t('test')}</TableHead>
                <TableHead className="text-right">
                  <span className="sr-only">{t('actions')}</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {items.map((item) => {
                const rowKey = endpointActionKey(item.endpoint_id)
                const probeKey = networkTestKey('endpoint', item.endpoint_id)
                const rowPending = Object.hasOwn(pendingActions, rowKey)
                const rowOrigin = pendingActions[rowKey]
                const probePending = Object.hasOwn(pendingActions, probeKey)
                return (
                  <TableRow
                    data-endpoint-row={item.endpoint_id}
                    key={item.endpoint_id}
                  >
                    <TableCell className="whitespace-normal">
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
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-2">
                        <Switch
                          aria-label={t('endpointEnabledToggle', {
                            name: item.name,
                          })}
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
                    </TableCell>
                    {showQuotaColumn ? (
                      <TableCell className="min-w-0 whitespace-normal">
                        {isQuotaEligible(item) ? (
                          <QuotaBadges
                            density="table"
                            eligible
                            endpointId={item.endpoint_id}
                          />
                        ) : null}
                      </TableCell>
                    ) : null}
                    <TableCell>
                      <div className="flex items-center gap-1">
                        <Button
                          aria-label={t('test')}
                          disabled={probePending}
                          onClick={() => onTest(item.endpoint_id)}
                          size="icon-sm"
                          type="button"
                          variant="ghost"
                        >
                          {probePending ? (
                            <LoaderCircle
                              aria-hidden="true"
                              className="animate-spin"
                            />
                          ) : (
                            <RefreshCw aria-hidden="true" />
                          )}
                        </Button>
                        <TestResultPopover
                          message={item.test_message}
                          severity={item.test_severity}
                        />
                      </div>
                    </TableCell>
                    <TableCell>
                      <div className="flex justify-end gap-1">
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
                            <LoaderCircle
                              aria-hidden="true"
                              className="animate-spin"
                            />
                          ) : (
                            <Trash2 aria-hidden="true" />
                          )}
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                )
              })}
            </TableBody>
          </Table>
        )}
        <TablePagination
          first={first}
          onChange={onPage}
          pageSizeOptions={STANDARD_PAGE_SIZE_OPTIONS}
          rows={rows}
          total={total}
        />
      </section>
    </>
  )
}
