import { useEffect, useState } from 'react'
import { Info } from 'lucide-react'
import type { RequestRecordOverviewBreakdownRow } from '@/generated/admin-api'
import { Button } from '@/components/ui/button'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { useLocale } from '@/lib/i18n'
import {
  formatUsageCount,
  formatUsagePercent,
  formatUsageTokenQuantity,
  formatUsageTokensPerSecond,
} from '@/lib/usage-format'
import { usageDetailGroup, type UsageDetailEntry } from '@/lib/usage-overview'
import {
  partitionZeroTokenEntries,
  type UsagePerspective,
} from '@/lib/usage-model'

const METRIC_WIDTHS = {
  avg_output_tokens_per_second: 112,
  cache_rate: 80,
  error_rate: 80,
  request_count: 88,
  request_share: 80,
  token_share: 80,
  total_tokens: 96,
} as const

const IDENTIFIER_MIN_WIDTH = 96
const IDENTIFIER_MAX_WIDTH = 220
const IDENTIFIER_CHAR_WIDTH = 6.5
const IDENTIFIER_CELL_PADDING = 16
const NARROW_VIEWPORT = '(max-width: 640px)'
const POPOVER_BORDER_WIDTH = 2
const VIEWPORT_GUTTER = 32

function textWidth(value: string): number {
  let width = IDENTIFIER_CELL_PADDING
  for (const character of value) {
    width +=
      (character.codePointAt(0) ?? 0) > 0x2e80
        ? IDENTIFIER_CHAR_WIDTH * 2
        : IDENTIFIER_CHAR_WIDTH
  }
  return Math.round(width)
}

function boundedIdentifierWidth(values: string[]): number {
  const width = values.reduce(
    (widest, value) => Math.max(widest, textWidth(value)),
    0,
  )
  return Math.min(Math.max(width, IDENTIFIER_MIN_WIDTH), IDENTIFIER_MAX_WIDTH)
}

function useNarrowViewport(): boolean {
  const [narrow, setNarrow] = useState(false)
  useEffect(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return
    const query = window.matchMedia(NARROW_VIEWPORT)
    setNarrow(query.matches)
    const listener = (event: MediaQueryListEvent) => setNarrow(event.matches)
    query.addEventListener('change', listener)
    return () => query.removeEventListener('change', listener)
  }, [])
  return narrow
}

/** The current viewport width, so the popover can size itself to its content. */
function useViewportWidth(): number {
  const [width, setWidth] = useState(() =>
    typeof window === 'undefined' ? 0 : window.innerWidth,
  )
  useEffect(() => {
    if (typeof window === 'undefined') return
    const update = () => setWidth(window.innerWidth)
    update()
    window.addEventListener('resize', update)
    return () => window.removeEventListener('resize', update)
  }, [])
  return width
}

type Column = {
  key: string
  header: string
  width: number
  pinned: boolean
  render: (entry: UsageDetailEntry) => string
  full?: (entry: UsageDetailEntry) => string
}

type BreakdownDetailPopoverProps = {
  row: RequestRecordOverviewBreakdownRow
  perspective: UsagePerspective
}

/**
 * The per-upstream or per-model detail behind one distribution row.
 *
 * The fold is presentation only: expanding reveals rows that were already in
 * the response and never re-queries, re-totals or changes a filter. Only an
 * exact `0` total folds; an unknown total stays visible and renders `-`.
 */
export function BreakdownDetailPopover({
  row,
  perspective,
}: BreakdownDetailPopoverProps) {
  const { locale, t } = useLocale()
  const narrow = useNarrowViewport()
  const viewportWidth = useViewportWidth()
  const group = usageDetailGroup(row, perspective)
  const rowKey = [
    perspective,
    row.label,
    row.endpoint_id ?? '',
    row.model ?? '',
    row.mcp_server_id ?? '',
  ].join('\u0000')
  const [reveal, setReveal] = useState(false)

  useEffect(() => {
    setReveal(false)
  }, [rowKey])

  const { visible, folded, hiddenRequestCount } = partitionZeroTokenEntries(
    group.entries,
  )
  const shown = reveal ? [...visible, ...folded] : visible

  if (!group.show) return null

  const isUpstream = perspective === 'upstream'
  const title = isUpstream
    ? t('overviewModelBreakdown')
    : t('overviewUpstreamBreakdown')

  const endpointWidth = boundedIdentifierWidth([
    t('overviewUpstreamEndpoint'),
    ...group.entries.map((entry) => entry.endpoint),
  ])
  const modelWidth = boundedIdentifierWidth([
    t('model'),
    ...group.entries.map((entry) => entry.model),
  ])

  const identifierColumns: Column[] = isUpstream
    ? [
        {
          key: 'model',
          header: t('model'),
          width: modelWidth,
          pinned: true,
          render: (entry) => entry.model,
        },
      ]
    : [
        {
          key: 'endpoint',
          header: t('overviewUpstreamEndpoint'),
          width: endpointWidth,
          pinned: true,
          render: (entry) => entry.endpoint,
        },
        {
          key: 'model',
          header: t('model'),
          width: modelWidth,
          pinned: true,
          render: (entry) => entry.model,
        },
      ]

  // A narrow viewport keeps one identifier pinned so the metrics stay legible.
  const columns: Column[] = narrow
    ? identifierColumns.map((column, index) => ({
        ...column,
        pinned: index === identifierColumns.length - 1,
      }))
    : identifierColumns

  const metricColumns: Column[] = [
    {
      key: 'request_count',
      header: t('requests'),
      width: METRIC_WIDTHS.request_count,
      pinned: false,
      render: (entry) => formatUsageCount(entry.request_count, locale),
    },
    {
      key: 'request_share',
      header: t('overviewRequestShare'),
      width: METRIC_WIDTHS.request_share,
      pinned: false,
      render: (entry) => formatUsagePercent(entry.request_share),
    },
    {
      key: 'total_tokens',
      header: t('overviewTotalTokens'),
      width: METRIC_WIDTHS.total_tokens,
      pinned: false,
      render: (entry) => formatUsageTokenQuantity(entry.total_tokens, locale),
    },
    {
      key: 'token_share',
      header: t('overviewTokenShare'),
      width: METRIC_WIDTHS.token_share,
      pinned: false,
      render: (entry) => formatUsagePercent(entry.token_share),
    },
    {
      key: 'cache_rate',
      header: t('overviewCacheRate'),
      width: METRIC_WIDTHS.cache_rate,
      pinned: false,
      render: (entry) => formatUsagePercent(entry.cache_rate),
    },
    {
      key: 'error_rate',
      header: t('overviewErrorRate'),
      width: METRIC_WIDTHS.error_rate,
      pinned: false,
      render: (entry) => formatUsagePercent(entry.error_rate),
    },
    {
      key: 'avg_output_tokens_per_second',
      header: t('overviewAvgOutputRate'),
      width: METRIC_WIDTHS.avg_output_tokens_per_second,
      pinned: false,
      render: (entry) =>
        formatUsageTokensPerSecond(entry.avg_output_tokens_per_second),
    },
  ]

  const allColumns = [...columns, ...metricColumns]
  const pinnedLeft = new Map<string, number>()
  let offset = 0
  for (const column of allColumns) {
    if (!column.pinned) continue
    pinnedLeft.set(column.key, offset)
    offset += column.width
  }
  const totalWidth = allColumns.reduce((sum, column) => sum + column.width, 0)
  // The table keeps its configured minimum widths; the popover grows to that
  // intrinsic width instead of the fixed narrow default and only stops at the
  // viewport, where the table scrolls inside its own container.
  const popoverWidth =
    viewportWidth > 0
      ? Math.min(
          totalWidth + POPOVER_BORDER_WIDTH,
          viewportWidth - VIEWPORT_GUTTER,
        )
      : totalWidth

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button
          aria-label={title}
          className="text-muted-foreground"
          onClick={(event) => event.stopPropagation()}
          size="icon-xs"
          type="button"
          variant="ghost"
        >
          <Info aria-hidden="true" />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        className="max-h-[50vh] w-auto max-w-[calc(100vw-2rem)] overflow-auto p-0"
        data-usage-popover-intrinsic-width={totalWidth}
        style={{ width: popoverWidth }}
      >
        <div className="border-b px-3 py-2 text-xs font-semibold">{title}</div>
        {group.entries.length === 0 ? (
          <div className="px-3 py-6 text-xs text-muted-foreground">
            {t('overviewUpstreamEmpty')}
          </div>
        ) : (
          <>
            <div className="overflow-x-auto">
              <Table style={{ width: totalWidth, tableLayout: 'fixed' }}>
                <TableHeader>
                  <TableRow>
                    {allColumns.map((column) => (
                      <TableHead
                        className={
                          column.pinned
                            ? 'sticky z-10 bg-popover whitespace-normal'
                            : 'whitespace-normal'
                        }
                        key={column.key}
                        style={{
                          left: pinnedLeft.get(column.key),
                          maxWidth: column.width,
                          minWidth: column.width,
                          width: column.width,
                        }}
                      >
                        {column.header}
                      </TableHead>
                    ))}
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {shown.map((entry) => (
                    <TableRow key={entry.key}>
                      {allColumns.map((column) => {
                        const value = column.render(entry)
                        return (
                          <TableCell
                            className={
                              column.pinned
                                ? 'sticky z-10 bg-popover'
                                : undefined
                            }
                            key={column.key}
                            style={{
                              left: pinnedLeft.get(column.key),
                              maxWidth: column.width,
                              minWidth: column.width,
                              width: column.width,
                            }}
                            title={value}
                          >
                            <span className="block truncate">{value}</span>
                          </TableCell>
                        )
                      })}
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
            {folded.length > 0 && (
              <div
                className="flex flex-wrap items-center justify-between gap-2 border-t px-3 py-2 text-xs"
                data-usage-zero-token-footer={reveal ? 'expanded' : 'collapsed'}
              >
                <span className="text-muted-foreground">
                  {reveal
                    ? t('overviewZeroTokenCount', {
                        count: hiddenRequestCount,
                      })
                    : t('overviewZeroTokenHidden', {
                        count: hiddenRequestCount,
                      })}
                </span>
                <Button
                  aria-expanded={reveal}
                  onClick={(event) => {
                    event.stopPropagation()
                    setReveal((current) => !current)
                  }}
                  size="xs"
                  type="button"
                  variant="ghost"
                >
                  {reveal
                    ? t('overviewZeroTokenHide')
                    : t('overviewZeroTokenReveal')}
                </Button>
              </div>
            )}
          </>
        )}
      </PopoverContent>
    </Popover>
  )
}
