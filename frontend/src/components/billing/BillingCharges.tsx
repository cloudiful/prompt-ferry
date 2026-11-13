import { useMemo, useRef } from 'react'
import { ReceiptText } from 'lucide-react'
import type { BillingChargeResponse } from '@/generated/admin-api'
import {
  DataTable,
  type DataTableColumnDef,
} from '@/components/shared/DataTable'
import { TablePagination } from '@/components/shared/TablePagination'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { useLocale } from '@/lib/i18n'
import { formatDateTime } from '@/format/datetime-core'
import { formatTokenQuantity } from '@/format/quantity'
import {
  BILLING_PAGE_SIZE_OPTIONS,
  formatBillingAmount,
} from '@/lib/billing-model'
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
  const callbacks = useRef({ onOpenDetail })
  callbacks.current = { onOpenDetail }

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

  const columns = useMemo<Array<DataTableColumnDef<BillingChargeResponse>>>(
    () => [
      {
        id: 'details',
        header: t('billingDetails'),
        cell: ({ row }) => (
          <Button
            aria-label={`${t('billingDetails')} ${row.original.request_id}`}
            onClick={() => callbacks.current.onOpenDetail(row.original)}
            size="icon-sm"
            type="button"
            variant="ghost"
          >
            <ReceiptText aria-hidden="true" />
          </Button>
        ),
      },
      ...(isAdmin
        ? [
            {
              id: 'user',
              header: t('user'),
              cell: ({ row }) => (
                <div className="max-w-48 break-words">
                  {row.original.user_login_name || '-'}
                </div>
              ),
            } satisfies DataTableColumnDef<BillingChargeResponse>,
          ]
        : []),
      {
        id: 'client-key',
        header: t('clientKey'),
        cell: ({ row }) => (
          <div className="max-w-48 break-words">
            {row.original.client_key_label || '-'}
          </div>
        ),
      },
      {
        id: 'public-model',
        header: t('publicModel'),
        cell: ({ row }) => (
          <div className="max-w-56 break-words">
            {row.original.requested_model || '-'}
          </div>
        ),
      },
      ...(isAdmin
        ? [
            {
              id: 'endpoint',
              header: t('endpoint'),
              cell: ({ row }) => (
                <div className="max-w-56 break-words">
                  {row.original.endpoint_name || '-'}
                </div>
              ),
            } satisfies DataTableColumnDef<BillingChargeResponse>,
          ]
        : []),
      {
        id: 'usage-tokens',
        header: t('usageTokens'),
        cell: ({ row }) => {
          const charge = row.original
          return (
            <div className="whitespace-nowrap">
              {`${formatTokenQuantity(charge.input_tokens, locale)} / ${formatTokenQuantity(charge.cache_read_tokens, locale)} / ${formatTokenQuantity(charge.cache_write_tokens, locale)} / ${formatTokenQuantity(charge.output_tokens, locale)}`}
            </div>
          )
        },
      },
      {
        id: 'billing-status',
        header: t('billingStatus'),
        cell: ({ row }) => (
          <Badge variant={statusVariant(row.original)}>
            {statusLabel(row.original)}
          </Badge>
        ),
      },
      {
        id: 'charge-amount',
        header: t('chargeAmount'),
        cell: ({ row }) => (
          <div className="whitespace-nowrap">
            {formatBillingAmount(
              row.original.customer_amount,
              row.original.currency,
            )}
          </div>
        ),
      },
      {
        id: 'created-at',
        header: t('createdAt'),
        cell: ({ row }) => (
          <div className="whitespace-nowrap">
            {formatDateTime(
              row.original.created_at,
              locale,
              undefined,
              timeZone.effective,
            )}
          </div>
        ),
      },
    ],
    [isAdmin, locale, t, timeZone.effective],
  )

  return (
    <section
      aria-busy={loading}
      className="grid min-w-0 max-w-full gap-3 rounded-lg border p-3"
    >
      <h2 className="m-0 text-sm font-semibold">{t('billingCharges')}</h2>
      <div className="min-w-0 max-w-full overflow-x-auto">
        <DataTable
          className="min-w-[60rem]"
          columns={columns}
          data={charges}
          emptyLabel={t('noBillingCharges')}
          getRowId={(charge) => String(charge.charge_id)}
          label={t('billingCharges')}
          loading={loading}
        />
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
