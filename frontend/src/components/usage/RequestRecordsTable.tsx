import type { ReactNode } from 'react'
import {
  ArrowDown,
  ArrowUp,
  ChartNoAxesColumn,
  ChevronsUpDown,
} from 'lucide-react'
import type { RequestRecordListRow } from '@/generated/admin-api'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { TablePagination } from '@/components/shared/TablePagination'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
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
  type UsageRecordView,
} from '@/lib/usage-records'
import {
  USAGE_PAGE_SIZE_OPTIONS,
  USAGE_SORT_FIELDS,
  usageAriaSort,
  type UsageCategory,
  type UsageSortOrder,
} from '@/lib/usage-model'

type UsageColumn = {
  id: string
  header: string
  /** The backend sort field; absent means the column is not sortable. */
  sortField?: string
  render: (record: UsageRecordView) => ReactNode
  /** Local sizing only; content, order and sort stay untouched. */
  widthClassName?: string
}

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

  const identityColumns: UsageColumn[] = [
    {
      id: 'details',
      header: '',
      render: (record) => (
        <Button
          aria-label={t('viewDetails')}
          onClick={() => onOpenDetail(record)}
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
      header: t('date'),
      render: dateCell,
      sortField: USAGE_SORT_FIELDS.request_date,
    },
    {
      id: 'created_at',
      header: t('time'),
      render: timeCell,
      sortField: USAGE_SORT_FIELDS.created_at,
    },
    ...(isAdmin
      ? [
          {
            id: 'user',
            header: t('user'),
            render: (record: UsageRecordView) => record.user_key,
            sortField: USAGE_SORT_FIELDS.user,
          },
        ]
      : []),
    {
      id: 'client_key',
      header: t('clientKey'),
      render: (record) => record.client_key_label || '-',
      sortField: USAGE_SORT_FIELDS.client_key,
    },
  ]

  const tailColumns: UsageColumn[] = [
    {
      id: 'request_state',
      header: t('status'),
      render: (record) => <RequestRecordsStateChips record={record} />,
      sortField: USAGE_SORT_FIELDS.request_state,
    },
    {
      id: 'duration_ms',
      header: t('totalLatency'),
      render: (record) => formatUsageMs(record.duration_ms),
      sortField: USAGE_SORT_FIELDS.duration_ms,
    },
    {
      id: 'error',
      header: t('error'),
      render: (record) => (
        <ErrorCell onOpenDetail={onOpenDetail} record={record} />
      ),
    },
  ]

  const aiColumns: UsageColumn[] = [
    ...identityColumns,
    {
      id: 'model',
      header: t('model'),
      render: (record) => (
        <span className="font-semibold">{record.model_display}</span>
      ),
      sortField: USAGE_SORT_FIELDS.model,
      widthClassName: 'min-w-[14rem]',
    },
    {
      id: 'target',
      header: t('upstream'),
      render: (record) => <UpstreamCell record={record} />,
      sortField: USAGE_SORT_FIELDS.target,
      widthClassName: 'min-w-[12rem]',
    },
    ...tailColumns.slice(0, 1),
    {
      id: 'redaction',
      header: t('redaction'),
      render: (record) =>
        record.redaction.applied ? (
          <span className="inline-flex items-center gap-1">
            <Badge>{t('redactionHit')}</Badge>
            <Badge variant="secondary">
              {String(record.redaction.replacements_count)}
            </Badge>
          </span>
        ) : (
          '-'
        ),
    },
    ...tailColumns.slice(1, 2),
    {
      id: 'total_tokens',
      header: t('tokenCacheSummary'),
      render: (record) =>
        `${formatUsageTokenQuantity(record.total_tokens, locale)} / ${formatUsageTokenQuantity(record.cached_tokens, locale)} / ${formatUsagePercent(record.cache_rate)}`,
      sortField: USAGE_SORT_FIELDS.total_tokens,
    },
    {
      id: 'throughput',
      header: `${t('e2eOutputRate')} token/s`,
      render: (record) => (
        <span title={t('e2eOutputRate')}>{formatUsageOutputRate(record)}</span>
      ),
    },
    ...tailColumns.slice(2),
  ]

  const mcpColumns: UsageColumn[] = [
    ...identityColumns,
    {
      id: 'target',
      header: t('mcpServer'),
      render: (record) => (
        <span className="font-semibold">
          {record.mcp_server_name || record.upstream_label}
        </span>
      ),
      sortField: USAGE_SORT_FIELDS.target,
    },
    {
      id: 'provider_kind',
      header: t('providerKind'),
      render: (record) =>
        record.server_provider_kind ? (
          <Badge variant="secondary">{record.server_provider_kind}</Badge>
        ) : (
          '-'
        ),
    },
    {
      id: 'mcp_method',
      header: t('mcpMethod'),
      render: (record) => (
        <span className="font-mono">{record.mcp_protocol_method || '-'}</span>
      ),
      sortField: USAGE_SORT_FIELDS.mcp_method,
    },
    {
      id: 'mcp_operation',
      header: t('mcpOperation'),
      render: (record) => (
        <span className="font-mono">{record.mcp_operation_name || '-'}</span>
      ),
      sortField: USAGE_SORT_FIELDS.mcp_operation,
    },
    ...tailColumns,
  ]

  const columns = category === 'ai' ? aiColumns : mcpColumns
  const emptyLabel =
    category === 'ai' ? t('noRequestRecords') : t('noMcpCallRecords')

  return (
    <div className="grid min-w-0 grid-cols-1 gap-3">
      <Table
        aria-busy={loading}
        className={category === 'ai' ? 'min-w-[76rem]' : 'min-w-[72rem]'}
      >
        <TableHeader>
          <TableRow>
            {columns.map((column) => {
              const ariaSort = column.sortField
                ? usageAriaSort(state, column.sortField)
                : undefined
              const active = ariaSort !== undefined && ariaSort !== 'none'
              return (
                <TableHead
                  aria-sort={ariaSort}
                  className={`whitespace-nowrap ${column.widthClassName ?? ''}`}
                  data-usage-column={column.id}
                  key={column.id}
                >
                  {column.sortField ? (
                    <button
                      className="inline-flex items-center gap-1 whitespace-nowrap text-left"
                      onClick={() => onSort(column.sortField as string)}
                      type="button"
                    >
                      <span>{column.header}</span>
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
                  ) : (
                    column.header
                  )}
                </TableHead>
              )
            })}
          </TableRow>
        </TableHeader>
        <TableBody>
          {records.map((row) => {
            const record = createUsageRecordView(row)
            return (
              <TableRow
                data-usage-record-row={row.record_id}
                key={row.record_id}
              >
                {columns.map((column) => (
                  <TableCell
                    className={`${column.id === 'request_date' || column.id === 'created_at' ? 'whitespace-nowrap' : 'whitespace-normal'} ${column.widthClassName ?? ''}`}
                    data-usage-cell={column.id}
                    key={column.id}
                  >
                    {column.render(record)}
                  </TableCell>
                ))}
              </TableRow>
            )
          })}
          {records.length === 0 && (
            <TableRow>
              <TableCell
                className="py-8 text-center text-muted-foreground"
                colSpan={columns.length}
              >
                {emptyLabel}
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
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

function UpstreamCell({ record }: { record: UsageRecordView }) {
  const { t } = useLocale()
  return (
    <span className="inline-flex items-center gap-1 whitespace-nowrap">
      <span className="font-semibold">{record.upstream_label}</span>
      <Badge variant="outline">
        {t('session')} {record.session_short_id}
      </Badge>
      {record.conversation_seq ? (
        <Badge variant="outline">#{record.conversation_seq}</Badge>
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
