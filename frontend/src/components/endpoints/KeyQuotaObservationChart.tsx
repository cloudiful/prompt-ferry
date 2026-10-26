import { lazy, Suspense, useEffect, useMemo, useState } from 'react'
import { LoaderCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import type {
  CanonicalQuotaWindowKind,
  QuotaKeyObservationTrendResponse,
} from '@/generated/admin-api'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import type { EChartsOption } from 'echarts'
import { useLocale } from '@/lib/i18n'
import { useThemeMode } from '@/lib/theme'
import { formatDateOnly, formatDateTime } from '@/format/datetime-core'
import {
  createQuotaTrendPanelOption,
  groupQuotaTrendSeries,
  quotaTrendLegendEntries,
  type QuotaTrendRange,
} from '@/quota-observation-charts-core'
import { chartThemeFor } from '@/theme/chart-tokens'

const UsageChart = lazy(() =>
  import('@/components/shared/UsageChart').then((module) => ({
    default: module.UsageChart,
  })),
)

export type KeyQuotaObservationChartProps = {
  failed: boolean
  loading: boolean
  onRetry: () => void
  range: QuotaTrendRange
  trend: QuotaKeyObservationTrendResponse | null
}

function pluralMessage(count: number, message: string): string {
  const [singular, plural] = message.split(' | ')
  if (plural === undefined) return message
  return count === 1 ? (singular ?? message) : plural
}

export function KeyQuotaObservationChart({
  failed,
  loading,
  onRetry,
  range,
  trend,
}: KeyQuotaObservationChartProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const { mode } = useThemeMode()
  const [hiddenByGroup, setHiddenByGroup] = useState<Record<string, string[]>>(
    {},
  )

  useEffect(() => {
    setHiddenByGroup({})
  }, [trend])

  const optionInput = useMemo(() => {
    const kinds: Record<CanonicalQuotaWindowKind, string> = {
      five_hour: t('tokenPlanInterval'),
      rolling: t('tokenPlanRolling'),
      weekly: t('tokenPlanWeekly'),
      monthly: t('tokenPlanMonthly'),
      balance: t('quotaTrendKindBalance'),
      unlimited: t('tokenPlanNoLimit'),
      custom: t('quotaTrendKindCustom'),
    }
    return {
      range,
      formatAxisDate: (value: string) =>
        formatDateTime(
          value,
          locale,
          { month: 'numeric', day: 'numeric' },
          timeZone.effective,
        ),
      formatAxisTime: (value: string) =>
        formatDateTime(
          value,
          locale,
          { hour: '2-digit', minute: '2-digit' },
          timeZone.effective,
        ),
      formatTooltipTime: (value: string) =>
        formatDateTime(value, locale, undefined, timeZone.effective),
      formatAmount: (value: number, unit: string) =>
        `${value.toFixed(2)}${unit ? ` ${unit}` : ''}`,
      formatDayKey: (value: string | number | Date) =>
        formatDateOnly(value, locale, undefined, timeZone.effective),
      theme: chartThemeFor(mode),
      labels: {
        percentSuffix: '%',
        unavailable: t('quotaTrendUnavailable'),
        kinds,
      },
    }
  }, [locale, mode, range, t, timeZone.effective])

  const groups = useMemo(
    () => groupQuotaTrendSeries(trend?.series ?? []),
    [trend],
  )
  const panels = useMemo(
    () =>
      groups
        .map((group) => {
          const entries = quotaTrendLegendEntries(group, optionInput)
          const hidden = new Set(hiddenByGroup[group.key] ?? [])
          return {
            entries,
            group,
            hidden,
            option: createQuotaTrendPanelOption(group, {
              ...optionInput,
              hiddenSeries: hidden,
            }),
          }
        })
        .filter((panel) =>
          panel.group.series.some((series) =>
            series.points.some((point) => point.value != null),
          ),
        ),
    [groups, hiddenByGroup, optionInput],
  )

  function toggleSeries(groupKey: string, name: string): void {
    setHiddenByGroup((current) => {
      const hidden = new Set(current[groupKey] ?? [])
      if (hidden.has(name)) hidden.delete(name)
      else hidden.add(name)
      return { ...current, [groupKey]: [...hidden] }
    })
  }

  function panelLabel(unit: string, metric: string): string {
    if (metric === 'remaining_percent') return t('quotaTrendRemainingQuota')
    return unit ? t('quotaTrendBalance', { unit }) : t('quotaTrendKindBalance')
  }

  return (
    <div className="grid min-w-0 gap-3">
      {trend &&
      (trend.failed_observations > 0 ||
        trend.keys_truncated ||
        trend.series_truncated) ? (
        <div className="flex flex-wrap gap-x-3 gap-y-1 text-xs">
          {trend.failed_observations > 0 ? (
            <span className="text-amber-700 dark:text-amber-300">
              {pluralMessage(
                trend.failed_observations,
                t('quotaTrendFailed', { count: trend.failed_observations }),
              )}
            </span>
          ) : null}
          {trend.keys_truncated ? (
            <span className="text-amber-700 dark:text-amber-300">
              {t('quotaTrendKeysTruncated')}
            </span>
          ) : null}
          {trend.series_truncated ? (
            <span className="text-amber-700 dark:text-amber-300">
              {t('quotaTrendSeriesTruncated')}
            </span>
          ) : null}
        </div>
      ) : null}

      {loading ? (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <LoaderCircle aria-hidden="true" className="size-4 animate-spin" />
          <span>{t('quotaTrendLoading')}</span>
        </div>
      ) : failed ? (
        <p className="text-sm text-destructive">
          {t('quotaTrendError')}{' '}
          <Button onClick={onRetry} size="sm" type="button" variant="link">
            {t('quotaTrendRetry')}
          </Button>
        </p>
      ) : panels.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t('quotaTrendEmpty')}</p>
      ) : (
        <div className="grid min-w-0 gap-3">
          {panels.map((panel) => (
            <section
              className="grid min-w-0 gap-2 rounded-md border p-2"
              key={panel.group.key}
            >
              <h4 className="text-xs font-medium">
                {panelLabel(panel.group.unit, panel.group.metric)}
              </h4>
              <div
                aria-label={t('quotaTrendLegendLabel')}
                className="max-h-24 overflow-y-auto"
                role="group"
              >
                <div className="flex min-w-0 flex-wrap gap-x-1 gap-y-1">
                  {panel.entries.map((entry) => {
                    const isVisible = !panel.hidden.has(entry.name)
                    return (
                      <Button
                        aria-pressed={isVisible}
                        className="h-auto min-w-0 max-w-full items-start justify-start whitespace-normal px-1.5 py-0.5 text-left text-xs"
                        key={entry.name}
                        onClick={() =>
                          toggleSeries(panel.group.key, entry.name)
                        }
                        type="button"
                        variant="ghost"
                      >
                        <span
                          aria-hidden="true"
                          className="mt-1 size-2 shrink-0 rounded-full"
                          style={{ backgroundColor: entry.color }}
                        />
                        <span
                          className={
                            isVisible
                              ? 'min-w-0 break-words'
                              : 'min-w-0 break-words line-through text-muted-foreground'
                          }
                        >
                          {entry.name}
                        </span>
                      </Button>
                    )
                  })}
                </div>
              </div>
              <Suspense
                fallback={
                  <div className="flex h-[20rem] items-center justify-center text-sm text-muted-foreground">
                    <LoaderCircle
                      aria-hidden="true"
                      className="size-4 animate-spin"
                    />
                  </div>
                }
              >
                <UsageChart option={panel.option as EChartsOption} />
              </Suspense>
            </section>
          ))}
        </div>
      )}
    </div>
  )
}
