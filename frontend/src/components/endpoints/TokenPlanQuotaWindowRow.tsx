import type { CanonicalQuotaRow } from '@/composables/useCanonicalQuotaWindows'
import { canonicalQuantityCaption } from '@/composables/useCanonicalQuotaWindows'
import { useLocale } from '@/lib/i18n'
import {
  formatResetDuration,
  knownRemainingPercent,
  progressColor,
  remainingPercent,
} from '@/models/endpoints/token-plan-window'

export type TokenPlanQuotaWindowRowProps = {
  label: string
  nowMs: number
  quantitySuffix?: string
  row: CanonicalQuotaRow
}

export function TokenPlanQuotaWindowRow({
  label,
  nowMs,
  quantitySuffix,
  row,
}: TokenPlanQuotaWindowRowProps) {
  const { t } = useLocale()
  const adapted = {
    end_at: row.resetAt,
    remaining_percent: row.remainingPercent,
  }
  const remaining = knownRemainingPercent(adapted)
  const resetAt = row.resetAt ? Date.parse(row.resetAt) : Number.NaN
  const resetMs = Number.isFinite(resetAt) ? resetAt - nowMs : null
  const caption = canonicalQuantityCaption(row)
  const isTimeWindow = row.kind !== 'balance' && row.kind !== 'unlimited'

  if (!isTimeWindow) {
    const value =
      row.kind === 'unlimited'
        ? t('tokenPlanNoLimit')
        : caption === null
          ? t('tokenPlanUsageUnknown')
          : `${caption}${quantitySuffix ?? ''}`
    return (
      <div className="@container min-w-0" data-quota-row={row.sourceId}>
        <div className="grid min-w-0 gap-1 @[20rem]:grid-cols-[minmax(7rem,auto)_minmax(0,1fr)] @[20rem]:items-center @[20rem]:gap-3">
          <span className="break-words text-muted-foreground">{label}</span>
          <span className="break-words">{value}</span>
        </div>
      </div>
    )
  }

  const percentage = remaining === null ? null : remainingPercent(adapted)
  const resetLabel = formatResetDuration(resetMs, t as unknown as TranslateFn)
  const percentageLabel =
    percentage === null
      ? '—'
      : `${t('quotaRemainingPercent')} ${percentage.toFixed(1)}%`

  return (
    <div className="@container min-w-0" data-quota-row={row.sourceId}>
      <div className="grid min-w-0 gap-1.5 @[26rem]:grid-cols-[minmax(7rem,auto)_minmax(0,1fr)_minmax(8.5rem,auto)] @[26rem]:items-center @[26rem]:gap-3">
        <span className="break-words text-muted-foreground">{label}</span>
        {percentage === null ? (
          <span aria-hidden="true" className="h-1.5" />
        ) : (
          <div
            aria-label={label}
            aria-valuemax={100}
            aria-valuemin={0}
            aria-valuenow={percentage}
            className="h-1.5 w-full overflow-hidden rounded-full bg-muted"
            role="progressbar"
          >
            <div
              className="h-full rounded-full"
              style={{
                backgroundColor: progressColor(adapted),
                width: `${percentage}%`,
              }}
            />
          </div>
        )}
        <div className="flex min-w-0 items-center justify-between gap-2 text-xs @[26rem]:justify-end">
          <span className="min-w-0 break-words text-muted-foreground">
            {resetLabel}
          </span>
          <span className="shrink-0 font-semibold whitespace-nowrap">
            {percentageLabel}
          </span>
        </div>
        {caption !== null ? (
          <span className="break-words text-muted-foreground @[26rem]:col-span-2 @[26rem]:col-start-2">
            {caption}
            {quantitySuffix ?? ''}
          </span>
        ) : null}
      </div>
    </div>
  )
}
