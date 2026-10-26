import type { BillingChargeDetailResponse } from '@/generated/admin-api'
import { Badge } from '@/components/ui/badge'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useLocale } from '@/lib/i18n'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { formatDateTime } from '@/format/datetime-core'
import { formatTokenQuantity } from '@/format/quantity'
import { formatBillingAmount } from '@/lib/billing-model'

type BillingChargeDetailDialogProps = {
  detail: BillingChargeDetailResponse | null
  isAdmin: boolean
  loading: boolean
  onOpenChange: (open: boolean) => void
  open: boolean
}

export function BillingChargeDetailDialog({
  detail,
  isAdmin,
  loading,
  onOpenChange,
  open,
}: BillingChargeDetailDialogProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const charge = detail?.charge ?? null

  function statusLabel(): string {
    if (!charge) return ''
    if (charge.pricing_status === 'priced') return t('pricingPriced')
    if (charge.usage_status === 'unknown') return t('usageUnknown')
    return t('pricingUnpriced')
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>{t('billingDetails')}</DialogTitle>
        </DialogHeader>
        {loading ? (
          <div
            aria-label={t('loading')}
            className="py-6 text-sm text-muted-foreground"
            role="status"
          >
            {t('loading')}
          </div>
        ) : charge ? (
          <div className="grid gap-4 text-xs">
            <section className="grid gap-2 rounded-lg border p-3">
              <div className="flex flex-wrap items-center gap-2">
                <Badge variant="outline">{statusLabel()}</Badge>
                <span className="text-muted-foreground">
                  {charge.request_id}
                </span>
              </div>
              <div className="grid gap-2 sm:grid-cols-2">
                <div>
                  <span className="text-muted-foreground">
                    {t('usageTokens')}
                  </span>
                  <strong className="mt-1 block">
                    {`${formatTokenQuantity(charge.input_tokens, locale)} / ${formatTokenQuantity(charge.cache_read_tokens, locale)} / ${formatTokenQuantity(charge.cache_write_tokens, locale)} / ${formatTokenQuantity(charge.output_tokens, locale)}`}
                  </strong>
                </div>
                <div>
                  <span className="text-muted-foreground">
                    {t('chargeAmount')}
                  </span>
                  <strong className="mt-1 block">
                    {formatBillingAmount(
                      charge.customer_amount,
                      charge.currency,
                    )}
                  </strong>
                </div>
              </div>
              {isAdmin && (
                <div className="grid gap-2 text-muted-foreground sm:grid-cols-2">
                  <span>
                    {`${t('endpoint')}: ${charge.endpoint_name || charge.endpoint_id || '-'}`}
                  </span>
                  <span>
                    {`${t('upstreamKey')}: ${charge.endpoint_key_id || '-'}`}
                  </span>
                </div>
              )}
              <div className="text-muted-foreground">
                {formatDateTime(
                  charge.created_at,
                  locale,
                  undefined,
                  timeZone.effective,
                )}
              </div>
            </section>
            <section className="grid gap-2">
              <h3 className="m-0 font-semibold">{t('billingLines')}</h3>
              {detail?.lines.length === 0 ? (
                <p className="m-0 text-muted-foreground">{t('noData')}</p>
              ) : (
                detail?.lines.map((line) => (
                  <div
                    className="flex flex-wrap items-center justify-between gap-2 border-b py-2"
                    key={line.line_id}
                  >
                    <span>
                      {`${line.meter} / ${formatTokenQuantity(line.token_count, locale)}`}
                    </span>
                    <span>
                      {`${formatBillingAmount(line.amount)} (${formatBillingAmount(line.unit_rate)} / M)`}
                    </span>
                  </div>
                ))
              )}
            </section>
          </div>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}
