import type { EChartsOption } from 'echarts'
import type { RequestRecordOverviewResponse } from '@/generated/admin-api'
import { lazy, Suspense } from 'react'
import { LoaderCircle } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { BreakdownDetailPopover } from '@/components/usage/BreakdownDetailPopover'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { useLocale } from '@/lib/i18n'
import { chartThemeFor } from '@/theme/chart-tokens'
import { useThemeMode } from '@/lib/theme'
import {
  formatUsageCount,
  formatUsageInstant,
  formatUsageMs,
  formatUsagePercent,
  formatUsageTokenQuantity,
  formatUsageTokensPerSecond,
  formatUsageUtcDate,
} from '@/lib/usage-format'
import {
  usageDrilldownForPerspective,
  usageTrendBucketLabel,
  type UsageOverviewDrilldown,
} from '@/lib/usage-overview'
import {
  type UsageCategory,
  type UsagePerspective,
  type UsageScope,
} from '@/lib/usage-model'
import { createUsageTrendOption } from '@/request-overview-charts-core'

const UsageChart = lazy(() =>
  import('@/components/shared/UsageChart').then((module) => ({
    default: module.UsageChart,
  })),
)

type MetricKind = 'count' | 'ms' | 'ratio' | 'tokens' | 'tokensPerSecond'

type MetricCard = {
  label: string
  value: number | null | undefined
  kind: MetricKind
}

type RequestRecordsOverviewPanelProps = {
  overview: RequestRecordOverviewResponse | null
  loading: boolean
  category: UsageCategory
  scope: UsageScope
  perspective: UsagePerspective
  perspectivePending: boolean
  onDrilldown: (drilldown: UsageOverviewDrilldown) => void
  onChangePerspective: (perspective: UsagePerspective) => void
  onChangeScope: (scope: UsageScope) => void
}

function metricValue(
  card: MetricCard,
  locale: Parameters<typeof formatUsageCount>[1],
): string {
  if (card.kind === 'ratio') return formatUsagePercent(card.value)
  if (card.kind === 'ms') return formatUsageMs(card.value)
  if (card.kind === 'tokens')
    return formatUsageTokenQuantity(card.value, locale)
  if (card.kind === 'tokensPerSecond')
    return formatUsageTokensPerSecond(card.value)
  return formatUsageCount(card.value, locale)
}

/**
 * The AI/MCP overview surface: metric cards, the trend chart and the
 * distribution table with its drilldown and detail popover.
 *
 * The component is controlled end to end: the page owns the mount, refresh and
 * cache, so this surface renders the given overview and reports intent through
 * callbacks without issuing a query of its own.
 */
export function RequestRecordsOverviewPanel({
  overview,
  loading,
  category,
  scope,
  perspective,
  perspectivePending,
  onDrilldown,
  onChangePerspective,
  onChangeScope,
}: RequestRecordsOverviewPanelProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const { mode } = useThemeMode()
  const summary = overview?.summary ?? null
  const hasTraffic = (summary?.request_count ?? 0) > 0
  const rows = overview?.breakdown ?? []

  const cards: MetricCard[] = !summary
    ? []
    : category === 'mcp'
      ? [
          { label: t('requests'), value: summary.request_count, kind: 'count' },
          {
            label: t('overviewSuccessRate'),
            value: summary.success_rate,
            kind: 'ratio',
          },
          {
            label: t('overviewErrors'),
            value: summary.error_count,
            kind: 'count',
          },
          {
            label: t('overviewP95Latency'),
            value: summary.p95_total_ms,
            kind: 'ms',
          },
          {
            label: t('overviewMcpMethods'),
            value: summary.method_count,
            kind: 'count',
          },
        ]
      : [
          {
            label: t('overviewTotalTokens'),
            value: summary.tokens.total_tokens,
            kind: 'tokens',
          },
          {
            label: t('overviewInputTokens'),
            value: summary.tokens.input_tokens,
            kind: 'tokens',
          },
          {
            label: t('overviewOutputTokens'),
            value: summary.tokens.output_tokens,
            kind: 'tokens',
          },
          {
            label: t('overviewCacheRate'),
            value: summary.tokens.cache_rate,
            kind: 'ratio',
          },
          {
            label: t('overviewCacheHitRate'),
            value: summary.tokens.cache_hit_rate,
            kind: 'ratio',
          },
          {
            label: t('overviewAvgOutputRate'),
            value: summary.avg_output_tokens_per_second,
            kind: 'tokensPerSecond',
          },
          { label: t('requests'), value: summary.request_count, kind: 'count' },
          {
            label: t('overviewSuccessRate'),
            value: summary.success_rate,
            kind: 'ratio',
          },
          {
            label: t('overviewErrors'),
            value: summary.error_count,
            kind: 'count',
          },
          {
            label: t('overviewP95Latency'),
            value: summary.p95_total_ms,
            kind: 'ms',
          },
        ]

  const formatBucket = (value: string): string =>
    usageTrendBucketLabel(
      overview?.trend_granularity,
      value,
      (instant) =>
        formatUsageInstant(instant, locale, timeZone.effective, {
          day: '2-digit',
          hour: '2-digit',
          minute: '2-digit',
          month: '2-digit',
        }),
      (utcDate) => formatUsageUtcDate(utcDate, locale),
    )

  const trendOption = createUsageTrendOption({
    category,
    labels: {
      cacheRead: t('overviewCacheRead'),
      cacheRate: t('overviewCacheRate'),
      cacheWrite: t('overviewCacheWrite'),
      error: t('overviewError'),
      input: t('overviewInputTokens'),
      output: t('overviewOutputTokens'),
      requests: t('requests'),
      success: t('overviewSuccess'),
    },
    theme: chartThemeFor(mode),
    trend: overview?.trend ?? [],
    formatTime: formatBucket,
    formatCompact: (value?: number | null) =>
      formatUsageTokenQuantity(value, locale),
  })

  const distributionTitle =
    category === 'mcp'
      ? t('overviewMcpServerDistribution')
      : perspective === 'upstream'
        ? t('overviewUpstreamDistribution')
        : t('overviewModelDistribution')

  function providerBadge(row: (typeof rows)[number]): string {
    const provider = row.server_provider_kind ?? 'generic'
    return row.usage_unit ? `${provider} · ${row.usage_unit}` : provider
  }

  return (
    <div className="grid min-w-0 grid-cols-1 gap-3">
      {category === 'mcp' && (
        <div className="flex items-center justify-end gap-2">
          <Select
            onValueChange={(value) => onChangeScope(value as UsageScope)}
            value={scope}
          >
            <SelectTrigger
              aria-label={t('mcpScope')}
              className="w-36"
              size="sm"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="service">{t('mcpScopeService')}</SelectItem>
              <SelectItem value="gateway">{t('mcpScopeGateway')}</SelectItem>
              <SelectItem value="all">{t('mcpScopeAll')}</SelectItem>
            </SelectContent>
          </Select>
        </div>
      )}

      {overview && hasTraffic ? (
        <div className="grid min-w-0 grid-cols-1 gap-3">
          <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4 xl:grid-cols-6">
            {cards.map((card) => (
              <article
                className="grid gap-1 rounded-lg border bg-card px-3 py-2.5"
                data-usage-metric={card.label}
                key={card.label}
              >
                <div className="text-[0.68rem] font-bold tracking-wide text-muted-foreground uppercase">
                  {card.label}
                </div>
                <div className="text-base leading-none font-bold">
                  {metricValue(card, locale)}
                </div>
              </article>
            ))}
          </div>

          <section className="rounded-lg border bg-card p-4">
            <div className="mb-2 text-sm font-semibold">
              {t('overviewTrend')}
            </div>
            <UsageChart option={trendOption as EChartsOption} />
          </section>

          <section className="overflow-hidden rounded-lg border bg-card">
            <div className="flex flex-wrap items-center justify-between gap-2 border-b px-4 py-3 text-sm font-semibold">
              <span>{distributionTitle}</span>
              {category === 'ai' && (
                <div className="flex items-center gap-2">
                  <Select
                    disabled={perspectivePending}
                    onValueChange={(value) =>
                      onChangePerspective(value as UsagePerspective)
                    }
                    value={perspective}
                  >
                    <SelectTrigger
                      aria-busy={perspectivePending}
                      aria-label={t('overviewPerspective')}
                      className="w-32"
                      size="sm"
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="model">
                        {t('overviewPerspectiveModel')}
                      </SelectItem>
                      <SelectItem value="upstream">
                        {t('overviewPerspectiveUpstream')}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                  {perspectivePending && (
                    <LoaderCircle
                      aria-hidden="true"
                      className="size-4 animate-spin text-muted-foreground"
                    />
                  )}
                </div>
              )}
            </div>
            <div className="overflow-x-auto">
              <Table aria-busy={loading}>
                <TableHeader>
                  <TableRow>
                    <TableHead>
                      {category === 'ai' && perspective === 'upstream'
                        ? t('overviewUpstreamEndpoint')
                        : t('overviewObject')}
                    </TableHead>
                    <TableHead>{t('requests')}</TableHead>
                    <TableHead>{t('overviewRequestShare')}</TableHead>
                    {category === 'ai' ? (
                      <>
                        <TableHead>{t('overviewTotalTokens')}</TableHead>
                        <TableHead>{t('overviewTokenShare')}</TableHead>
                        <TableHead>{t('overviewCacheRate')}</TableHead>
                        <TableHead>{t('overviewErrorRate')}</TableHead>
                        <TableHead>{t('overviewAvgOutputRate')}</TableHead>
                      </>
                    ) : (
                      <TableHead>{t('overviewSuccessRate')}</TableHead>
                    )}
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {rows.map((row) => {
                    const drilldown = usageDrilldownForPerspective(
                      perspective,
                      category,
                      row,
                    )
                    return (
                      <TableRow
                        className={drilldown ? 'cursor-pointer' : undefined}
                        data-drilldown={drilldown ? 'true' : 'false'}
                        data-usage-breakdown-row={row.label}
                        key={`${row.label}:${row.endpoint_id ?? ''}:${row.model ?? ''}`}
                        onClick={
                          drilldown ? () => onDrilldown(drilldown) : undefined
                        }
                        onKeyDown={
                          drilldown
                            ? (event) => {
                                if (event.target !== event.currentTarget) return
                                if (event.key !== 'Enter' && event.key !== ' ')
                                  return
                                event.preventDefault()
                                onDrilldown(drilldown)
                              }
                            : undefined
                        }
                        tabIndex={drilldown ? 0 : undefined}
                      >
                        <TableCell className="max-w-64 break-words">
                          <span className="inline-flex items-center gap-1">
                            <span className="font-medium">{row.label}</span>
                            {category === 'ai' ? (
                              <BreakdownDetailPopover
                                perspective={perspective}
                                row={row}
                              />
                            ) : (
                              <Badge variant="secondary">
                                {providerBadge(row)}
                              </Badge>
                            )}
                          </span>
                        </TableCell>
                        <TableCell className="whitespace-nowrap">
                          {formatUsageCount(row.request_count, locale)}
                        </TableCell>
                        <TableCell className="whitespace-nowrap">
                          {formatUsagePercent(row.request_share)}
                        </TableCell>
                        {category === 'ai' ? (
                          <>
                            <TableCell className="whitespace-nowrap">
                              {formatUsageTokenQuantity(
                                row.tokens.total_tokens,
                                locale,
                              )}
                            </TableCell>
                            <TableCell className="whitespace-nowrap">
                              {formatUsagePercent(row.token_share)}
                            </TableCell>
                            <TableCell className="whitespace-nowrap">
                              {formatUsagePercent(row.tokens.cache_rate)}
                            </TableCell>
                            <TableCell className="whitespace-nowrap">
                              {formatUsagePercent(row.error_rate)}
                            </TableCell>
                            <TableCell className="whitespace-nowrap">
                              {formatUsageTokensPerSecond(
                                row.avg_output_tokens_per_second,
                              )}
                            </TableCell>
                          </>
                        ) : (
                          <TableCell className="whitespace-nowrap">
                            {formatUsagePercent(row.success_rate)}
                          </TableCell>
                        )}
                      </TableRow>
                    )
                  })}
                  {rows.length === 0 && (
                    <TableRow>
                      <TableCell
                        className="py-8 text-center text-muted-foreground"
                        colSpan={category === 'ai' ? 8 : 4}
                      >
                        -
                      </TableCell>
                    </TableRow>
                  )}
                </TableBody>
              </Table>
            </div>
          </section>
        </div>
      ) : (
        <div className="rounded-lg border bg-card px-4 py-8 text-sm text-muted-foreground">
          {loading
            ? t('loading')
            : category === 'ai'
              ? t('noAiRequestRecords')
              : t('noMcpCallRecords')}
        </div>
      )}
    </div>
  )
}
