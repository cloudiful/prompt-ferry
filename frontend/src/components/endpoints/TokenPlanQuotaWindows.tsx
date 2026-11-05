import { TokenPlanQuotaWindowRow } from '@/components/endpoints/TokenPlanQuotaWindowRow'
import {
  toCanonicalQuotaRows,
  type CanonicalQuotaRow,
} from '@/composables/useCanonicalQuotaWindows'
import type {
  SubscriptionQuotaObservation,
  TokenPlanKeyUsage,
} from '@/generated/admin-api'
import { formatDateTime } from '@/format/datetime-core'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { useLocale, type Translate } from '@/lib/i18n'

export type TokenPlanQuotaGroup = {
  modelName: string | null
  isDefaultScope: boolean
  observation: SubscriptionQuotaObservation | null
  rows: CanonicalQuotaRow[]
}

export type TokenPlanQuotaWindowsProps = {
  keyUsage: TokenPlanKeyUsage
  muted?: boolean
  nowMs: number
}

function isTimeWindow(row: CanonicalQuotaRow): boolean {
  return (
    row.kind === 'five_hour' ||
    row.kind === 'rolling' ||
    row.kind === 'weekly' ||
    row.kind === 'monthly' ||
    row.kind === 'custom'
  )
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
  switch (row.kind) {
    case 'five_hour':
      return t('tokenPlanInterval')
    case 'rolling':
      return t('tokenPlanRolling')
    case 'weekly':
      return t('tokenPlanWeekly')
    case 'monthly':
      return t('tokenPlanMonthly')
    case 'unlimited':
      return t('tokenPlanNoLimit')
    default:
      return t('tokenPlanInterval')
  }
}

export function quotaModelGroups(
  keyUsage: TokenPlanKeyUsage,
  nowMs: number,
): TokenPlanQuotaGroup[] {
  const groups = new Map<string | null, TokenPlanQuotaGroup>()
  for (const row of toCanonicalQuotaRows(keyUsage, nowMs)) {
    if (!isTimeWindow(row)) continue
    const modelName = row.sourceModel
    const existing = groups.get(modelName)
    if (existing) {
      existing.rows.push(row)
      existing.isDefaultScope &&= row.isDefaultScope
      continue
    }
    groups.set(modelName, {
      modelName,
      isDefaultScope: row.isDefaultScope,
      observation: null,
      rows: [row],
    })
  }

  for (const model of keyUsage.model_remains) {
    if (model.observation == null) continue
    const existing = groups.get(model.model_name)
    if (existing) {
      existing.observation = model.observation
      continue
    }
    groups.set(model.model_name, {
      modelName: model.model_name,
      isDefaultScope: true,
      observation: model.observation,
      rows: [],
    })
  }

  return [...groups.values()]
}

export function TokenPlanQuotaWindows({
  keyUsage,
  muted = false,
  nowMs,
}: TokenPlanQuotaWindowsProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const groups = quotaModelGroups(keyUsage, nowMs)
  if (groups.length === 0) return null

  return (
    <div className="@container grid min-w-0 gap-3">
      {groups.map((group) => {
        const observation = group.observation
        const retryAt = observation?.next_retry_at
        const showHeading =
          groups.length > 1 ||
          (group.modelName !== null && !group.isDefaultScope)
        return (
          <section
            className="grid min-w-0 content-start gap-2"
            data-quota-model={group.modelName ?? ''}
            data-quota-muted={muted ? 'true' : 'false'}
            key={group.modelName ?? 'default'}
          >
            {showHeading && group.modelName !== null ? (
              <h3
                className={
                  muted
                    ? 'break-words text-xs font-medium text-muted-foreground'
                    : 'break-words text-xs font-medium text-foreground'
                }
              >
                {group.modelName}
              </h3>
            ) : null}
            {observation &&
            (observation.stale || observation.last_error_code || retryAt) ? (
              <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 text-xs">
                {observation.stale ? (
                  <span className="text-amber-700 dark:text-amber-300">
                    {t('quotaCachedObservation')}
                  </span>
                ) : null}
                {observation.last_error_code ? (
                  <span className="break-words text-amber-700 dark:text-amber-300">
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
            {group.rows.length > 0 ? (
              <div className="grid min-w-0 grid-cols-[repeat(auto-fit,minmax(min(100%,12rem),1fr))] gap-2">
                {group.rows.map((row) => (
                  <TokenPlanQuotaWindowRow
                    key={row.sourceId}
                    label={quotaWindowLabel(row, t as Translate)}
                    modelName={row.sourceModel}
                    muted={muted}
                    nowMs={nowMs}
                    quantitySuffix={
                      row.unit === 'currency' ? ' USD' : undefined
                    }
                    row={row}
                  />
                ))}
              </div>
            ) : null}
          </section>
        )
      })}
    </div>
  )
}
