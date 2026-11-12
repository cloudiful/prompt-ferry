import { useMemo, useRef } from 'react'
import { Info, Pause, Pencil, Play, Plus, Trash2 } from 'lucide-react'
import type { BillingPriceRuleResponse } from '@/generated/admin-api'
import {
  DataTable,
  type DataTableColumnDef,
} from '@/components/shared/DataTable'
import { TablePagination } from '@/components/shared/TablePagination'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { useLocale } from '@/lib/i18n'
import { useAsyncAction } from '@/lib/use-async-action'
import { formatDateTime } from '@/format/datetime-core'
import { billingPriceRuleActionKey } from '@/lib/billing'
import {
  BILLING_PRICE_RULE_PAGE_SIZE_OPTIONS,
  formatBillingRate,
} from '@/lib/billing-model'
import type { TablePageChange } from '@/models/pagination'

type BillingPriceRulesProps = {
  first: number
  loading: boolean
  rows: number
  rules: BillingPriceRuleResponse[]
  total: number
  onCreate: () => void
  onDelete: (rule: BillingPriceRuleResponse) => void
  onEdit: (rule: BillingPriceRuleResponse) => void
  onPage: (change: TablePageChange) => void
  onToggle: (rule: BillingPriceRuleResponse) => void
}

export function BillingPriceRules({
  first,
  loading,
  rows,
  rules,
  total,
  onCreate,
  onDelete,
  onEdit,
  onPage,
  onToggle,
}: BillingPriceRulesProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const asyncAction = useAsyncAction()
  const callbacks = useRef({ onDelete, onEdit, onToggle })
  callbacks.current = { onDelete, onEdit, onToggle }
  const createPending = asyncAction.isPending(billingPriceRuleActionKey(null))

  function renderActions(rule: BillingPriceRuleResponse) {
    const pendingAction = asyncAction.pendingOrigin(
      billingPriceRuleActionKey(rule.price_rule_id),
    )
    const pending = pendingAction !== null
    return (
      <div className="flex justify-end gap-1 whitespace-nowrap">
        <Button
          aria-label={`${t('editPriceRule')} ${rule.public_model}`}
          disabled={pending}
          onClick={() => callbacks.current.onEdit(rule)}
          size="icon-sm"
          type="button"
          variant="ghost"
        >
          <Pencil aria-hidden="true" />
        </Button>
        <Button
          aria-label={`${t('deletePriceRule')} ${rule.public_model}`}
          disabled={pending}
          onClick={() => callbacks.current.onDelete(rule)}
          size="icon-sm"
          type="button"
          variant="ghost"
        >
          {pendingAction === 'delete' ? (
            <span className="size-3 animate-spin rounded-full border-2 border-current border-r-transparent" />
          ) : (
            <Trash2 aria-hidden="true" />
          )}
        </Button>
        <Button
          aria-label={`${
            rule.enabled ? t('disablePriceRule') : t('enablePriceRule')
          } ${rule.public_model}`}
          disabled={pending}
          onClick={() => callbacks.current.onToggle(rule)}
          size="icon-sm"
          type="button"
          variant="ghost"
        >
          {pendingAction === 'toggle' ? (
            <span className="size-3 animate-spin rounded-full border-2 border-current border-r-transparent" />
          ) : rule.enabled ? (
            <Pause aria-hidden="true" />
          ) : (
            <Play aria-hidden="true" />
          )}
        </Button>
      </div>
    )
  }

  const columns = useMemo<Array<DataTableColumnDef<BillingPriceRuleResponse>>>(
    () => [
      {
        id: 'public-model',
        header: t('publicModel'),
        cell: ({ row }) => (
          <div className="max-w-64 break-words font-medium">
            {row.original.public_model}
          </div>
        ),
      },
      {
        id: 'input-rate',
        header: t('inputRate'),
        cell: ({ row }) => (
          <div className="whitespace-nowrap">
            {formatBillingRate(row.original.input_rate, row.original.currency)}
          </div>
        ),
      },
      {
        id: 'cache-read-rate',
        header: t('cacheReadRate'),
        cell: ({ row }) => (
          <div className="whitespace-nowrap">
            {formatBillingRate(
              row.original.cache_read_rate,
              row.original.currency,
            )}
          </div>
        ),
      },
      {
        id: 'cache-write-rate',
        header: t('cacheWriteRate'),
        cell: ({ row }) => (
          <div className="whitespace-nowrap">
            {formatBillingRate(
              row.original.cache_write_rate,
              row.original.currency,
            )}
          </div>
        ),
      },
      {
        id: 'output-rate',
        header: t('outputRate'),
        cell: ({ row }) => (
          <div className="whitespace-nowrap">
            {formatBillingRate(row.original.output_rate, row.original.currency)}
          </div>
        ),
      },
      {
        id: 'effective-from',
        header: t('effectiveFrom'),
        cell: ({ row }) => (
          <div className="whitespace-nowrap">
            {formatDateTime(
              row.original.effective_from,
              locale,
              undefined,
              timeZone.effective,
            )}
          </div>
        ),
      },
      {
        id: 'status',
        header: t('status'),
        cell: ({ row }) => (
          <Badge variant={row.original.enabled ? 'default' : 'outline'}>
            {row.original.enabled ? t('active') : t('disabled')}
          </Badge>
        ),
      },
      {
        id: 'actions',
        header: () => <span className="sr-only">{t('editPriceRule')}</span>,
        cell: ({ row }) => renderActions(row.original),
      },
    ],
    [asyncAction, locale, t, timeZone.effective],
  )

  return (
    <section
      aria-busy={loading}
      className="grid min-w-0 gap-3 rounded-lg border p-3"
    >
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <div className="mr-auto flex items-center gap-1">
          <h2 className="m-0 text-sm font-semibold">
            {t('billingPriceRules')}
          </h2>
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  aria-label={t('billingRuleScopeHint')}
                  size="icon-sm"
                  type="button"
                  variant="ghost"
                >
                  <Info aria-hidden="true" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{t('billingRuleScopeHint')}</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        </div>
        <Button
          disabled={createPending}
          onClick={onCreate}
          size="sm"
          type="button"
        >
          {createPending ? (
            <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />
          ) : (
            <Plus aria-hidden="true" />
          )}
          {t('newPriceRule')}
        </Button>
      </div>
      <div className="min-w-0 overflow-x-auto">
        <DataTable
          className="min-w-[64rem]"
          columns={columns}
          data={rules}
          emptyLabel={t('noPriceRules')}
          getRowId={(rule) => rule.price_rule_id}
          label={t('billingPriceRules')}
          loading={loading}
        />
      </div>
      <TablePagination
        first={first}
        onChange={onPage}
        pageSizeOptions={BILLING_PRICE_RULE_PAGE_SIZE_OPTIONS}
        rows={rows}
        total={total}
      />
    </section>
  )
}
