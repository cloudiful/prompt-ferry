import { Clock, RefreshCw, TriangleAlert } from 'lucide-react'
import type { ConfigAuditEntry } from '@/generated/admin-api'
import { Button } from '@/components/ui/button'
import {
  DataTable,
  type DataTableColumnDef,
} from '@/components/shared/DataTable'
import { SettingsCard } from '@/components/settings/SettingsCard'
import { TablePagination } from '@/components/shared/TablePagination'
import { useLocale } from '@/lib/i18n'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import {
  CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_OPTIONS,
  configArchiveAuditRow,
  formatAuditCount,
  type ConfigArchiveAuditRow,
} from '@/lib/config-archive-model'
import type { TablePageChange } from '@/models/pagination'

type ConfigArchiveAuditTableProps = {
  entries: ConfigAuditEntry[]
  error: string
  failed: boolean
  first: number
  loading: boolean
  onPage: (change: TablePageChange) => void
  onRefresh: () => void
  rows: number
  total: number
  unavailable: boolean
  unavailableMessage: string
}

/**
 * The archive audit trail: newest first, one page at a time.
 *
 * Every column is metadata the server already returned — action, actor, backend,
 * byte count, fingerprint, summed domain records, result, error code and time.
 * A nullable column renders as `-` while a real zero stays `0`, the fingerprint
 * shows a short form with the full value in its title, and the timestamp uses
 * the active locale and display time zone.
 */
export function ConfigArchiveAuditTable({
  entries,
  error,
  failed,
  first,
  loading,
  onPage,
  onRefresh,
  rows,
  total,
  unavailable,
  unavailableMessage,
}: ConfigArchiveAuditTableProps) {
  const { t, locale } = useLocale()
  const { effective: timeZone } = useDisplayTimeZone()
  const projected = entries.map((entry) =>
    configArchiveAuditRow(entry, locale, timeZone),
  )

  const columns: Array<DataTableColumnDef<ConfigArchiveAuditRow>> = [
    {
      id: 'action',
      header: t('configArchiveAuditAction'),
      cell: ({ row }) => (
        <span
          className={
            row.original.action === 'export'
              ? 'rounded border border-primary/40 px-1.5 py-0.5 text-[10px] text-primary'
              : 'rounded border border-warning/40 px-1.5 py-0.5 text-[10px] text-warning'
          }
        >
          {t(
            row.original.action === 'export'
              ? 'configArchiveAuditActionExport'
              : 'configArchiveAuditActionImport',
          )}
        </span>
      ),
    },
    {
      id: 'actor',
      header: t('configArchiveAuditActor'),
      cell: ({ row }) => row.original.actor,
    },
    {
      id: 'backend',
      header: t('configArchiveExportBackend'),
      cell: ({ row }) => row.original.backendKind,
    },
    {
      id: 'bytes',
      header: t('configArchiveAuditBytes'),
      cell: ({ row }) => (
        <span className="tabular-nums">
          {formatAuditCount(row.original.archiveBytes, locale)}
        </span>
      ),
    },
    {
      id: 'fingerprint',
      header: t('configArchiveExportFingerprint'),
      cell: ({ row }) => (
        <span className="font-mono" title={row.original.fullFingerprint}>
          {row.original.fingerprint}
        </span>
      ),
    },
    {
      id: 'records',
      header: t('configArchiveAuditRecords'),
      cell: ({ row }) => (
        <span className="tabular-nums">
          {formatAuditCount(row.original.records, locale)}
        </span>
      ),
    },
    {
      id: 'result',
      header: t('configArchiveAuditResult'),
      cell: ({ row }) => (
        <span
          data-audit-result={row.original.success ? 'success' : 'failed'}
          className={
            row.original.success
              ? 'text-[11px] text-primary'
              : 'text-[11px] text-destructive'
          }
        >
          {t(row.original.success ? 'success' : 'failed')}
        </span>
      ),
    },
    {
      id: 'error',
      header: t('error'),
      cell: ({ row }) => (
        <div className="grid gap-px">
          <span>{row.original.errorCode}</span>
          {row.original.errorMessage ? (
            <span
              className="block max-w-[16rem] truncate text-[11px] text-muted-foreground"
              title={row.original.errorMessage}
            >
              {row.original.errorMessage}
            </span>
          ) : null}
        </div>
      ),
    },
    {
      id: 'createdAt',
      header: t('configArchiveAuditCreatedAt'),
      cell: ({ row }) => (
        <span className="whitespace-nowrap">{row.original.createdAt}</span>
      ),
    },
  ]

  return (
    <SettingsCard icon={Clock} title={t('configArchiveAuditTitle')}>
      <div className="flex min-w-0 flex-wrap items-center justify-end gap-2">
        {loading ? (
          <span className="text-[11px] text-muted-foreground" role="status">
            {t('loading')}
          </span>
        ) : null}
        <Button
          disabled={loading || unavailable}
          id="settings-config-archive-audit-refresh"
          onClick={onRefresh}
          size="xs"
          type="button"
          variant="outline"
        >
          <RefreshCw aria-hidden="true" />
          {t('refresh')}
        </Button>
      </div>

      {unavailable ? (
        <p className="m-0 flex items-start gap-1.5 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-muted-foreground">
          <TriangleAlert
            aria-hidden="true"
            className="mt-0.5 size-3.5 shrink-0 text-warning"
          />
          <span className="min-w-0 break-words">
            {unavailableMessage
              ? `${t('error')}: ${unavailableMessage}`
              : t('error')}
          </span>
          <Button
            id="settings-config-archive-audit-unavailable-retry"
            onClick={onRefresh}
            size="xs"
            type="button"
            variant="outline"
          >
            <RefreshCw aria-hidden="true" />
            {t('retry')}
          </Button>
        </p>
      ) : null}

      {!unavailable && failed ? (
        <div
          className="flex min-w-0 flex-wrap items-center gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2"
          role="alert"
        >
          <span className="min-w-0 break-words text-xs text-muted-foreground">
            {error || t('error')}
          </span>
          <Button
            id="settings-config-archive-audit-retry"
            onClick={onRefresh}
            size="xs"
            type="button"
            variant="outline"
          >
            <RefreshCw aria-hidden="true" />
            {t('refresh')}
          </Button>
        </div>
      ) : null}

      <DataTable
        busy={loading}
        className="min-w-0"
        columns={columns}
        data={projected}
        emptyLabel={t('configArchiveAuditEmpty')}
        getRowId={(row) => String(row.auditId)}
        label={t('configArchiveAuditTitle')}
        loading={loading}
        rowAttributes={(row) => ({ 'data-audit-id': row.auditId })}
        tableAttributes={{ 'data-config-archive-audit': 'true' }}
      />

      <TablePagination
        first={first}
        onChange={onPage}
        pageSizeOptions={CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_OPTIONS}
        rows={rows}
        total={total}
      />
    </SettingsCard>
  )
}
