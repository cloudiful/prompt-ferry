import { LoaderCircle, Pencil, RefreshCw, Trash2 } from 'lucide-react'
import { TestResultPopover } from '@/components/shared/TestResultPopover'
import { TablePagination } from '@/components/shared/TablePagination'
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
import { useLocale } from '@/lib/i18n'
import {
  modelRouteActionKey,
  type ModelRouteListItemView,
} from '@/models/endpoints'
import { modelRouteTestCounts } from '@/models/endpoints/model-route-item'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  type TablePageChange,
} from '@/models/pagination'
import { ModelRouteTargetsSummary } from './ModelRouteTargetsSummary'

type ModelRoutesTableProps = {
  first: number
  items: ModelRouteListItemView[]
  onDelete: (ruleId: string) => void
  onEdit: (ruleId: string) => void
  onPageChange: (change: TablePageChange) => void
  onProbe: (ruleId: string) => void
  onShowDetails: (ruleId: string) => void
  onToggle: (ruleId: string, enabled: boolean) => void
  pendingActions: Readonly<Record<string, string>>
  rows: number
  total: number
}

export function ModelRoutesTable({
  first,
  items,
  onDelete,
  onEdit,
  onPageChange,
  onProbe,
  onShowDetails,
  onToggle,
  pendingActions,
  rows,
  total,
}: ModelRoutesTableProps) {
  const { t } = useLocale()

  return (
    <div className="hidden min-w-0 md:block">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{t('modelPattern')}</TableHead>
            <TableHead>{t('endpoint')}</TableHead>
            <TableHead>{t('status')}</TableHead>
            <TableHead>{t('test')}</TableHead>
            <TableHead>
              <span className="sr-only">{t('edit')}</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {items.length === 0 ? (
            <TableRow>
              <TableCell
                className="py-8 text-center text-muted-foreground"
                colSpan={5}
              >
                {t('noModelRoutes')}
              </TableCell>
            </TableRow>
          ) : (
            items.map((item) => {
              const writeKey = modelRouteActionKey(item.rule_id)
              const pending = pendingActions[writeKey]
              const counts = modelRouteTestCounts(item.test_results)
              return (
                <TableRow
                  data-model-route-row={item.rule_id}
                  key={item.rule_id}
                >
                  <TableCell className="max-w-[22rem]">
                    <span
                      aria-label={item.model_pattern}
                      className="block max-w-[22rem] truncate font-medium"
                      title={item.model_pattern}
                    >
                      {item.model_pattern}
                    </span>
                  </TableCell>
                  <TableCell className="min-w-0 max-w-[28rem]">
                    <div className="grid min-w-0 gap-1">
                      <ModelRouteTargetsSummary targets={item.targets} />
                      {item.owner_label ? (
                        <span className="text-xs text-muted-foreground">
                          {item.owner_label}
                        </span>
                      ) : null}
                    </div>
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center gap-2 whitespace-nowrap">
                      <span className="rounded-full border px-2 py-0.5 text-xs">
                        {item.scope_label}
                      </span>
                      <Switch
                        aria-label={`${t('status')} ${item.model_pattern}`}
                        checked={item.enabled}
                        disabled={Boolean(pending)}
                        onCheckedChange={(enabled) =>
                          onToggle(item.rule_id, enabled)
                        }
                      />
                      {pending === 'toggle' ? (
                        <LoaderCircle
                          aria-label={t('loading')}
                          className="size-4 animate-spin text-muted-foreground"
                        />
                      ) : null}
                    </div>
                  </TableCell>
                  <TableCell>
                    <TestResultPopover
                      detailLabel={
                        counts.total
                          ? `${counts.passed}/${counts.total}`
                          : undefined
                      }
                      message={item.test_message}
                      onSelect={() => onShowDetails(item.rule_id)}
                      severity={item.test_severity}
                    />
                  </TableCell>
                  <TableCell>
                    <div className="flex justify-end gap-1">
                      <Button
                        aria-label={`${t('test')} ${item.model_pattern}`}
                        disabled={item.testing}
                        onClick={() => onProbe(item.rule_id)}
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
                  </TableCell>
                </TableRow>
              )
            })
          )}
        </TableBody>
      </Table>
      <TablePagination
        first={first}
        onChange={onPageChange}
        pageSizeOptions={STANDARD_PAGE_SIZE_OPTIONS}
        rows={rows}
        total={total}
      />
    </div>
  )
}
