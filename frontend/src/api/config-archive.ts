import {
  exportConfig,
  importConfig,
  listConfigAudit,
  previewConfigImport,
} from '../generated/admin-api'
import type {
  ConfigAuditPage,
  ConfigExportRequest,
  ConfigImportApplied,
  ConfigImportPreview,
  ConfigImportRequest,
} from '../generated/admin-api'
import { expectData, withData } from '../api'

/**
 * Fixed, non-secret archive download name. The backend `Content-Disposition`
 * filename carries a timestamp and is deliberately ignored so no server value
 * reaches the filesystem name; the passphrase never appears here.
 */
export const CONFIG_ARCHIVE_FILENAME = 'prompt-ferry-config.pfce'
export const CONFIG_ARCHIVE_MIME = 'application/octet-stream'

/** Non-secret export outcome: bytes plus the response metadata headers. */
export type ConfigArchiveExportResult = {
  bytes: ArrayBuffer
  backendKind: string
  formatVersion: number | null
  payloadFingerprint: string
}

export function buildExportRequest(passphrase: string): ConfigExportRequest {
  return { passphrase }
}

export function buildImportRequest(
  archiveBase64: string,
  passphrase: string,
): ConfigImportRequest {
  return { archive_base64: archiveBase64, passphrase }
}

export async function fetchConfigAuditPage(
  first: number,
  rows: number,
): Promise<ConfigAuditPage> {
  return expectData(
    await listConfigAudit<true>(withData({ query: { first, rows } })),
  )
}

export async function exportConfigArchive(
  passphrase: string,
): Promise<ConfigArchiveExportResult> {
  // The archive is binary; `arrayBuffer` keeps it byte-exact, and the
  // `fields` style is the only way to read the non-secret metadata headers.
  const result = (await exportConfig<true>({
    body: buildExportRequest(passphrase),
    parseAs: 'arrayBuffer',
    responseStyle: 'fields',
  })) as unknown as { data: ArrayBuffer; response: Response }
  const headers = result.response.headers
  return {
    bytes: result.data,
    backendKind: headers.get('x-config-export-backend') ?? '',
    formatVersion: parseIntegerHeader(
      headers.get('x-config-export-format-version'),
    ),
    payloadFingerprint: headers.get('x-config-export-fingerprint') ?? '',
  }
}

export async function previewConfigImportArchive(
  archiveBase64: string,
  passphrase: string,
): Promise<ConfigImportPreview> {
  return expectData(
    await previewConfigImport<true>(
      withData({ body: buildImportRequest(archiveBase64, passphrase) }),
    ),
  )
}

export async function applyConfigImportArchive(
  archiveBase64: string,
  passphrase: string,
): Promise<ConfigImportApplied> {
  return expectData(
    await importConfig<true>(
      withData({ body: buildImportRequest(archiveBase64, passphrase) }),
    ),
  )
}

/** Read a chosen archive file into bytes; the caller encodes and drops them. */
export async function readArchiveFile(file: File): Promise<ArrayBuffer> {
  return file.arrayBuffer()
}

/**
 * Standard base64 of the archive body. Encoding runs in chunks because a
 * single spread over a large archive would overflow the call stack.
 */
export function encodeArchiveBase64(bytes: ArrayBuffer): string {
  const view = new Uint8Array(bytes)
  const chunkSize = 0x8000
  let binary = ''
  for (let offset = 0; offset < view.length; offset += chunkSize) {
    binary += String.fromCharCode(...view.subarray(offset, offset + chunkSize))
  }
  return btoa(binary)
}

/** Browser-only: hand the already-produced bytes to a fixed-name download. */
export function downloadConfigArchive(bytes: ArrayBuffer): void {
  const url = URL.createObjectURL(
    new Blob([bytes], { type: CONFIG_ARCHIVE_MIME }),
  )
  const link = document.createElement('a')
  link.href = url
  link.download = CONFIG_ARCHIVE_FILENAME
  link.rel = 'noopener'
  link.click()
  URL.revokeObjectURL(url)
}

function parseIntegerHeader(value: string | null): number | null {
  if (!value) return null
  const parsed = Number.parseInt(value, 10)
  return Number.isFinite(parsed) ? parsed : null
}
