import type { CanonicalQuotaRow } from '@/composables/useCanonicalQuotaWindows'
import { canonicalQuantityCaption } from '@/composables/useCanonicalQuotaWindows'
import { useLocale } from '@/lib/i18n'
import {
  formatCompactResetCountdown,
  knownRemainingPercent,
  progressColor,
  remainingPercent,
  shortWindowLabel,
} from '@/models/endpoints/token-plan-window'

export type TokenPlanQuotaWindowRowProps = {
  label: string
  modelName?: string | null
  nowMs: number
  quantitySuffix?: string
  row: CanonicalQuotaRow
  showModelName?: boolean
}

function isTimeWindow(row: CanonicalQuotaRow): boolean {
  return row.kind !== 'balance' && row.kind !== 'unlimited'
}

export function TokenPlanQuotaWindowRow({
  label,
  modelName,
  nowMs,
  quantitySuffix,
  row,
  showModelName = false,
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
  const accessibleLabel = modelName ? `${modelName} · ${label}` : label

  if (!isTimeWindow(row)) {
    const value =
      row.kind === 'unlimited'
        ? t('tokenPlanNoLimit')
        : caption === null
          ? t('tokenPlanUsageUnknown')
          : `${caption}${quantitySuffix ?? ''}`
    return (
      <div className="@container min-w-0" data-quota-row={row.sourceId}>
        <div className="grid min-w-0 gap-1 @[20rem]:grid-cols-[minmax(7rem,auto)_minmax(0,1fr)] @[20rem]:items-center @[20rem]:gap-3">
          <span
            className="break-words text-muted-foreground"
            title={accessibleLabel}
          >
            {label}
          </span>
          <span className="break-words">{value}</span>
        </div>
      </div>
    )
  }

  const percentage = remaining === null ? null : remainingPercent(adapted)
  const resetLabel =
    remaining === null
      ? '-'
      : formatCompactResetCountdown(resetMs, t as unknown as TranslateFn)
  const shortLabel =
    shortWindowLabel(
      row.kind,
      row.durationSeconds,
      t as unknown as TranslateFn,
    ) ?? label
  const visibleLabel =
    showModelName && modelName ? `${modelName} · ${shortLabel}` : shortLabel

  return (
    <div className="@container min-w-0" data-quota-row={row.sourceId}>
      <div className="grid min-w-0 gap-1">
        <div className="flex min-w-0 items-baseline justify-between gap-2">
          <span
            className="min-w-0 break-words text-xs font-medium text-foreground"
            title={accessibleLabel}
          >
            {visibleLabel}
          </span>
          {caption !== null ? (
            <span className="min-w-0 break-words text-right text-muted-foreground">
              {caption}
              {quantitySuffix ?? ''}
            </span>
          ) : null}
        </div>
        {percentage === null ? (
          <div className="flex h-8 min-h-8 items-center justify-between gap-2 rounded-md border border-dashed px-2 text-xs text-muted-foreground">
            <span aria-hidden="true">—</span>
            <span>{resetLabel}</span>
          </div>
        ) : (
          <div
            aria-label={accessibleLabel}
            aria-valuemax={100}
            aria-valuemin={0}
            aria-valuenow={percentage}
            className="relative h-8 min-h-8 w-full overflow-hidden rounded-md bg-muted"
            role="progressbar"
          >
            <div
              className="h-full"
              style={{
                backgroundColor: progressColor(adapted),
                width: `${percentage}%`,
              }}
            />
            <div className="absolute inset-0 flex min-w-0 items-center justify-between gap-2 px-1.5 text-xs">
              <span className="rounded bg-background/85 px-1 font-semibold whitespace-nowrap text-foreground">
                {percentage.toFixed(1)}%
              </span>
              <span className="min-w-0 truncate rounded bg-background/85 px-1 text-muted-foreground">
                {resetLabel}
              </span>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
