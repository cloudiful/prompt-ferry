/**
 * Bounded tests for the synthetic configuration-archive fixture format.
 *
 * Everything here is explicitly synthetic: the envelope is plain UTF-8 with two
 * SHA-256 digests, not the production `.pfce` encryption, so these tests pin the
 * fixture's own consistency and its backend-shaped rejections — never a real
 * archive, a real passphrase, or any security property.
 */

import { describe, expect, test } from 'bun:test'
import {
  SYNTHETIC_ARCHIVE_BACKEND,
  SYNTHETIC_ARCHIVE_MAGIC,
  SYNTHETIC_ARCHIVE_VERSION,
  SYNTHETIC_MAX_ARCHIVE_BYTES,
  SYNTHETIC_MAX_PASSPHRASE_CHARS,
  decodeSyntheticArchive,
  decodeSyntheticArchiveBase64,
  diffSyntheticArchive,
  encodeSyntheticArchive,
  passphraseCharCount,
  sha256Hex,
  summarizeSyntheticArchiveRequest,
  syntheticArchiveWarnings,
  validateSyntheticPassphrase,
  type SyntheticArchiveEnvelope,
  type SyntheticArchivePayload,
} from './config-archive'

const PASSPHRASE = 'synthetic-fixture-passphrase'
const OTHER_PASSPHRASE = 'synthetic-other-passphrase'

const payload: SyntheticArchivePayload = {
  exported_at: '2026-10-23T12:00:00.000Z',
  domains: [
    { name: 'endpoints', records: 3, unrecoverable_secrets: 1 },
    { name: 'model_routes', records: 2, unrecoverable_secrets: 0 },
    { name: 'mcp_servers', records: 2, unrecoverable_secrets: 1 },
  ],
}

function encoded() {
  const result = encodeSyntheticArchive(PASSPHRASE, payload)
  if (!result.ok) throw new Error('the synthetic payload did not encode')
  return result.value
}

/** Build an envelope by hand so a single field can be tampered with. */
function craft(overrides: Partial<SyntheticArchiveEnvelope> = {}): Uint8Array {
  const payloadJson = JSON.stringify(payload)
  const envelope: SyntheticArchiveEnvelope = {
    version: SYNTHETIC_ARCHIVE_VERSION,
    backend: SYNTHETIC_ARCHIVE_BACKEND,
    passphrase_digest: sha256Hex(PASSPHRASE),
    payload_fingerprint: sha256Hex(payloadJson),
    payload: payloadJson,
    ...overrides,
  }
  return new TextEncoder().encode(
    `${SYNTHETIC_ARCHIVE_MAGIC}\n${JSON.stringify(envelope)}`,
  )
}

function code(result: ReturnType<typeof decodeSyntheticArchive>) {
  return result.ok ? null : result.error.code
}

describe('synthetic config archive roundtrip', () => {
  test('re-encodes the same payload with the same fingerprint and byte length', () => {
    const first = encoded()
    const second = encoded()
    expect(first.bytes).toEqual(second.bytes)
    expect(first.payload_fingerprint).toBe(sha256Hex(JSON.stringify(payload)))
    expect(first.backend).toBe(SYNTHETIC_ARCHIVE_BACKEND)
    expect(first.format_version).toBe(SYNTHETIC_ARCHIVE_VERSION)
    expect(
      new TextDecoder().decode(
        first.bytes.slice(0, SYNTHETIC_ARCHIVE_MAGIC.length + 1),
      ),
    ).toBe(`${SYNTHETIC_ARCHIVE_MAGIC}\n`)

    const decoded = decodeSyntheticArchive(PASSPHRASE, first.bytes, {
      expectedBackend: SYNTHETIC_ARCHIVE_BACKEND,
    })
    expect(decoded.ok).toBe(true)
    if (!decoded.ok) return
    expect(decoded.value.payload).toEqual(payload)
    expect(decoded.value.payload_fingerprint).toBe(first.payload_fingerprint)
    expect(decoded.value.backend).toBe(SYNTHETIC_ARCHIVE_BACKEND)
    expect(decoded.value.format_version).toBe(SYNTHETIC_ARCHIVE_VERSION)
    expect(decoded.value.payload.domains).toEqual(payload.domains)
  })

  test('a base64 body decodes to the same bytes', () => {
    const { bytes } = encoded()
    const base64 = Buffer.from(bytes).toString('base64')
    const decoded = decodeSyntheticArchiveBase64(base64)
    expect(decoded.ok).toBe(true)
    if (!decoded.ok) return
    expect(decoded.value).toEqual(bytes)
  })

  test('never writes the passphrase into the envelope', () => {
    const text = new TextDecoder().decode(encoded().bytes)
    expect(text).not.toContain(PASSPHRASE)
    expect(text).toContain(sha256Hex(PASSPHRASE))
  })
})

describe('synthetic config archive passphrase bounds', () => {
  test('counts characters, not bytes or UTF-16 units', () => {
    expect(passphraseCharCount('密码密码密码')).toBe(6)
    expect(new TextEncoder().encode('密码密码密码').length).toBe(18)
    expect(validateSyntheticPassphrase('密码密码密码')?.code).toBe(
      'archive_passphrase_too_short',
    )
  })

  test('accepts the bound values and rejects one short or one long', () => {
    const shortest = 'a'.repeat(12)
    const longest = 'a'.repeat(SYNTHETIC_MAX_PASSPHRASE_CHARS)
    expect(validateSyntheticPassphrase(shortest)).toBeNull()
    expect(validateSyntheticPassphrase(longest)).toBeNull()

    const short = validateSyntheticPassphrase('a'.repeat(11))
    expect(short?.code).toBe('archive_passphrase_too_short')
    expect(short?.status).toBe(400)
    expect(short?.message).toBe(
      'archive passphrase must be at least 12 characters',
    )

    const long = validateSyntheticPassphrase(
      'a'.repeat(SYNTHETIC_MAX_PASSPHRASE_CHARS + 1),
    )
    expect(long?.code).toBe('archive_passphrase_too_long')
    expect(long?.status).toBe(400)
    expect(long?.message).toBe(
      'archive passphrase must be at most 1024 characters',
    )
  })

  test('refuses to encode with an out-of-range passphrase', () => {
    const result = encodeSyntheticArchive('short', payload)
    expect(result.ok).toBe(false)
    if (result.ok) return
    expect(result.error.code).toBe('archive_passphrase_too_short')
  })
})

describe('synthetic config archive rejections', () => {
  test('a wrong passphrase is archive_decrypt_failed and leaks nothing', () => {
    const result = decodeSyntheticArchive(OTHER_PASSPHRASE, craft())
    expect(code(result)).toBe('archive_decrypt_failed')
    if (result.ok) return
    expect(result.error.status).toBe(400)
    expect(result.error.message).toBe(
      'configuration archive could not be decrypted',
    )
    expect(JSON.stringify(result.error)).not.toContain(PASSPHRASE)
    expect(JSON.stringify(result.error)).not.toContain(OTHER_PASSPHRASE)
  })

  test('a foreign magic marker is archive_invalid_header', () => {
    const bytes = new TextEncoder().encode('{"not":"an archive"}')
    const result = decodeSyntheticArchive(PASSPHRASE, bytes)
    expect(code(result)).toBe('archive_invalid_header')
    if (result.ok) return
    expect(result.error.status).toBe(500)
    expect(result.error.message).toBe(
      'not a prompt-ferry configuration archive',
    )
  })

  test('a truncated or unparsable envelope is archive_invalid_header', () => {
    const truncated = decodeSyntheticArchive(
      PASSPHRASE,
      new TextEncoder().encode(`${SYNTHETIC_ARCHIVE_MAGIC}\n{`),
    )
    expect(code(truncated)).toBe('archive_invalid_header')
  })

  test('an unsupported version is archive_unsupported_version', () => {
    const result = decodeSyntheticArchive(PASSPHRASE, craft({ version: 9 }))
    expect(code(result)).toBe('archive_unsupported_version')
    if (result.ok) return
    expect(result.error.status).toBe(422)
    expect(result.error.message).toBe(
      'unsupported configuration archive version 9 (supported 1)',
    )
  })

  test('a foreign backend is archive_backend_mismatch', () => {
    const result = decodeSyntheticArchive(
      PASSPHRASE,
      craft({ backend: 'sqlite' }),
      {
        expectedBackend: SYNTHETIC_ARCHIVE_BACKEND,
      },
    )
    expect(code(result)).toBe('archive_backend_mismatch')
    if (result.ok) return
    expect(result.error.status).toBe(422)
    expect(result.error.message).toBe(
      'configuration archive was written by the sqlite backend but the target is postgresql',
    )
  })

  test('an unknown backend tag is archive_invalid_header', () => {
    const result = decodeSyntheticArchive(
      PASSPHRASE,
      craft({ backend: 'mysql' }),
    )
    expect(code(result)).toBe('archive_invalid_header')
  })

  test('a forged payload fingerprint is archive_invalid_payload', () => {
    const result = decodeSyntheticArchive(
      PASSPHRASE,
      craft({ payload_fingerprint: 'deadbeef' }),
    )
    expect(code(result)).toBe('archive_invalid_payload')
    if (result.ok) return
    expect(result.error.status).toBe(500)
    expect(result.error.message).toBe(
      'configuration archive payload is invalid: payload fingerprint does not match',
    )
  })

  test('an unreadable payload is archive_invalid_payload', () => {
    const result = decodeSyntheticArchive(
      PASSPHRASE,
      craft({
        payload: 'not json',
        payload_fingerprint: sha256Hex('not json'),
      }),
    )
    expect(code(result)).toBe('archive_invalid_payload')
    if (result.ok) return
    expect(result.error.message).toBe(
      'configuration archive payload is invalid: unreadable snapshot',
    )
  })

  test('a payload with an unusable domain shape fails validation', () => {
    const broken = JSON.stringify({
      exported_at: 'not a timestamp',
      domains: [{ name: '', records: -1, unrecoverable_secrets: 0 }],
    })
    const result = decodeSyntheticArchive(
      PASSPHRASE,
      craft({ payload: broken, payload_fingerprint: sha256Hex(broken) }),
    )
    expect(code(result)).toBe('archive_invalid_payload')
    if (result.ok) return
    expect(result.error.message).toBe(
      'configuration archive payload is invalid: snapshot failed validation',
    )
  })

  test('the size bound is enforced before the header is read', () => {
    const { bytes } = encoded()
    const result = decodeSyntheticArchive(PASSPHRASE, bytes, {
      maxBytes: bytes.length - 1,
    })
    expect(code(result)).toBe('archive_too_large')
    if (result.ok) return
    expect(result.error.status).toBe(413)
    expect(result.error.message).toBe(
      `configuration archive exceeds the ${bytes.length - 1} byte limit`,
    )
    expect(SYNTHETIC_MAX_ARCHIVE_BYTES).toBe(64 * 1024 * 1024)
  })

  test('a non-base64 body is config_import_invalid_archive', () => {
    const result = decodeSyntheticArchiveBase64('not*base64!')
    expect(result.ok).toBe(false)
    if (result.ok) return
    expect(result.error.code).toBe('config_import_invalid_archive')
    expect(result.error.status).toBe(400)
    expect(result.error.message).toBe(
      'configuration archive cannot be imported: archive body is not valid base64',
    )
  })

  test('an oversized import body is config_import_too_large', () => {
    const base64 = Buffer.from(encoded().bytes).toString('base64')
    const result = decodeSyntheticArchiveBase64(base64, {
      maxBase64Chars: 8,
    })
    expect(result.ok).toBe(false)
    if (result.ok) return
    expect(result.error.code).toBe('config_import_too_large')
    expect(result.error.status).toBe(413)
  })

  test('a decoded body above the byte bound is config_import_too_large', () => {
    const { bytes } = encoded()
    const result = decodeSyntheticArchiveBase64(
      Buffer.from(bytes).toString('base64'),
      { maxBytes: bytes.length - 1 },
    )
    expect(result.ok).toBe(false)
    if (result.ok) return
    expect(result.error.code).toBe('config_import_too_large')
    expect(result.error.status).toBe(413)
  })
})

describe('synthetic config archive request summary', () => {
  test('keeps only presence flags and lengths of a passphrase and a body', () => {
    const archiveBase64 = Buffer.from(encoded().bytes).toString('base64')
    const summary = summarizeSyntheticArchiveRequest({
      archive_base64: archiveBase64,
      passphrase: PASSPHRASE,
    })
    expect(summary).toEqual({
      archive_base64_chars: archiveBase64.length,
      archive_base64_present: true,
      passphrase_chars: PASSPHRASE.length,
      passphrase_present: true,
    })
    const serialized = JSON.stringify(summary)
    expect(serialized).not.toContain(PASSPHRASE)
    expect(serialized).not.toContain(archiveBase64.slice(0, 32))
  })

  test('counts passphrase characters and flags an absent or malformed body', () => {
    expect(
      summarizeSyntheticArchiveRequest({ passphrase: '密码密码密码' })
        .passphrase_chars,
    ).toBe(6)
    expect(summarizeSyntheticArchiveRequest(null)).toEqual({
      archive_base64_chars: 0,
      archive_base64_present: false,
      passphrase_chars: 0,
      passphrase_present: false,
    })
    expect(summarizeSyntheticArchiveRequest({ passphrase: 42 })).toEqual({
      archive_base64_chars: 0,
      archive_base64_present: false,
      passphrase_chars: 0,
      passphrase_present: false,
    })
    expect(
      summarizeSyntheticArchiveRequest({ archive_base64: '' })
        .archive_base64_present,
    ).toBe(false)
  })

  test('an export request body summarizes to the passphrase shape alone', () => {
    const summary = summarizeSyntheticArchiveRequest({ passphrase: PASSPHRASE })
    expect(summary.passphrase_present).toBe(true)
    expect(summary.archive_base64_present).toBe(false)
    expect(summary.archive_base64_chars).toBe(0)
  })
})

describe('synthetic config archive import preview shape', () => {
  test('the diff reconciles with both record counts', () => {
    const target = [
      { name: 'endpoints', records: 1 },
      { name: 'model_routes', records: 2 },
      { name: 'mcp_servers', records: 5 },
    ]
    const diff = diffSyntheticArchive(payload, target)
    expect(diff).toEqual([
      {
        archive_records: 3,
        creates: 2,
        deletes: 0,
        name: 'endpoints',
        target_records: 1,
        unrecoverable_secrets: 1,
        updates: 1,
      },
      {
        archive_records: 2,
        creates: 0,
        deletes: 0,
        name: 'model_routes',
        target_records: 2,
        unrecoverable_secrets: 0,
        updates: 2,
      },
      {
        archive_records: 2,
        creates: 0,
        deletes: 3,
        name: 'mcp_servers',
        target_records: 5,
        unrecoverable_secrets: 1,
        updates: 2,
      },
    ])
    for (const entry of diff) {
      expect(entry.creates + entry.updates).toBe(entry.archive_records)
      expect(entry.updates + entry.deletes).toBe(entry.target_records)
    }
  })

  test('an unchanged round trip is a pure update', () => {
    const diff = diffSyntheticArchive(
      payload,
      payload.domains.map((domain) => ({
        name: domain.name,
        records: domain.records,
      })),
    )
    for (const entry of diff) {
      expect(entry.creates).toBe(0)
      expect(entry.deletes).toBe(0)
      expect(entry.updates).toBe(entry.archive_records)
    }
  })

  test('a domain the fixture does not track counts every record as a create', () => {
    const [entry] = diffSyntheticArchive(payload, [])
    expect(entry?.target_records).toBe(0)
    expect(entry?.creates).toBe(3)
    expect(entry?.updates).toBe(0)
    expect(entry?.deletes).toBe(0)
  })

  test('warnings use the backend wording and only mention secret counts', () => {
    expect(syntheticArchiveWarnings(payload)).toEqual([
      'endpoints: 1 record(s) carry an unrecoverable secret',
      'mcp_servers: 1 record(s) carry an unrecoverable secret',
    ])
  })
})
