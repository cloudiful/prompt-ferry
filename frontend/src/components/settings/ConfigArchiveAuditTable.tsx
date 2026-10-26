import { useEffect, useRef } from 'react'
import { Clock, RefreshCw, TriangleAlert } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { SettingsCard } from '@/components/settings/SettingsCard'
import { TablePagination } from '@/components/shared/TablePagination'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { useLocale } from '@/lib/i18n'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { useNotifier } from '@/lib/notifier'
import { useSettingsConfigArchiveStore } from '@/lib/config-archive'
import {
  CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_KEY,
  CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_OPTIONS,
  configArchiveAuditPageSize,
  configArchiveAuditRow,
  formatAuditCount,
} from '@/lib/config-archive-model'
import { readStorage, writeStorage } from '@/storage'
import type { TablePageChange } from '@/models/pagination'

/**
 * The archive audit trail: newest first, one page at a time.
 *
 * Every column is metadata the server already returned — action, actor, backend,
 * byte count, fingerprint, summed domain records, result, error code and time.
 * A nullable column renders as `-` while a real zero stays `0`, the fingerprint
 * shows a short form with the full value in its title, and the timestamp uses
 * the active locale and display time zone rather than the Vue formatter.
 */
export function ConfigArchiveAuditTable() {
  const { t, locale } = useLocale()
  const { effective: timeZone } = useDisplayTimeZone()
  const { notifyApiError } = useNotifier()
  const entries = useSettingsConfigArchiveStore((state) => state.auditEntries)
  const first = useSettingsConfigArchiveStore((state) => state.auditFirst)
  const rows = useSettingsConfigArchiveStore((state) => state.auditRows)
  const total = useSettingsConfigArchiveStore((state) => state.auditTotal)
  const loading = useSettingsConfigArchiveStore((state) => state.auditLoading)
  const failed = useSettingsConfigArchiveStore((state) => state.auditFailed)
  const error = useSettingsConfigArchiveStore((state) => state.auditError)
  const unavailable = useSettingsConfigArchiveStore(
    (state) => state.auditUnavailable,
  )
  const unavailableMessage = useSettingsConfigArchiveStore(
    (state) => state.auditUnavailableMessage,
  )
  const setAuditRows = useSettingsConfigArchiveStore(
    (state) => state.setAuditRows,
  )
  const refreshAudit = useSettingsConfigArchiveStore(
    (state) => state.refreshAudit,
  )

  const rowsRef = useRef(rows)
  rowsRef.current = rows

  useEffect(() => {
    // The page size is a preference, not data: it is restored once on mount and
    // stored again whenever the operator changes it.
    const stored = readStorage(CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_KEY)
    setAuditRows(configArchiveAuditPageSize(stored))
  }, [setAuditRows])

  async function read(nextFirst = first, nextRows = rowsRef.current) {
    try {
      await refreshAudit(nextFirst, nextRows)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  function onPageChange(change: TablePageChange): void {
    // A page-size change arrives as a first-page change, so persisting the rows
    // here keeps the stored preference in step with what was read.
    if (change.rows !== rows) {
      setAuditRows(change.rows)
      writeStorage(CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_KEY, String(change.rows))
    }
    void read(change.first, change.rows)
  }

  const projected = entries.map((entry) =>
    configArchiveAuditRow(entry, locale, timeZone),
  )

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
          onClick={() => void read()}
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
            onClick={() => void read()}
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
            onClick={() => void read()}
            size="xs"
            type="button"
            variant="outline"
          >
            <RefreshCw aria-hidden="true" />
            {t('refresh')}
          </Button>
        </div>
      ) : null}

      <Table className="min-w-0" data-config-archive-audit="true">
        <TableHeader>
          <TableRow>
            <TableHead>{t('configArchiveAuditAction')}</TableHead>
            <TableHead>{t('configArchiveAuditActor')}</TableHead>
            <TableHead>{t('configArchiveExportBackend')}</TableHead>
            <TableHead>{t('configArchiveAuditBytes')}</TableHead>
            <TableHead>{t('configArchiveExportFingerprint')}</TableHead>
            <TableHead>{t('configArchiveAuditRecords')}</TableHead>
            <TableHead>{t('configArchiveAuditResult')}</TableHead>
            <TableHead>{t('error')}</TableHead>
            <TableHead>{t('configArchiveAuditCreatedAt')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {projected.map((row) => (
            <TableRow data-audit-id={row.auditId} key={row.auditId}>
              <TableCell>
                <span
                  className={
                    row.action === 'export'
                      ? 'rounded border border-primary/40 px-1.5 py-0.5 text-[10px] text-primary'
                      : 'rounded border border-warning/40 px-1.5 py-0.5 text-[10px] text-warning'
                  }
                >
                  {t(
                    row.action === 'export'
                      ? 'configArchiveAuditActionExport'
                      : 'configArchiveAuditActionImport',
                  )}
                </span>
              </TableCell>
              <TableCell>{row.actor}</TableCell>
              <TableCell>{row.backendKind}</TableCell>
              <TableCell className="tabular-nums">
                {formatAuditCount(row.archiveBytes, locale)}
              </TableCell>
              <TableCell>
                <span className="font-mono" title={row.fullFingerprint}>
                  {row.fingerprint}
                </span>
              </TableCell>
              <TableCell className="tabular-nums">
                {formatAuditCount(row.records, locale)}
              </TableCell>
              <TableCell>
                <span
                  data-audit-result={row.success ? 'success' : 'failed'}
                  className={
                    row.success
                      ? 'text-[11px] text-primary'
                      : 'text-[11px] text-destructive'
                  }
                >
                  {t(row.success ? 'success' : 'failed')}
                </span>
              </TableCell>
              <TableCell>
                <div className="grid gap-px">
                  <span>{row.errorCode}</span>
                  {row.errorMessage ? (
                    <span
                      className="block max-w-[16rem] truncate text-[11px] text-muted-foreground"
                      title={row.errorMessage}
                    >
                      {row.errorMessage}
                    </span>
                  ) : null}
                </div>
              </TableCell>
              <TableCell className="whitespace-nowrap">
                {row.createdAt}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>

      {projected.length === 0 && !loading && !failed ? (
        <p className="m-0 px-1 text-xs text-muted-foreground">
          {t('configArchiveAuditEmpty')}
        </p>
      ) : null}

      <TablePagination
        first={first}
        onChange={onPageChange}
        pageSizeOptions={CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_OPTIONS}
        rows={rows}
        total={total}
      />
    </SettingsCard>
  )
}
