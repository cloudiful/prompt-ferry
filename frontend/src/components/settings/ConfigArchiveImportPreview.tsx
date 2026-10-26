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
import { formatAuditCount, shortFingerprint } from '@/lib/config-archive-model'
import { useSettingsConfigArchiveStore } from '@/lib/config-archive'
import { formatDateTime } from '@/format/datetime-core'

/**
 * The import preview, and the applied summary that replaces it.
 *
 * Both are non-secret: the preview is the server's per-domain diff with its
 * exported-at, format, fingerprint and warnings, and the applied summary reports
 * every restored domain with its record and unrecoverable-secret counts. A null
 * count renders as `-` while a real zero stays `0`, and the time uses the active
 * locale and display zone.
 */
export function ConfigArchiveImportPreview() {
  const { t, locale } = useLocale()
  const { effective: timeZone } = useDisplayTimeZone()
  const preview = useSettingsConfigArchiveStore((state) => state.importPreview)
  const applied = useSettingsConfigArchiveStore((state) => state.importApplied)

  if (applied) {
    return (
      <section
        className="grid min-w-0 gap-2 rounded-md border border-primary/30 bg-primary/5 px-3 py-2"
        data-config-archive-applied="true"
        id="settings-config-archive-applied"
      >
        <p className="m-0 text-xs font-medium">
          {t('configArchiveImportApplied')}
        </p>
        <dl className="m-0 flex min-w-0 flex-wrap items-center gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
          <div className="flex items-center gap-1">
            <dt>{t('configArchiveExportBackend')}</dt>
            <dd className="m-0 font-medium">{applied.backendKind}</dd>
          </div>
          <div className="flex items-center gap-1">
            <dt>{t('configArchiveExportFormatVersion')}</dt>
            <dd className="m-0 font-medium">{applied.formatVersion}</dd>
          </div>
          <div className="flex items-center gap-1">
            <dt>{t('configArchiveExportFingerprint')}</dt>
            <dd className="m-0 font-mono" title={applied.payloadFingerprint}>
              {shortFingerprint(applied.payloadFingerprint)}
            </dd>
          </div>
        </dl>
        <Table className="min-w-0">
          <TableHeader>
            <TableRow>
              <TableHead>{t('name')}</TableHead>
              <TableHead>{t('configArchiveAuditRecords')}</TableHead>
              <TableHead>
                {t('configArchivePreviewUnrecoverableSecrets')}
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {applied.domains.map((domain) => (
              <TableRow data-applied-domain={domain.name} key={domain.name}>
                <TableCell>{domain.name}</TableCell>
                <TableCell className="tabular-nums">
                  {formatAuditCount(domain.records, locale)}
                </TableCell>
                <TableCell className="tabular-nums">
                  {formatAuditCount(domain.unrecoverableSecrets, locale)}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </section>
    )
  }

  if (!preview) return null

  return (
    <section
      className="grid min-w-0 gap-2 rounded-md border bg-card px-3 py-2"
      data-config-archive-preview="true"
      id="settings-config-archive-preview-panel"
    >
      <dl className="m-0 flex min-w-0 flex-wrap items-center gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
        <div className="flex items-center gap-1">
          <dt>{t('configArchiveExportBackend')}</dt>
          <dd className="m-0 font-medium">{preview.backendKind}</dd>
        </div>
        <div className="flex items-center gap-1">
          <dt>{t('configArchiveExportFormatVersion')}</dt>
          <dd className="m-0 font-medium">{preview.formatVersion}</dd>
        </div>
        <div className="flex items-center gap-1">
          <dt>{t('configArchiveExportFingerprint')}</dt>
          <dd className="m-0 font-mono" title={preview.payloadFingerprint}>
            {shortFingerprint(preview.payloadFingerprint)}
          </dd>
        </div>
        <div className="flex items-center gap-1">
          <dt>{t('configArchiveAuditCreatedAt')}</dt>
          <dd className="m-0 font-medium">
            {formatDateTime(preview.exportedAt, locale, undefined, timeZone)}
          </dd>
        </div>
      </dl>

      <Table className="min-w-0">
        <TableHeader>
          <TableRow>
            <TableHead>{t('name')}</TableHead>
            <TableHead>{t('configArchivePreviewTargetRecords')}</TableHead>
            <TableHead>{t('configArchiveAuditRecords')}</TableHead>
            <TableHead>{t('configArchivePreviewCreates')}</TableHead>
            <TableHead>{t('configArchivePreviewUpdates')}</TableHead>
            <TableHead>{t('configArchivePreviewDeletes')}</TableHead>
            <TableHead>
              {t('configArchivePreviewUnrecoverableSecrets')}
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {preview.domains.map((domain) => (
            <TableRow data-preview-domain={domain.name} key={domain.name}>
              <TableCell>{domain.name}</TableCell>
              <TableCell className="tabular-nums">
                {formatAuditCount(domain.targetRecords, locale)}
              </TableCell>
              <TableCell className="tabular-nums">
                {formatAuditCount(domain.archiveRecords, locale)}
              </TableCell>
              <TableCell className="tabular-nums">
                {formatAuditCount(domain.creates, locale)}
              </TableCell>
              <TableCell className="tabular-nums">
                {formatAuditCount(domain.updates, locale)}
              </TableCell>
              <TableCell className="tabular-nums">
                {formatAuditCount(domain.deletes, locale)}
              </TableCell>
              <TableCell className="tabular-nums">
                {formatAuditCount(domain.unrecoverableSecrets, locale)}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>

      {preview.warnings.length > 0 ? (
        <ul
          className="m-0 grid gap-1 pl-4 text-[11px] leading-relaxed text-muted-foreground"
          id="settings-config-archive-preview-warnings"
        >
          {preview.warnings.map((warning) => (
            <li key={warning}>{warning}</li>
          ))}
        </ul>
      ) : null}
    </section>
  )
}
