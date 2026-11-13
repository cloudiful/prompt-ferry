import { useMemo, type ReactNode } from 'react'
import {
  ArrowDown,
  ArrowUp,
  ChartNoAxesColumn,
  ChevronsUpDown,
} from 'lucide-react'
import type { RequestRecordListRow } from '@/generated/admin-api'
import {
  DataTable,
  type DataTableColumnDef,
} from '@/components/shared/DataTable'
import { TablePagination } from '@/components/shared/TablePagination'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { RequestRecordsStateChips } from '@/components/usage/RequestRecordsStateChips'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { useLocale } from '@/lib/i18n'
import {
  formatUsageDate,
  formatUsageInstant,
  formatUsageMs,
  formatUsagePercent,
  formatUsageTokenQuantity,
} from '@/lib/usage-format'
import {
  createUsageRecordView,
  formatUsageOutputRate,
  usageOutputRate,
  type UsageRecordView,
} from '@/lib/usage-records'
import { cn } from '@/lib/utils'
import {
  USAGE_PAGE_SIZE_OPTIONS,
  USAGE_SORT_FIELDS,
  usageAriaSort,
  type UsageCategory,
  type UsageSortOrder,
} from '@/lib/usage-model'

type RequestRecordsTableProps = {
  category: UsageCategory
  first: number
  isAdmin: boolean
  loading: boolean
  onOpenDetail: (record: RequestRecordListRow) => void
  onPage: (first: number, rows: number) => void
  onSort: (field: string) => void
  records: RequestRecordListRow[]
  rows: number
  sortField: string
  sortOrder: UsageSortOrder
  total: number
}

const COLUMN_WIDTHS: Record<string, string> = {
  model: 'min-w-[14rem]',
  target: 'min-w-[12rem]',
}

const NOWRAP_COLUMNS = new Set(['request_date', 'created_at'])

const COLUMN_SORT_FIELDS: Record<string, string | undefined> = {
  created_at: USAGE_SORT_FIELDS.created_at,
  client_key: USAGE_SORT_FIELDS.client_key,
  duration_ms: USAGE_SORT_FIELDS.duration_ms,
  mcp_operation: USAGE_SORT_FIELDS.mcp_operation,
  model: USAGE_SORT_FIELDS.model,
  request_date: USAGE_SORT_FIELDS.request_date,
  request_state: USAGE_SORT_FIELDS.request_state,
  target: USAGE_SORT_FIELDS.target,
  total_tokens: USAGE_SORT_FIELDS.total_tokens,
  user: USAGE_SORT_FIELDS.user,
}

function SortHeader({
  field,
  label,
  onSort,
  sortField,
  sortOrder,
}: {
  field: string
  label: string
  onSort: (field: string) => void
  sortField: string
  sortOrder: UsageSortOrder
}) {
  const ariaSort = usageAriaSort({ field: sortField, order: sortOrder }, field)
  const active = ariaSort !== undefined && ariaSort !== 'none'
  return (
    <button
      className="inline-flex items-center gap-1 whitespace-nowrap text-left"
      onClick={() => onSort(field)}
      type="button"
    >
      <span>{label}</span>
      {active ? (
        ariaSort === 'ascending' ? (
          <ArrowUp aria-hidden="true" className="size-3" />
        ) : (
          <ArrowDown aria-hidden="true" className="size-3" />
        )
      ) : (
        <ChevronsUpDown
          aria-hidden="true"
          className="size-3 text-muted-foreground"
        />
      )}
    </button>
  )
}

/**
 * The AI/MCP records table.
 *
 * It renders the rows the parent hands over and reports every intent through a
 * callback: sorting, paging and opening a detail. Nothing is re-sorted,
 * re-paged or cached locally, so a late server response is always the visible
 * order. Date and time are formatted at render time from the current display
 * time zone, while the Date sort keeps the accepted UTC-day contract.
 */
export function RequestRecordsTable({
  category,
  first,
  isAdmin,
  loading,
  onOpenDetail,
  onPage,
  onSort,
  records,
  rows,
  sortField,
  sortOrder,
  total,
}: RequestRecordsTableProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const views = useMemo(() => records.map(createUsageRecordView), [records])
  const state = { field: sortField, order: sortOrder }

  const dateCell = (record: UsageRecordView) =>
    formatUsageDate(record.created_at, locale, timeZone.effective)
  const timeCell = (record: UsageRecordView) =>
    formatUsageInstant(record.created_at, locale, timeZone.effective, {
      hour: '2-digit',
      hour12: false,
      minute: '2-digit',
      second: '2-digit',
    })

  const columns = useMemo<Array<DataTableColumnDef<UsageRecordView>>>(() => {
    const sortable = (id: string, label: string): ReactNode => {
      const field = COLUMN_SORT_FIELDS[id]
      if (!field) return label
      return (
        <SortHeader
          field={field}
          label={label}
          onSort={onSort}
          sortField={sortField}
          sortOrder={sortOrder}
        />
      )
    }

    const identityColumns: Array<DataTableColumnDef<UsageRecordView>> = [
      {
        id: 'details',
        header: '',
        cell: ({ row }) => (
          <Button
            aria-label={t('viewDetails')}
            onClick={() => onOpenDetail(row.original)}
            size="icon-xs"
            type="button"
            variant="ghost"
          >
            <ChartNoAxesColumn aria-hidden="true" />
          </Button>
        ),
      },
      {
        id: 'request_date',
        header: () => sortable('request_date', t('date')),
        cell: ({ row }) => dateCell(row.original),
      },
      {
        id: 'created_at',
        header: () => sortable('created_at', t('time')),
        cell: ({ row }) => timeCell(row.original),
      },
      ...(isAdmin
        ? [
            {
              id: 'user',
              header: () => sortable('user', t('user')),
              cell: ({ row }: { row: { original: UsageRecordView } }) =>
                row.original.user_key,
            } satisfies DataTableColumnDef<UsageRecordView>,
          ]
        : []),
      {
        id: 'client_key',
        header: () => sortable('client_key', t('clientKey')),
        cell: ({ row }) => row.original.client_key_label || '-',
      },
    ]

    const tailColumns: Array<DataTableColumnDef<UsageRecordView>> = [
      {
        id: 'request_state',
        header: () => sortable('request_state', t('status')),
        cell: ({ row }) => <RequestRecordsStateChips record={row.original} />,
      },
      {
        id: 'duration_ms',
        header: () => sortable('duration_ms', t('totalLatency')),
        cell: ({ row }) => formatUsageMs(row.original.duration_ms),
      },
      {
        id: 'error',
        header: t('error'),
        cell: ({ row }) => (
          <ErrorCell onOpenDetail={onOpenDetail} record={row.original} />
        ),
      },
    ]

    const aiColumns: Array<DataTableColumnDef<UsageRecordView>> = [
      ...identityColumns,
      {
        id: 'model',
        header: () => sortable('model', t('model')),
        cell: ({ row }) => <ModelCell record={row.original} />,
      },
      {
        id: 'target',
        header: () => sortable('target', t('upstream')),
        cell: ({ row }) => <UpstreamCell record={row.original} />,
      },
      ...tailColumns.slice(0, 1),
      {
        id: 'redaction',
        header: t('redaction'),
        cell: ({ row }) =>
          row.original.redaction.applied ? (
            <span className="inline-flex items-center gap-1">
              <Badge>{t('redactionHit')}</Badge>
              <Badge variant="secondary">
                {String(row.original.redaction.replacements_count)}
              </Badge>
            </span>
          ) : (
            '-'
          ),
      },
      ...tailColumns.slice(1, 2),
      {
        id: 'total_tokens',
        header: () => sortable('total_tokens', t('tokenCacheSummary')),
        cell: ({ row }) => (
          <span className="whitespace-nowrap">
            {`${formatUsageTokenQuantity(row.original.total_tokens, locale)} · ${formatUsagePercent(row.original.cache_rate)}`}
          </span>
        ),
      },
      {
        id: 'throughput',
        header: `${t('e2eOutputRate')} token/s`,
        cell: ({ row }) => <RateCell record={row.original} />,
      },
      ...tailColumns.slice(2),
    ]

    const mcpColumns: Array<DataTableColumnDef<UsageRecordView>> = [
      ...identityColumns,
      {
        id: 'target',
        header: () => sortable('target', t('mcpServer')),
        cell: ({ row }) => (
          <span className="font-semibold">
            {row.original.mcp_server_name || row.original.upstream_label}
          </span>
        ),
      },
      {
        id: 'mcp_operation',
        header: () => sortable('mcp_operation', t('mcpOperation')),
        cell: ({ row }) => (
          <span className="font-mono">
            {row.original.mcp_operation_name || '-'}
          </span>
        ),
      },
      ...tailColumns,
    ]

    return category === 'ai' ? aiColumns : mcpColumns
  }, [
    category,
    isAdmin,
    locale,
    onOpenDetail,
    onSort,
    sortField,
    sortOrder,
    t,
    timeZone.effective,
  ])

  const emptyLabel =
    category === 'ai' ? t('noRequestRecords') : t('noMcpCallRecords')

  return (
    <div className="grid min-w-0 grid-cols-1 gap-3">
      <DataTable
        busy={loading}
        cellAttributes={({ column }) => ({
          className: `${NOWRAP_COLUMNS.has(column.id) ? 'whitespace-nowrap' : 'whitespace-normal'} ${COLUMN_WIDTHS[column.id] ?? ''}`,
          'data-usage-cell': column.id,
        })}
        className={category === 'ai' ? 'min-w-[76rem]' : 'min-w-[72rem]'}
        columns={columns}
        data={views}
        emptyLabel={emptyLabel}
        getRowId={(record) => String(record.record_id)}
        headerAttributes={({ id }) => {
          const field = COLUMN_SORT_FIELDS[id]
          return {
            className: `whitespace-nowrap ${COLUMN_WIDTHS[id] ?? ''}`,
            'data-usage-column': id,
            ...(field ? { 'aria-sort': usageAriaSort(state, field) } : {}),
          }
        }}
        label={t(category === 'ai' ? 'aiRequests' : 'mcpCalls')}
        rowAttributes={(record) => ({
          'data-usage-record-row': record.record_id,
        })}
      />
      <TablePagination
        first={first}
        onChange={(change) => onPage(change.first, change.rows)}
        pageSizeOptions={USAGE_PAGE_SIZE_OPTIONS}
        rows={rows}
        total={total}
      />
    </div>
  )
}

const RATE_TONES = {
  slow: 'border-red-200 bg-red-100 text-red-800 dark:border-red-800 dark:bg-red-950/60 dark:text-red-200',
  medium: 'border-border bg-muted text-muted-foreground',
  fast: 'border-emerald-200 bg-emerald-100 text-emerald-800 dark:border-emerald-800 dark:bg-emerald-950/60 dark:text-emerald-200',
} as const

function ModelCell({ record }: { record: UsageRecordView }) {
  const requested = record.requested_model || record.model || '-'
  const upstream = (record.upstream_model || '').trim()
  const showsUpstream = upstream !== '' && upstream !== requested
  return (
    <span className="font-semibold" title={record.model_display}>
      {requested}
      {showsUpstream ? (
        <span className="font-normal text-muted-foreground">
          {` (${upstream})`}
        </span>
      ) : null}
    </span>
  )
}

function RateCell({ record }: { record: UsageRecordView }) {
  const { t } = useLocale()
  const rate = usageOutputRate(record)
  if (rate === null) return <span>-</span>
  const text = formatUsageOutputRate(record)
  const band = rate < 20 ? 'slow' : rate < 50 ? 'medium' : 'fast'
  return (
    <span
      className={cn(
        'inline-flex items-center rounded-full border px-2 py-0.5 text-xs font-medium tabular-nums',
        RATE_TONES[band],
      )}
      data-usage-rate-band={band}
      title={`${t('e2eOutputRate')} ${text} token/s`}
    >
      {text}
    </span>
  )
}

function UpstreamCell({ record }: { record: UsageRecordView }) {
  const { t } = useLocale()
  return (
    <span className="inline-flex items-center gap-1 whitespace-nowrap">
      <span className="font-semibold">{record.upstream_label}</span>
      <Badge
        className="border-border/70 font-normal text-muted-foreground"
        variant="outline"
      >
        {t('session')} {record.session_short_id}
      </Badge>
      {record.conversation_seq ? (
        <Badge
          className="border-border/70 font-normal text-muted-foreground"
          variant="outline"
        >
          #{record.conversation_seq}
        </Badge>
      ) : null}
      {record.is_first_turn ? (
        <Badge variant="outline">{t('firstTurn')}</Badge>
      ) : null}
    </span>
  )
}

function ErrorCell({
  onOpenDetail,
  record,
}: {
  onOpenDetail: (record: RequestRecordListRow) => void
  record: UsageRecordView
}) {
  const { t } = useLocale()
  if (!record.error_message) return <span>-</span>
  const label = record.error_code
    ? `${record.error_code}: ${record.error_message}`
    : record.error_message
  const neutral =
    record.request_category === 'ai' && record.request_state === 'aborted'
  return (
    <Button
      aria-label={label}
      className={
        neutral
          ? 'max-w-56 justify-start truncate text-muted-foreground'
          : 'max-w-56 justify-start truncate text-destructive'
      }
      onClick={() => onOpenDetail(record)}
      size="sm"
      type="button"
      variant="link"
    >
      {label}
    </Button>
  )
}
