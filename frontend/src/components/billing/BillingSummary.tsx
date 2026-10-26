import type {
  BillingBreakdownResponse,
  BillingSummaryResponse,
} from '@/generated/admin-api'
import { Badge } from '@/components/ui/badge'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { useLocale } from '@/lib/i18n'
import { formatNumber } from '@/format/datetime-core'
import { formatTokenQuantity } from '@/format/quantity'
import { formatBillingAmount } from '@/lib/billing-model'

type BillingSummaryProps = {
  summary: BillingSummaryResponse | null
}

function BreakdownTable({
  rows,
  title,
}: {
  rows: BillingBreakdownResponse[]
  title: string
}) {
  const { locale, t } = useLocale()

  return (
    <section className="grid min-w-0 gap-2 rounded-lg border p-3">
      <h3 className="m-0 text-sm font-semibold">{title}</h3>
      <div className="min-w-0 overflow-x-auto">
        <Table className="min-w-[52rem]">
          <TableHeader>
            <TableRow>
              <TableHead>{t('model')}</TableHead>
              <TableHead>{t('billingRequests')}</TableHead>
              <TableHead>{t('inputRate')}</TableHead>
              <TableHead>{t('cacheReadRate')}</TableHead>
              <TableHead>{t('cacheWriteRate')}</TableHead>
              <TableHead>{t('outputRate')}</TableHead>
              <TableHead>{t('chargeAmount')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.length === 0 ? (
              <TableRow>
                <TableCell
                  className="py-6 text-center text-muted-foreground"
                  colSpan={7}
                >
                  {t('noData')}
                </TableCell>
              </TableRow>
            ) : (
              rows.map((row) => (
                <TableRow key={row.grouping_key}>
                  <TableCell className="max-w-64 break-words">
                    {row.grouping_key}
                  </TableCell>
                  <TableCell>
                    {formatNumber(row.request_count, locale)}
                  </TableCell>
                  <TableCell>
                    {formatTokenQuantity(row.input_tokens, locale)}
                  </TableCell>
                  <TableCell>
                    {formatTokenQuantity(row.cache_read_tokens, locale)}
                  </TableCell>
                  <TableCell>
                    {formatTokenQuantity(row.cache_write_tokens, locale)}
                  </TableCell>
                  <TableCell>
                    {formatTokenQuantity(row.output_tokens, locale)}
                  </TableCell>
                  <TableCell>
                    {formatBillingAmount(row.customer_amount)}
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </div>
    </section>
  )
}

export function BillingSummary({ summary }: BillingSummaryProps) {
  const { locale, t } = useLocale()
  const currency = summary?.currency ?? 'CNY'

  return (
    <section className="grid min-w-0 gap-3">
      <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
        <div className="rounded-lg border p-3">
          <div className="text-xs text-muted-foreground">
            {t('billingAmount')}
          </div>
          <strong className="mt-1 block text-xl">
            {formatBillingAmount(summary?.customer_amount, currency)}
          </strong>
        </div>
        <div className="rounded-lg border p-3">
          <div className="text-xs text-muted-foreground">
            {t('billingRequests')}
          </div>
          <strong className="mt-1 block text-xl">
            {formatNumber(summary?.request_count ?? 0, locale)}
          </strong>
        </div>
        <div className="rounded-lg border p-3">
          <div className="text-xs text-muted-foreground">
            {t('billingKnown')}
          </div>
          <strong className="mt-1 block text-xl">
            {formatNumber(summary?.known_count ?? 0, locale)}
          </strong>
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant="outline">
          {`${t('billingPriced')}: ${summary?.priced_count ?? 0}`}
        </Badge>
        <Badge
          className="border-amber-600/40 text-amber-700 dark:text-amber-300"
          variant="outline"
        >
          {`${t('billingUnpriced')}: ${summary?.unpriced_count ?? 0}`}
        </Badge>
        <Badge variant="secondary">
          {`${t('billingUnknown')}: ${summary?.unknown_count ?? 0}`}
        </Badge>
      </div>
      <div className="grid min-w-0 gap-3">
        <BreakdownTable
          rows={summary?.by_client_key ?? []}
          title={t('billingClientKeys')}
        />
        <BreakdownTable
          rows={summary?.by_model ?? []}
          title={t('billingModels')}
        />
      </div>
    </section>
  )
}
