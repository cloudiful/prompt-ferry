import type { CanonicalQuotaRow } from '@/composables/useCanonicalQuotaWindows'
import type { SubscriptionQuotaObservation } from '@/generated/admin-api'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { useLocale, type Translate } from '@/lib/i18n'
import { formatDateTime } from '@/format/datetime-core'
import { TokenPlanQuotaWindowRow } from '@/components/endpoints/TokenPlanQuotaWindowRow'

export type SubscriptionQuotaModelGroup = {
  modelName: string
  observation: SubscriptionQuotaObservation | null
  rows: CanonicalQuotaRow[]
}

export type SubscriptionQuotaWindowsProps = {
  groups: SubscriptionQuotaModelGroup[]
  nowMs: number
}

function formatDuration(seconds: number): string {
  const week = 7 * 24 * 60 * 60
  const day = 24 * 60 * 60
  if (seconds % week === 0) return `${seconds / week}w`
  if (seconds % day === 0) return `${seconds / day}d`
  if (seconds % 3600 === 0) return `${seconds / 3600}h`
  if (seconds % 60 === 0) return `${seconds / 60}m`
  return `${seconds}s`
}

export function quotaWindowLabel(row: CanonicalQuotaRow, t: Translate): string {
  if (row.kind === 'custom') {
    const duration = row.durationSeconds
    if (duration === 5 * 60 * 60) return t('tokenPlanInterval')
    if (duration === 7 * 24 * 60 * 60) return t('tokenPlanWeekly')
    if (duration === null || duration <= 0) {
      return t('tokenPlanWindowDurationUnknown')
    }
    return t('tokenPlanDurationWindow', { duration: formatDuration(duration) })
  }
  if (row.sourceId.startsWith('command_code:')) {
    return row.kind === 'weekly'
      ? t('tokenPlanWeeklyUsd')
      : t('tokenPlanFiveHour')
  }
  if (row.sourceId.startsWith('glm:')) {
    return row.kind === 'weekly' ? t('tokenPlanWeekly') : t('tokenPlanInterval')
  }
  if (row.sourceId.startsWith('opencode_go:')) {
    if (row.kind === 'rolling') return t('tokenPlanRolling')
    if (row.kind === 'weekly') return t('tokenPlanWeekly')
    return t('tokenPlanMonthly')
  }
  if (row.kind === 'balance') {
    if (row.sourceId.startsWith('deepseek:')) {
      return row.unit
        ? `${t('tokenPlanDeepSeekBalance')} · ${row.unit}`
        : t('tokenPlanDeepSeekBalance')
    }
    if (row.sourceId.startsWith('openrouter:')) {
      return t('tokenPlanOpenRouterBalance')
    }
    return t('tokenPlanBalances')
  }
  if (row.kind === 'unlimited') return t('tokenPlanNoLimit')
  return row.kind === 'weekly' || row.kind === 'monthly'
    ? t('tokenPlanWeekly')
    : t('tokenPlanInterval')
}

function observationSource(
  observation: SubscriptionQuotaObservation,
  t: Translate,
): string | null {
  if (observation.source === 'manual') return t('quotaSourceManual')
  if (observation.source === 'request') return t('quotaSourceRequest')
  if (observation.source === 'periodic') return t('quotaSourcePeriodic')
  return null
}

export function SubscriptionQuotaWindows({
  groups,
  nowMs,
}: SubscriptionQuotaWindowsProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  if (groups.length === 0) return null

  return (
    <div className="@container grid min-w-0 grid-cols-[minmax(0,1fr)] gap-3 @[40rem]:grid-cols-2">
      {groups.map((group) => {
        const observation = group.observation
        const observedAt = observation?.observed_at
        const retryAt = observation?.next_retry_at
        const source = observation ? observationSource(observation, t) : null
        return (
          <section
            className="grid min-w-0 content-start gap-2 rounded-md border p-3"
            data-quota-model={group.modelName}
            key={group.modelName}
          >
            <h3 className="break-words font-medium text-foreground">
              {group.modelName}
            </h3>
            {observation ? (
              <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 text-xs">
                {observedAt ? (
                  <span className="text-muted-foreground">
                    {t('quotaObservedAt', {
                      time: formatDateTime(
                        observedAt,
                        locale,
                        { dateStyle: 'medium', timeStyle: 'short' },
                        timeZone.effective,
                      ),
                    })}
                  </span>
                ) : null}
                {source ? (
                  <span className="text-muted-foreground">{source}</span>
                ) : null}
                {observation.refreshing ? (
                  <span className="text-muted-foreground">
                    {t('quotaRefreshing')}
                  </span>
                ) : null}
                {observation.stale ? (
                  <span className="text-amber-700 dark:text-amber-300">
                    {t('quotaCachedObservation')}
                  </span>
                ) : null}
                {observation.last_error_code ? (
                  <span className="text-amber-700 dark:text-amber-300">
                    {t('quotaRefreshFailedCached')}
                  </span>
                ) : null}
                {retryAt ? (
                  <span className="text-muted-foreground">
                    {t('quotaRetryAt', {
                      time: formatDateTime(
                        retryAt,
                        locale,
                        { dateStyle: 'medium', timeStyle: 'short' },
                        timeZone.effective,
                      ),
                    })}
                  </span>
                ) : null}
              </div>
            ) : null}
            {group.rows.map((row) => (
              <TokenPlanQuotaWindowRow
                key={row.sourceId}
                label={quotaWindowLabel(row, t)}
                nowMs={nowMs}
                quantitySuffix={row.unit === 'currency' ? ' USD' : undefined}
                row={row}
              />
            ))}
          </section>
        )
      })}
    </div>
  )
}
