import { describe, expect, it } from 'vitest'
import {
  buildExportRequest,
  buildImportRequest,
  CONFIG_ARCHIVE_FILENAME,
  encodeArchiveBase64,
} from '../../api/config-archive'

describe('config archive request shape', () => {
  it('builds a passphrase-only export request', () => {
    const request = buildExportRequest('correct horse battery staple')

    expect(request).toEqual({ passphrase: 'correct horse battery staple' })
    expect(Object.keys(request)).toEqual(['passphrase'])
  })

  it('builds an import request carrying base64 and passphrase only', () => {
    const request = buildImportRequest('QUJDRA==', 'archive-passphrase')

    expect(request).toEqual({
      archive_base64: 'QUJDRA==',
      passphrase: 'archive-passphrase',
    })
    expect(Object.keys(request).sort()).toEqual([
      'archive_base64',
      'passphrase',
    ])
  })
})

describe('config archive base64 encoding', () => {
  it('encodes an empty archive as an empty string', () => {
    expect(encodeArchiveBase64(new ArrayBuffer(0))).toBe('')
  })

  it('round-trips bytes that span more than one chunk', () => {
    const size = 0x8000 + 123
    const bytes = Uint8Array.from({ length: size }, (_, index) => index % 251)

    const encoded = encodeArchiveBase64(bytes.buffer)
    const decoded = Uint8Array.from(atob(encoded), (char) => char.charCodeAt(0))

    expect(decoded.length).toBe(size)
    expect(Array.from(decoded)).toEqual(Array.from(bytes))
  })
})

describe('config archive download name', () => {
  it('stays fixed and non-secret', () => {
    expect(CONFIG_ARCHIVE_FILENAME).toBe('prompt-ferry-config.pfce')
  })
})
