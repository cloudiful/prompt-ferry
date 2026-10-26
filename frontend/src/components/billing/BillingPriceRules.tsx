import { Info, Pause, Pencil, Play, Plus, Trash2 } from 'lucide-react'
import type { BillingPriceRuleResponse } from '@/generated/admin-api'
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
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { TablePagination } from '@/components/shared/TablePagination'
import { useLocale } from '@/lib/i18n'
import { useDisplayTimeZone } from '@/lib/display-timezone'
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
  const createPending = asyncAction.isPending(billingPriceRuleActionKey(null))

  return (
    <section className="grid min-w-0 gap-3 rounded-lg border p-3">
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
        <Table aria-busy={loading} className="min-w-[64rem]">
          <TableHeader>
            <TableRow>
              <TableHead>{t('publicModel')}</TableHead>
              <TableHead>{t('inputRate')}</TableHead>
              <TableHead>{t('cacheReadRate')}</TableHead>
              <TableHead>{t('cacheWriteRate')}</TableHead>
              <TableHead>{t('outputRate')}</TableHead>
              <TableHead>{t('effectiveFrom')}</TableHead>
              <TableHead>{t('status')}</TableHead>
              <TableHead />
            </TableRow>
          </TableHeader>
          <TableBody>
            {rules.length === 0 ? (
              <TableRow>
                <TableCell
                  className="py-8 text-center text-muted-foreground"
                  colSpan={8}
                >
                  {loading ? t('loading') : t('noPriceRules')}
                </TableCell>
              </TableRow>
            ) : (
              rules.map((rule) => {
                const pendingAction = asyncAction.pendingOrigin(
                  billingPriceRuleActionKey(rule.price_rule_id),
                )
                const pending = pendingAction !== null
                return (
                  <TableRow key={rule.price_rule_id}>
                    <TableCell className="max-w-64 break-words font-medium">
                      {rule.public_model}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      {formatBillingRate(rule.input_rate, rule.currency)}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      {formatBillingRate(rule.cache_read_rate, rule.currency)}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      {formatBillingRate(rule.cache_write_rate, rule.currency)}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      {formatBillingRate(rule.output_rate, rule.currency)}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      {formatDateTime(
                        rule.effective_from,
                        locale,
                        undefined,
                        timeZone.effective,
                      )}
                    </TableCell>
                    <TableCell>
                      <Badge variant={rule.enabled ? 'default' : 'outline'}>
                        {rule.enabled ? t('active') : t('disabled')}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      <div className="flex justify-end gap-1 whitespace-nowrap">
                        <Button
                          aria-label={`${t('editPriceRule')} ${rule.public_model}`}
                          disabled={pending}
                          onClick={() => onEdit(rule)}
                          size="icon-sm"
                          type="button"
                          variant="ghost"
                        >
                          <Pencil aria-hidden="true" />
                        </Button>
                        <Button
                          aria-label={`${t('deletePriceRule')} ${rule.public_model}`}
                          disabled={pending}
                          onClick={() => onDelete(rule)}
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
                            rule.enabled
                              ? t('disablePriceRule')
                              : t('enablePriceRule')
                          } ${rule.public_model}`}
                          disabled={pending}
                          onClick={() => onToggle(rule)}
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
                    </TableCell>
                  </TableRow>
                )
              })
            )}
          </TableBody>
        </Table>
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
