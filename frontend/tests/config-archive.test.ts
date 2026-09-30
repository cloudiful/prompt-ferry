import { beforeEach, expect, mock, test } from 'bun:test'
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
  // Keep the property configurable so the whole suite runs in one bun process
  // alongside other suites that redefine `globalThis.localStorage`.
  configurable: true,
})

const PASSPHRASE = 'hunter2-secret-pass'
const ARCHIVE_BYTES = Uint8Array.from([65, 66, 67, 68]).buffer
const ARCHIVE_BASE64 = 'QUJDRA=='

function previewFixture(): ConfigImportPreview {
  return {
    backend_kind: 'sqlite',
    domains: [
      {
        archive_records: 3,
        creates: 1,
        deletes: 0,
        name: 'endpoints',
        target_records: 2,
        unrecoverable_secrets: 0,
        updates: 1,
      },
    ],
    exported_at: '2026-09-30T00:00:00Z',
    format_version: 1,
    payload_fingerprint: 'fingerprint-preview',
    warnings: ['relay secrets are not restorable'],
  }
}

function appliedFixture(): ConfigImportApplied {
  return {
    backend_kind: 'sqlite',
    domains: [{ name: 'endpoints', records: 3, unrecoverable_secrets: 0 }],
    format_version: 1,
    payload_fingerprint: 'fingerprint-applied',
  }
}

const actualApi = await import('../src/api/config-archive')

const exportMock = mock(async () => ({
  backendKind: 'sqlite',
  bytes: ARCHIVE_BYTES,
  formatVersion: 1,
  payloadFingerprint: 'fingerprint-export',
}))
const downloadMock = mock((_bytes: ArrayBuffer) => {})
const previewMock = mock(async () => previewFixture())
const applyMock = mock(async () => appliedFixture())
const auditMock = mock(async (first: number, rows: number) => ({
  entries: [],
  first,
  rows,
  total: 0,
}))
const readFileMock = mock(async () => ARCHIVE_BYTES)

// Keep the pure helpers and every other export intact; only the network and
// browser side effects are replaced.
mock.module('../src/api/config-archive', () => ({
  ...actualApi,
  applyConfigImportArchive: applyMock,
  downloadConfigArchive: downloadMock,
  exportConfigArchive: exportMock,
  fetchConfigAuditPage: auditMock,
  previewConfigImportArchive: previewMock,
  readArchiveFile: readFileMock,
}))

const { createConfigArchiveStore } =
  await import('../src/stores/settings/config-archive')
const { resolveSettingsTab } = await import('../src/models/settings')
const { navItems, visibleNavItems } = await import('../src/nav')

beforeEach(() => {
  exportMock.mockClear()
  downloadMock.mockClear()
  previewMock.mockClear()
  applyMock.mockClear()
  auditMock.mockClear()
  readFileMock.mockClear()
})

test('config archive settings tab is admin-only', () => {
  expect(resolveSettingsTab('config-archive', true)).toBe('config-archive')
  expect(resolveSettingsTab('config-archive', false)).toBe('general')

  const navChild = navItems
    .find((item) => item.section === 'settings')
    ?.children?.find((child) => child.path === '/settings/config-archive')
  expect(navChild?.adminOnly).toBe(true)

  const adminChildren =
    visibleNavItems(true).find((item) => item.section === 'settings')
      ?.children ?? []
  const userChildren =
    visibleNavItems(false).find((item) => item.section === 'settings')
      ?.children ?? []
  expect(
    adminChildren.some((child) => child.path === '/settings/config-archive'),
  ).toBe(true)
  expect(
    userChildren.some((child) => child.path === '/settings/config-archive'),
  ).toBe(false)
})

test('export sends only the passphrase, downloads fixed-name bytes, then clears it', async () => {
  const store = createConfigArchiveStore()
  store.exportPassphrase.value = PASSPHRASE

  await store.exportArchive()

  expect(exportMock).toHaveBeenCalledTimes(1)
  expect(exportMock.mock.calls[0]?.[0]).toBe(PASSPHRASE)
  expect(downloadMock).toHaveBeenCalledWith(ARCHIVE_BYTES)
  expect(store.exportSummary.value).toEqual({
    backendKind: 'sqlite',
    formatVersion: 1,
    payloadFingerprint: 'fingerprint-export',
    size: 4,
  })
  expect(store.exportPassphrase.value).toBe('')
  expect(JSON.stringify(store.exportSummary.value)).not.toContain(PASSPHRASE)
})

test('selecting a file keeps only name, size, and transient base64', async () => {
  const store = createConfigArchiveStore()
  store.importPassphrase.value = PASSPHRASE

  await store.selectImportFile({
    name: 'backup.pfce',
    size: 4,
  } as unknown as File)

  expect(readFileMock).toHaveBeenCalledTimes(1)
  expect(store.importFile.value).toEqual({ name: 'backup.pfce', size: 4 })
  expect(store.importArchiveBase64.value).toBe(ARCHIVE_BASE64)

  store.clearImportFlow()
  expect(store.importArchiveBase64.value).toBe('')
  expect(store.importFile.value).toBeNull()
  expect(store.importPassphrase.value).toBe('')
})

test('preview maps the base64 body and passphrase without clearing the flow', async () => {
  const store = createConfigArchiveStore()
  store.importPassphrase.value = PASSPHRASE
  await store.selectImportFile({
    name: 'backup.pfce',
    size: 4,
  } as unknown as File)

  await store.previewImport()

  expect(previewMock).toHaveBeenCalledWith(ARCHIVE_BASE64, PASSPHRASE)
  expect(store.importPreview.value?.warnings).toEqual([
    'relay secrets are not restorable',
  ])
  expect(store.importPassphrase.value).toBe(PASSPHRASE)
})

test('apply sends the base64 body, clears every transient secret, and audits only ids', async () => {
  const store = createConfigArchiveStore()
  store.importPassphrase.value = PASSPHRASE
  await store.selectImportFile({
    name: 'backup.pfce',
    size: 4,
  } as unknown as File)
  await store.previewImport()

  await store.applyImport()

  expect(applyMock).toHaveBeenCalledWith(ARCHIVE_BASE64, PASSPHRASE)
  expect(store.importApplied.value?.payload_fingerprint).toBe(
    'fingerprint-applied',
  )
  expect(store.importPassphrase.value).toBe('')
  expect(store.importArchiveBase64.value).toBe('')
  expect(store.importFile.value).toBeNull()
  expect(store.importPreview.value).toBeNull()

  // The audit refresh carries pagination only, never a secret.
  expect(auditMock).toHaveBeenCalledWith(0, 10)
  const audited = JSON.stringify(auditMock.mock.calls)
  expect(audited).not.toContain(PASSPHRASE)
  expect(audited).not.toContain(ARCHIVE_BASE64)
})

test('unmounting clears passphrases and the archive body', async () => {
  const store = createConfigArchiveStore()
  store.exportPassphrase.value = PASSPHRASE
  store.importPassphrase.value = PASSPHRASE
  await store.selectImportFile({
    name: 'backup.pfce',
    size: 4,
  } as unknown as File)

  store.resetConfigArchiveSecrets()

  expect(store.exportPassphrase.value).toBe('')
  expect(store.importPassphrase.value).toBe('')
  expect(store.importArchiveBase64.value).toBe('')
})

test('audit refresh stores the page window it was asked for', async () => {
  const store = createConfigArchiveStore()
  auditMock.mockImplementationOnce(async (first: number, rows: number) => ({
    entries: [],
    first,
    rows,
    total: 42,
  }))

  await store.refreshConfigAudit(20, 20)

  expect(auditMock).toHaveBeenLastCalledWith(20, 20)
  expect(store.auditFirst.value).toBe(20)
  expect(store.auditRows.value).toBe(20)
  expect(store.auditTotal.value).toBe(42)
})
