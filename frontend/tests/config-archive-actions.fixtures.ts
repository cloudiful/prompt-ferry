import { mock } from 'bun:test'
import type {
  ConfigImportApplied,
  ConfigImportPreview,
} from '../src/generated/admin-api'

const storage = new Map<string, string>()
Object.defineProperty(globalThis, 'localStorage', {
  value: {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  },
  configurable: true,
})

const actualApi = await import('../src/api/config-archive')
const archiveBytes = Uint8Array.from([65, 66, 67]).buffer

export const exportConfigArchive = mock(async (_passphrase: string) => ({
  backendKind: 'sqlite',
  bytes: archiveBytes,
  formatVersion: 1,
  payloadFingerprint: 'export-fingerprint',
}))
export const downloadConfigArchive = mock((_bytes: ArrayBuffer) => {})
export const previewConfigImportArchive = mock(
  async (_archive: string, _passphrase: string) => previewFixture(),
)
export const applyConfigImportArchive = mock(
  async (_archive: string, _passphrase: string) => appliedFixture(),
)
export const fetchConfigAuditPage = mock(
  async (first: number, rows: number) => ({
    entries: [],
    first,
    rows,
    total: 0,
  }),
)
export const readArchiveFile = mock(async (_file: File) => archiveBytes)

mock.module('../src/api/config-archive', () => ({
  ...actualApi,
  applyConfigImportArchive,
  downloadConfigArchive,
  exportConfigArchive,
  fetchConfigAuditPage,
  previewConfigImportArchive,
  readArchiveFile,
}))

const { createConfigArchiveStore } =
  await import('../src/stores/settings/config-archive')

export function previewFixture(): ConfigImportPreview {
  return {
    backend_kind: 'sqlite',
    domains: [],
    exported_at: '2026-10-12T00:00:00Z',
    format_version: 1,
    payload_fingerprint: 'preview-fingerprint',
    warnings: [],
  }
}

export function appliedFixture(): ConfigImportApplied {
  return {
    backend_kind: 'sqlite',
    domains: [],
    format_version: 1,
    payload_fingerprint: 'applied-fingerprint',
  }
}

export function deferred<T = void>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

export function freshArchiveStore() {
  exportConfigArchive.mockClear()
  downloadConfigArchive.mockClear()
  previewConfigImportArchive.mockClear()
  applyConfigImportArchive.mockClear()
  fetchConfigAuditPage.mockClear()
  readArchiveFile.mockClear()
  exportConfigArchive.mockImplementation(async () => ({
    backendKind: 'sqlite',
    bytes: archiveBytes,
    formatVersion: 1,
    payloadFingerprint: 'export-fingerprint',
  }))
  previewConfigImportArchive.mockImplementation(async () => previewFixture())
  applyConfigImportArchive.mockImplementation(async () => appliedFixture())
  fetchConfigAuditPage.mockImplementation(async (first, rows) => ({
    entries: [],
    first,
    rows,
    total: 0,
  }))
  readArchiveFile.mockImplementation(async () => archiveBytes)
  return createConfigArchiveStore()
}
