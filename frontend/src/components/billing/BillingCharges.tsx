import { ReceiptText } from 'lucide-react'
import type { BillingChargeResponse } from '@/generated/admin-api'
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
import { TablePagination } from '@/components/shared/TablePagination'
import { useLocale } from '@/lib/i18n'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { formatDateTime } from '@/format/datetime-core'
import { formatTokenQuantity } from '@/format/quantity'
import { formatBillingAmount } from '@/lib/billing-model'
import { BILLING_PAGE_SIZE_OPTIONS } from '@/lib/billing-model'
import type { TablePageChange } from '@/models/pagination'

type BillingChargesProps = {
  charges: BillingChargeResponse[]
  first: number
  isAdmin: boolean
  loading: boolean
  rows: number
  total: number
  onOpenDetail: (charge: BillingChargeResponse) => void
  onPage: (change: TablePageChange) => void
}

export function BillingCharges({
  charges,
  first,
  isAdmin,
  loading,
  rows,
  total,
  onOpenDetail,
  onPage,
}: BillingChargesProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const columnCount = isAdmin ? 9 : 7

  function statusLabel(charge: BillingChargeResponse): string {
    if (charge.pricing_status === 'priced') return t('pricingPriced')
    if (charge.usage_status === 'unknown') return t('usageUnknown')
    return t('pricingUnpriced')
  }

  function statusVariant(
    charge: BillingChargeResponse,
  ): 'default' | 'outline' | 'secondary' {
    if (charge.pricing_status === 'priced') return 'default'
    if (charge.usage_status === 'unknown') return 'outline'
    return 'secondary'
  }

  return (
    <section className="grid min-w-0 max-w-full gap-3 rounded-lg border p-3">
      <h2 className="m-0 text-sm font-semibold">{t('billingCharges')}</h2>
      <div className="min-w-0 max-w-full overflow-x-auto">
        <Table aria-busy={loading} className="min-w-[60rem]">
          <TableHeader>
            <TableRow>
              <TableHead>{t('billingDetails')}</TableHead>
              {isAdmin && <TableHead>{t('user')}</TableHead>}
              <TableHead>{t('clientKey')}</TableHead>
              <TableHead>{t('publicModel')}</TableHead>
              {isAdmin && <TableHead>{t('endpoint')}</TableHead>}
              <TableHead>{t('usageTokens')}</TableHead>
              <TableHead>{t('billingStatus')}</TableHead>
              <TableHead>{t('chargeAmount')}</TableHead>
              <TableHead>{t('createdAt')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {charges.length === 0 ? (
              <TableRow>
                <TableCell
                  className="py-8 text-center text-muted-foreground"
                  colSpan={columnCount}
                >
                  {loading ? t('loading') : t('noBillingCharges')}
                </TableCell>
              </TableRow>
            ) : (
              charges.map((charge) => (
                <TableRow key={charge.charge_id}>
                  <TableCell>
                    <Button
                      aria-label={`${t('billingDetails')} ${charge.request_id}`}
                      onClick={() => onOpenDetail(charge)}
                      size="icon-sm"
                      type="button"
                      variant="ghost"
                    >
                      <ReceiptText aria-hidden="true" />
                    </Button>
                  </TableCell>
                  {isAdmin && (
                    <TableCell className="max-w-48 break-words">
                      {charge.user_login_name || '-'}
                    </TableCell>
                  )}
                  <TableCell className="max-w-48 break-words">
                    {charge.client_key_label || '-'}
                  </TableCell>
                  <TableCell className="max-w-56 break-words">
                    {charge.requested_model || '-'}
                  </TableCell>
                  {isAdmin && (
                    <TableCell className="max-w-56 break-words">
                      {charge.endpoint_name || '-'}
                    </TableCell>
                  )}
                  <TableCell className="whitespace-nowrap">
                    {`${formatTokenQuantity(charge.input_tokens, locale)} / ${formatTokenQuantity(charge.cache_read_tokens, locale)} / ${formatTokenQuantity(charge.cache_write_tokens, locale)} / ${formatTokenQuantity(charge.output_tokens, locale)}`}
                  </TableCell>
                  <TableCell>
                    <Badge variant={statusVariant(charge)}>
                      {statusLabel(charge)}
                    </Badge>
                  </TableCell>
                  <TableCell className="whitespace-nowrap">
                    {formatBillingAmount(
                      charge.customer_amount,
                      charge.currency,
                    )}
                  </TableCell>
                  <TableCell className="whitespace-nowrap">
                    {formatDateTime(
                      charge.created_at,
                      locale,
                      undefined,
                      timeZone.effective,
                    )}
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </div>
      <TablePagination
        first={first}
        onChange={onPage}
        pageSizeOptions={BILLING_PAGE_SIZE_OPTIONS}
        rows={rows}
        total={total}
      />
    </section>
  )
}
