/**
 * Synthetic configuration-archive fixture format.
 *
 * This module exists so the fixture can answer a consistent export/import pair
 * instead of fixed bytes with an "anything non-empty works" passphrase. It is
 * NOT the production `.pfce` format, NOT encryption, and NOT a security test:
 * the real archive is a ChaCha20-Poly1305 sealed, Argon2id-keyed snapshot, while
 * this envelope is a plain-text magic line, a JSON header and the UTF-8 JSON
 * payload with two SHA-256 digests taken through `node:crypto`. The point is
 * only that the fixture rejects the cases the admin API rejects (passphrase
 * bounds, unknown header, unsupported version, foreign backend, wrong
 * passphrase, corrupt payload) with the backend's own codes, messages and
 * statuses, so the browser evidence has something honest to assert against.
 *
 * Layout, all UTF-8:
 *
 * ```text
 * "PROMPT_FERRY_SYNTHETIC_ARCHIVE\n"
 *   | {"version":1,"backend":"postgresql","passphrase_digest":"<sha256 hex>",
 *      "payload_fingerprint":"<sha256 hex>","payload":"<payload JSON string>"}
 * ```
 *
 * The payload string is an embedded JSON document so the fingerprint covers
 * exactly the bytes that were fingerprinted; nothing here is sealed, so a
 * tampered payload is only ever as detectable as that digest.
 */

import { createHash } from 'node:crypto'

export const SYNTHETIC_ARCHIVE_MAGIC = 'PROMPT_FERRY_SYNTHETIC_ARCHIVE'
export const SYNTHETIC_ARCHIVE_VERSION = 1
/** The backend the fixture pretends to export from and import into. */
export const SYNTHETIC_ARCHIVE_BACKEND = 'postgresql'
export const SYNTHETIC_ARCHIVE_BACKENDS = ['postgresql', 'sqlite'] as const
export const SYNTHETIC_MIN_PASSPHRASE_CHARS = 12
export const SYNTHETIC_MAX_PASSPHRASE_CHARS = 1024
export const SYNTHETIC_MAX_ARCHIVE_BYTES = 64 * 1024 * 1024
export const SYNTHETIC_MAX_ARCHIVE_BASE64_CHARS =
  Math.ceil(SYNTHETIC_MAX_ARCHIVE_BYTES / 3) * 4

export type SyntheticArchiveBackend =
  (typeof SYNTHETIC_ARCHIVE_BACKENDS)[number]

export type SyntheticArchiveDomain = {
  name: string
  records: number
  unrecoverable_secrets: number
}

export type SyntheticArchivePayload = {
  exported_at: string
  domains: SyntheticArchiveDomain[]
}

export type SyntheticArchiveEnvelope = {
  version: number
  backend: string
  passphrase_digest: string
  payload_fingerprint: string
  payload: string
}

/** An error body shaped exactly like the admin API's. */
export type SyntheticArchiveFailure = {
  code: string
  message: string
  status: number
}

export type SyntheticArchiveResult<T> =
  { ok: true; value: T } | { ok: false; error: SyntheticArchiveFailure }

export type EncodedSyntheticArchive = {
  bytes: Uint8Array
  payload_fingerprint: string
  backend: string
  format_version: number
}

export type DecodedSyntheticArchive = {
  payload: SyntheticArchivePayload
  payload_fingerprint: string
  backend: string
  format_version: number
}

export type SyntheticArchiveDiff = {
  name: string
  archive_records: number
  target_records: number
  creates: number
  updates: number
  deletes: number
  unrecoverable_secrets: number
}

export type SyntheticArchiveLimits = {
  maxBytes?: number
  maxBase64Chars?: number
}

export function sha256Hex(value: string): string {
  return createHash('sha256').update(value, 'utf8').digest('hex')
}

/** Rust's `chars().count()`, not UTF-16 units and not bytes. */
export function passphraseCharCount(passphrase: string): number {
  return [...passphrase].length
}

function failure(
  code: string,
  message: string,
  status: number,
): { ok: false; error: SyntheticArchiveFailure } {
  return { ok: false, error: { code, message, status } }
}

/** `validate_passphrase`: 12..1024 characters, both bounds answered as 400. */
export function validateSyntheticPassphrase(
  passphrase: string,
): SyntheticArchiveFailure | null {
  const chars = passphraseCharCount(passphrase)
  if (chars < SYNTHETIC_MIN_PASSPHRASE_CHARS) {
    return {
      code: 'archive_passphrase_too_short',
      message: `archive passphrase must be at least ${SYNTHETIC_MIN_PASSPHRASE_CHARS} characters`,
      status: 400,
    }
  }
  if (chars > SYNTHETIC_MAX_PASSPHRASE_CHARS) {
    return {
      code: 'archive_passphrase_too_long',
      message: `archive passphrase must be at most ${SYNTHETIC_MAX_PASSPHRASE_CHARS} characters`,
      status: 400,
    }
  }
  return null
}

/**
 * Seal the payload into the synthetic envelope. The passphrase is validated
 * here exactly as the codec does, and only its SHA-256 digest enters the
 * envelope: no plaintext passphrase is ever written into the bytes.
 */
export function encodeSyntheticArchive(
  passphrase: string,
  payload: SyntheticArchivePayload,
  backend: SyntheticArchiveBackend = SYNTHETIC_ARCHIVE_BACKEND,
): SyntheticArchiveResult<EncodedSyntheticArchive> {
  const invalid = validateSyntheticPassphrase(passphrase)
  if (invalid) return { ok: false, error: invalid }
  const payloadJson = JSON.stringify(payload)
  const envelope: SyntheticArchiveEnvelope = {
    version: SYNTHETIC_ARCHIVE_VERSION,
    backend,
    passphrase_digest: sha256Hex(passphrase),
    payload_fingerprint: sha256Hex(payloadJson),
    payload: payloadJson,
  }
  const bytes = new TextEncoder().encode(
    `${SYNTHETIC_ARCHIVE_MAGIC}\n${JSON.stringify(envelope)}`,
  )
  if (bytes.length > SYNTHETIC_MAX_ARCHIVE_BYTES) {
    return failure(
      'archive_too_large',
      `configuration archive exceeds the ${SYNTHETIC_MAX_ARCHIVE_BYTES} byte limit`,
      413,
    )
  }
  return {
    ok: true,
    value: {
      bytes,
      payload_fingerprint: envelope.payload_fingerprint,
      backend,
      format_version: SYNTHETIC_ARCHIVE_VERSION,
    },
  }
}

function payloadShapeValid(
  payload: unknown,
): payload is SyntheticArchivePayload {
  if (!payload || typeof payload !== 'object') return false
  const candidate = payload as Partial<SyntheticArchivePayload>
  if (typeof candidate.exported_at !== 'string') return false
  if (!Array.isArray(candidate.domains)) return false
  return candidate.domains.every((domain) => {
    if (!domain || typeof domain !== 'object') return false
    const entry = domain as Partial<SyntheticArchiveDomain>
    return (
      typeof entry.name === 'string' &&
      entry.name !== '' &&
      Number.isInteger(entry.records) &&
      (entry.records ?? -1) >= 0 &&
      Number.isInteger(entry.unrecoverable_secrets) &&
      (entry.unrecoverable_secrets ?? -1) >= 0
    )
  })
}

/**
 * Open the envelope, in the codec's order: passphrase bounds, size, header,
 * version, backend tag, target backend, passphrase, payload.
 *
 * The statuses follow the route that consumes the answer: the archive-level
 * codes keep the export handler's statuses, which are also what an import
 * answers for a structural archive failure.
 */
export function decodeSyntheticArchive(
  passphrase: string,
  bytes: Uint8Array,
  options: SyntheticArchiveLimits & { expectedBackend?: string } = {},
): SyntheticArchiveResult<DecodedSyntheticArchive> {
  const maxBytes = options.maxBytes ?? SYNTHETIC_MAX_ARCHIVE_BYTES
  const invalid = validateSyntheticPassphrase(passphrase)
  if (invalid) return { ok: false, error: invalid }
  if (bytes.length > maxBytes) {
    return failure(
      'archive_too_large',
      `configuration archive exceeds the ${maxBytes} byte limit`,
      413,
    )
  }
  const text = new TextDecoder().decode(bytes)
  const prefix = `${SYNTHETIC_ARCHIVE_MAGIC}\n`
  if (!text.startsWith(prefix)) {
    return failure(
      'archive_invalid_header',
      'not a prompt-ferry configuration archive',
      500,
    )
  }
  let envelope: Partial<SyntheticArchiveEnvelope>
  try {
    envelope = JSON.parse(
      text.slice(prefix.length),
    ) as Partial<SyntheticArchiveEnvelope>
  } catch {
    return failure(
      'archive_invalid_header',
      'not a prompt-ferry configuration archive',
      500,
    )
  }
  if (typeof envelope.version !== 'number') {
    return failure(
      'archive_invalid_header',
      'not a prompt-ferry configuration archive',
      500,
    )
  }
  if (envelope.version !== SYNTHETIC_ARCHIVE_VERSION) {
    return failure(
      'archive_unsupported_version',
      `unsupported configuration archive version ${envelope.version} (supported ${SYNTHETIC_ARCHIVE_VERSION})`,
      422,
    )
  }
  const backend = envelope.backend
  if (
    typeof backend !== 'string' ||
    !(SYNTHETIC_ARCHIVE_BACKENDS as ReadonlyArray<string>).includes(backend)
  ) {
    return failure(
      'archive_invalid_header',
      'not a prompt-ferry configuration archive',
      500,
    )
  }
  if (options.expectedBackend && options.expectedBackend !== backend) {
    return failure(
      'archive_backend_mismatch',
      `configuration archive was written by the ${backend} backend but the target is ${options.expectedBackend}`,
      422,
    )
  }
  if (
    typeof envelope.passphrase_digest !== 'string' ||
    envelope.passphrase_digest !== sha256Hex(passphrase)
  ) {
    return failure(
      'archive_decrypt_failed',
      'configuration archive could not be decrypted',
      400,
    )
  }
  if (typeof envelope.payload !== 'string') {
    return failure(
      'archive_invalid_payload',
      'configuration archive payload is invalid: unreadable snapshot',
      500,
    )
  }
  let payload: unknown
  try {
    payload = JSON.parse(envelope.payload)
  } catch {
    return failure(
      'archive_invalid_payload',
      'configuration archive payload is invalid: unreadable snapshot',
      500,
    )
  }
  if (
    typeof envelope.payload_fingerprint !== 'string' ||
    envelope.payload_fingerprint !== sha256Hex(envelope.payload)
  ) {
    return failure(
      'archive_invalid_payload',
      'configuration archive payload is invalid: payload fingerprint does not match',
      500,
    )
  }
  if (!payloadShapeValid(payload)) {
    return failure(
      'archive_invalid_payload',
      'configuration archive payload is invalid: snapshot failed validation',
      500,
    )
  }
  return {
    ok: true,
    value: {
      payload,
      payload_fingerprint: envelope.payload_fingerprint,
      backend,
      format_version: envelope.version,
    },
  }
}

/**
 * Decode a request's `archive_base64`, enforcing both size bounds the import
 * request has: the encoded length and the decoded length.
 */
export function decodeSyntheticArchiveBase64(
  archiveBase64: string,
  options: SyntheticArchiveLimits = {},
): SyntheticArchiveResult<Uint8Array> {
  const maxBase64Chars =
    options.maxBase64Chars ?? SYNTHETIC_MAX_ARCHIVE_BASE64_CHARS
  const maxBytes = options.maxBytes ?? SYNTHETIC_MAX_ARCHIVE_BYTES
  if (archiveBase64.length > maxBase64Chars) {
    return failure(
      'config_import_too_large',
      `configuration archive import request exceeds the ${maxBase64Chars} byte limit`,
      413,
    )
  }
  if (
    archiveBase64.length % 4 !== 0 ||
    !/^[A-Za-z0-9+/]*={0,2}$/.test(archiveBase64)
  ) {
    return invalidBase64()
  }
  const bytes = new Uint8Array(Buffer.from(archiveBase64, 'base64'))
  if (bytes.length > maxBytes) {
    return failure(
      'config_import_too_large',
      `configuration archive import request exceeds the ${maxBytes} byte limit`,
      413,
    )
  }
  return { ok: true, value: bytes }
}

/** `config_import_invalid_archive`: the import layer owns the base64 body. */
function invalidBase64(): { ok: false; error: SyntheticArchiveFailure } {
  return failure(
    'config_import_invalid_archive',
    'configuration archive cannot be imported: archive body is not valid base64',
    400,
  )
}

/** What the fixture may keep about an archive request body, and nothing else. */
export type SyntheticArchiveRequestSummary = {
  passphrase_present: boolean
  passphrase_chars: number
  archive_base64_present: boolean
  archive_base64_chars: number
}

/**
 * Reduce an archive request body to its non-secret shape.
 *
 * The fixture's own request log must never hold the passphrase or the archive
 * body, so an export or import request is recorded as presence flags and
 * lengths only: enough to prove what was sent, never the value. The passphrase
 * length is counted in characters, matching the codec's bound.
 */
export function summarizeSyntheticArchiveRequest(
  body: unknown,
): SyntheticArchiveRequestSummary {
  const candidate =
    body && typeof body === 'object'
      ? (body as { passphrase?: unknown; archive_base64?: unknown })
      : {}
  const passphrase =
    typeof candidate.passphrase === 'string' ? candidate.passphrase : ''
  const archiveBase64 =
    typeof candidate.archive_base64 === 'string' ? candidate.archive_base64 : ''
  return {
    passphrase_present: passphrase !== '',
    passphrase_chars: passphraseCharCount(passphrase),
    archive_base64_present: archiveBase64 !== '',
    archive_base64_chars: archiveBase64.length,
  }
}

/** The per-domain import warnings, in the backend's own wording. */
export function syntheticArchiveWarnings(
  payload: SyntheticArchivePayload,
): string[] {
  const warnings: string[] = []
  for (const domain of payload.domains) {
    if (domain.unrecoverable_secrets > 0) {
      warnings.push(
        `${domain.name}: ${domain.unrecoverable_secrets} record(s) carry an unrecoverable secret`,
      )
    }
  }
  return warnings
}

/**
 * The dry-run difference against the fixture's current record counts.
 *
 * Every archive record either updates a matched target record or creates a new
 * one, and every unmatched target record is a delete, so the three counts
 * reconcile with both sides for any pair of numbers.
 */
export function diffSyntheticArchive(
  payload: SyntheticArchivePayload,
  target: ReadonlyArray<{ name: string; records: number }>,
): SyntheticArchiveDiff[] {
  return payload.domains.map((domain) => {
    const targetRecords =
      target.find((entry) => entry.name === domain.name)?.records ?? 0
    const matched = Math.min(domain.records, targetRecords)
    return {
      name: domain.name,
      archive_records: domain.records,
      target_records: targetRecords,
      creates: domain.records - matched,
      updates: matched,
      deletes: targetRecords - matched,
      unrecoverable_secrets: domain.unrecoverable_secrets,
    }
  })
}
