/**
 * Pure presentation and classification rules for the configuration archive.
 *
 * The export and audit surface is built from three kinds of fact: the bytes the
 * server returned and their non-secret metadata headers, the audit page the
 * server answered, and the structured capability code that says this setting is
 * unavailable on the running backend. Everything here is a pure projection of
 * those facts, so the store stays about coordination and the components stay
 * about rendering.
 *
 * Nothing in this module can hold a passphrase or an archive body: the summary
 * carries the backend, the format version, the fingerprint and the byte length
 * only, and the audit projection reads metadata columns the server already
 * sent.
 */

import type {
  ConfigAuditEntry,
  ConfigImportApplied,
  ConfigImportPreview,
} from '@/generated/admin-api'
import type { AsyncActionOutcome } from './async-action'
import { formatDateTime, formatNumber } from '@/format/datetime-core'
import type { Locale } from '@/i18n'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  normalizePageSize,
  pageSizeStorageKey,
} from '@/models/pagination'
import { recordActionKey } from '@/stores/record-action-types'

/**
 * The two guarded resources of this tab.
 *
 * They are not `setting:` keys: the archive is not a settings section, and the
 * export and the import are independent operations, so neither may block the
 * other.
 */
export const CONFIG_ARCHIVE_EXPORT_KEY = recordActionKey(
  'configArchive',
  'export',
)

/** The export origin label, as the pending map reports it. */
export const CONFIG_ARCHIVE_EXPORT_ORIGIN = 'export'

/**
 * The import shares one claim between the preview and the confirmed apply, so a
 * preview cannot land inside an apply of the same archive, while the export
 * stays independent of both.
 */
export const CONFIG_ARCHIVE_IMPORT_KEY = recordActionKey(
  'configArchive',
  'import',
)

export const CONFIG_ARCHIVE_IMPORT_PREVIEW_ORIGIN = 'preview'
export const CONFIG_ARCHIVE_IMPORT_APPLY_ORIGIN = 'apply'

/** The file picker's accept list: the archive extension and its media type. */
export const CONFIG_ARCHIVE_FILE_ACCEPT = '.pfce,application/octet-stream'

export const CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_SCOPE = 'config-archive-audit'
export const CONFIG_ARCHIVE_AUDIT_DEFAULT_ROWS = 10
export const CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_OPTIONS = STANDARD_PAGE_SIZE_OPTIONS

/** The persisted page-size preference key; the value is a number, never data. */
export const CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_KEY = pageSizeStorageKey(
  CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_SCOPE,
)

export function configArchiveAuditPageSize(
  stored: number | string | null,
): number {
  return normalizePageSize(
    stored,
    CONFIG_ARCHIVE_AUDIT_DEFAULT_ROWS,
    CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_OPTIONS,
  )
}

/** The non-secret export outcome the card reports. */
export type ConfigArchiveExportSummary = {
  backendKind: string
  formatVersion: number | null
  payloadFingerprint: string
  size: number
}

export type ConfigArchiveExportInput = {
  backendKind: string
  formatVersion: number | null
  payloadFingerprint: string
  bytes: ArrayBuffer
}

/** Project the export result onto the summary: byte length, never the bytes. */
export function configArchiveExportSummary(
  result: ConfigArchiveExportInput,
): ConfigArchiveExportSummary {
  return {
    backendKind: result.backendKind,
    formatVersion: result.formatVersion,
    payloadFingerprint: result.payloadFingerprint,
    size: result.bytes.byteLength,
  }
}

/** The selected archive file, as the card remembers it: name and size only. */
export type ConfigArchiveImportFile = {
  name: string
  size: number
}

/** One domain of the import preview, as the table renders it. */
export type ConfigArchivePreviewRow = {
  name: string
  archiveRecords: number
  targetRecords: number
  creates: number
  updates: number
  deletes: number
  unrecoverableSecrets: number
}

/** One restored domain of the applied summary. */
export type ConfigArchiveAppliedRow = {
  name: string
  records: number
  unrecoverableSecrets: number
}

/**
 * The preview the wizard holds. It is the server's non-secret diff plus the
 * identity it was produced under, so a superseded answer can never be applied.
 */
export type ConfigArchiveImportPreview = {
  backendKind: string
  formatVersion: number
  payloadFingerprint: string
  exportedAt: string
  warnings: string[]
  domains: ConfigArchivePreviewRow[]
}

export type ConfigArchiveImportApplied = {
  backendKind: string
  formatVersion: number
  payloadFingerprint: string
  domains: ConfigArchiveAppliedRow[]
}

/**
 * The buttons the wizard enables.
 *
 * A preview needs a selected file, a non-empty passphrase and no import in
 * flight. The apply needs a current preview and the same idle state; pressing it
 * only opens the confirmation, so nothing reaches the server until the operator
 * confirms.
 */
export function configArchiveImportRequestEnabled(input: {
  hasFile: boolean
  passphrase: string
  pending: boolean
}): boolean {
  return !input.pending && input.hasFile && input.passphrase !== ''
}

export function configArchiveApplyEnabled(input: {
  hasPreview: boolean
  pending: boolean
}): boolean {
  return !input.pending && input.hasPreview
}

/** Project the server's preview onto the wizard's non-secret shape. */
export function configArchivePreview(
  preview: ConfigImportPreview,
): ConfigArchiveImportPreview {
  return {
    backendKind: preview.backend_kind,
    domains: preview.domains.map((domain) => ({
      archiveRecords: domain.archive_records,
      creates: domain.creates,
      deletes: domain.deletes,
      name: domain.name,
      targetRecords: domain.target_records,
      unrecoverableSecrets: domain.unrecoverable_secrets,
      updates: domain.updates,
    })),
    exportedAt: preview.exported_at,
    formatVersion: preview.format_version,
    payloadFingerprint: preview.payload_fingerprint,
    warnings: preview.warnings,
  }
}

/** Project the server's applied answer onto the summary the card reports. */
export function configArchiveApplied(
  applied: ConfigImportApplied,
): ConfigArchiveImportApplied {
  return {
    backendKind: applied.backend_kind,
    domains: applied.domains.map((domain) => ({
      name: domain.name,
      records: domain.records,
      unrecoverableSecrets: domain.unrecoverable_secrets,
    })),
    formatVersion: applied.format_version,
    payloadFingerprint: applied.payload_fingerprint,
  }
}

/** The file size the card reports, with the locale's grouping. */
export function configArchiveFileSize(size: number, locale: Locale): string {
  return formatNumber(size, locale)
}

/** The card's short fingerprint; the accessible title carries the full value. */
export function shortFingerprint(
  fingerprint: string | null | undefined,
): string {
  if (!fingerprint) return '-'
  return fingerprint.slice(0, 16)
}

/** Summed records across the audited domains. */
export function auditDomainRecords(entry: ConfigAuditEntry): number {
  return entry.domains.reduce((total, domain) => total + domain.records, 0)
}

/**
 * One audit row, as the table renders it.
 *
 * A nullable column renders as `-`; a real zero stays `0`, so an empty archive
 * and an unknown byte count never look the same.
 */
export type ConfigArchiveAuditRow = {
  auditId: number
  action: 'export' | 'import'
  actor: string
  backendKind: string
  archiveBytes: number | null
  fingerprint: string
  fullFingerprint: string
  records: number
  success: boolean
  errorCode: string
  errorMessage: string
  createdAt: string
}

export function configArchiveAuditRow(
  entry: ConfigAuditEntry,
  locale: Locale,
  timeZone?: string,
): ConfigArchiveAuditRow {
  return {
    action: entry.action,
    actor: entry.actor_login_name || '-',
    archiveBytes: entry.archive_bytes ?? null,
    auditId: entry.audit_id,
    backendKind: entry.backend_kind,
    createdAt: formatDateTime(entry.created_at, locale, undefined, timeZone),
    errorCode: entry.error_code || '-',
    errorMessage: entry.error_message || '',
    fingerprint: shortFingerprint(entry.payload_fingerprint),
    fullFingerprint: entry.payload_fingerprint || '',
    records: auditDomainRecords(entry),
    success: entry.success,
  }
}

/** A nullable byte count or record count: `-` when absent, `0` when real. */
export function formatAuditCount(
  value: number | null | undefined,
  locale: Locale,
): string {
  if (value === null || value === undefined || !Number.isFinite(value)) {
    return '-'
  }
  return formatNumber(value, locale)
}

/**
 * The distinct capability answers of this tab.
 *
 * The three archive routes are unavailable on SQLite, each with its own code.
 * They stay distinguishable here so the export card and the audit table can
 * each show their own unavailability instead of one generic error.
 */
export type ConfigArchiveCapability = 'export' | 'audit' | 'import'

const CAPABILITY_CODES: Readonly<Record<string, ConfigArchiveCapability>> = {
  sqlite_config_export_unavailable: 'export',
  sqlite_config_import_unavailable: 'import',
  sqlite_config_audit_unavailable: 'audit',
}

export function configArchiveCapability(
  cause: unknown,
): ConfigArchiveCapability | null {
  if (!cause || typeof cause !== 'object') return null
  if (!('error' in cause)) return null
  const envelope = (cause as { error?: unknown }).error
  if (!envelope || typeof envelope !== 'object') return null
  const code = (envelope as { code?: unknown }).code
  if (typeof code !== 'string') return null
  return CAPABILITY_CODES[code] ?? null
}

/** The capability answer's own text, for the unavailability box. */
export function configArchiveCapabilityMessage(cause: unknown): string {
  if (cause && typeof cause === 'object' && 'error' in cause) {
    const envelope = (cause as { error?: unknown }).error
    if (envelope && typeof envelope === 'object') {
      const code = (envelope as { code?: unknown }).code
      const message = (envelope as { message?: unknown }).message
      if (typeof code === 'string' && typeof message === 'string') {
        return `${code}: ${message}`
      }
      if (typeof message === 'string') return message
    }
  }
  return ''
}

/**
 * Whether an audit page should be re-read one page earlier.
 *
 * A page-size change or a shrinking trail can leave the window at or past the
 * total, which answers an empty page while the trail is not empty. Only the
 * last valid page is requested, once.
 */
export function configArchivePreviousPageFirst(
  first: number,
  rows: number,
  total: number,
  entryCount: number,
): number | null {
  if (entryCount > 0 || total <= 0 || first < total) return null
  const previous = Math.floor((total - 1) / rows) * rows
  // Without a real move backwards the re-read would answer the same empty page.
  return previous < first ? previous : null
}

/**
 * Whether the export button may be pressed: a non-empty passphrase and no
 * export in flight. There is no client-side length rule; the server owns the
 * passphrase bounds and answers them.
 */
export function configArchiveExportEnabled(input: {
  passphrase: string
  pending: boolean
}): boolean {
  return !input.pending && input.passphrase !== ''
}

/** The outcome types the card and the tab report through. */
export type ConfigArchiveExportOutcome = AsyncActionOutcome<void>

export function configArchiveExportExecuted(
  outcome: ConfigArchiveExportOutcome,
): boolean {
  return outcome.executed === true
}
