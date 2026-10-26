import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ConfigAuditEntry, ConfigAuditPage } from '@/generated/admin-api'

const api = vi.hoisted(() => ({
  downloadConfigArchive: vi.fn(),
  exportConfigArchive: vi.fn(),
  fetchConfigAuditPage: vi.fn(),
  buildExportRequest: (passphrase: string) => ({ passphrase }),
  buildImportRequest: (archiveBase64: string, passphrase: string) => ({
    archive_base64: archiveBase64,
    passphrase,
  }),
  previewConfigImportArchive: vi.fn(),
  applyConfigImportArchive: vi.fn(),
  readArchiveFile: vi.fn(),
  // The real helper encodes in 32 KB chunks; the mock keeps that shape so a
  // large synthetic body still exercises the chunk loop.
  encodeArchiveBase64: (bytes: ArrayBuffer): string => {
    const view = new Uint8Array(bytes)
    const chunkSize = 0x8000
    let binary = ''
    for (let offset = 0; offset < view.length; offset += chunkSize) {
      binary += String.fromCharCode(
        ...view.subarray(offset, offset + chunkSize),
      )
    }
    return btoa(binary)
  },
  CONFIG_ARCHIVE_FILENAME: 'prompt-ferry-config.pfce',
  CONFIG_ARCHIVE_MIME: 'application/octet-stream',
}))

vi.mock('@/api/config-archive', () => api)

import { messages } from '@/i18n'
import { getPendingActions } from '@/lib/async-action'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { setDisplayTimeZone } from '@/lib/display-timezone'
import { useSessionStore } from '@/lib/session'
import {
  configArchiveIdentityKey,
  disposeSettingsConfigArchive,
  setSettingsConfigArchiveNotifier,
  useSettingsConfigArchiveStore,
} from '@/lib/config-archive'
import {
  CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_KEY,
  CONFIG_ARCHIVE_EXPORT_KEY,
  CONFIG_ARCHIVE_FILE_ACCEPT,
  CONFIG_ARCHIVE_IMPORT_KEY,
  configArchiveApplied,
  configArchiveApplyEnabled,
  configArchiveFileSize,
  configArchiveImportRequestEnabled,
  configArchivePreview,
  auditDomainRecords,
  configArchiveAuditPageSize,
  configArchiveAuditRow,
  configArchiveCapability,
  configArchiveExportEnabled,
  configArchiveExportSummary,
  configArchivePreviousPageFirst,
  formatAuditCount,
  shortFingerprint,
} from '@/lib/config-archive-model'
import { ConfigArchiveExportCard } from '@/components/settings/ConfigArchiveExportCard'
import { SettingsConfigArchiveTab } from '@/components/settings/SettingsConfigArchiveTab'
import { ConfigArchiveImportCard } from '@/components/settings/ConfigArchiveImportCard'
import { ConfigArchiveImportConfirmDialog } from '@/components/settings/ConfigArchiveImportConfirmDialog'

const copy = messages['en-US']
const PASSPHRASE = 'synthetic-archive-passphrase'

const admin = {
  display_name: 'Fixture Administrator',
  is_admin: true,
  login_name: 'fixture-admin',
  user_id: 1,
}

const operator = {
  display_name: 'Fixture Operator',
  is_admin: false,
  login_name: 'fixture-operator',
  user_id: 2,
}

function auditEntry(
  overrides: Partial<ConfigAuditEntry> = {},
): ConfigAuditEntry {
  return {
    action: 'export',
    actor_login_name: 'fixture-admin',
    actor_user_id: 1,
    archive_bytes: 525,
    audit_id: 1001,
    backend_kind: 'postgresql',
    created_at: '2026-10-23T12:00:00.000Z',
    domains: [
      { name: 'endpoints', records: 3, unrecoverable_secrets: 1 },
      { name: 'model_routes', records: 2, unrecoverable_secrets: 0 },
    ],
    error_code: null,
    error_message: null,
    format_version: 1,
    payload_fingerprint: 'abcdef0123456789abcdef0123456789abcdef0123456789',
    success: true,
    ...overrides,
  }
}

function auditPage(
  entries: ConfigAuditEntry[],
  first = 0,
  rows = 10,
  total = entries.length,
): ConfigAuditPage {
  return { entries, first, rows, total }
}

function exportResult() {
  return {
    backendKind: 'postgresql',
    bytes: new ArrayBuffer(525),
    formatVersion: 1,
    payloadFingerprint: 'fedcba9876543210fedcba9876543210',
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((settle, fail) => {
    resolve = settle
    reject = fail
  })
  return { promise, reject, resolve }
}

function store() {
  return useSettingsConfigArchiveStore.getState()
}

async function flushMicrotasks(times = 8) {
  for (let index = 0; index < times; index += 1) {
    await act(async () => {
      await Promise.resolve()
    })
  }
}

function capabilityError(code: string) {
  return {
    error: { code, message: 'this Admin API capability is not available here' },
  }
}

function renderSurface(node: React.ReactNode) {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({
          defaultOptions: { queries: { retry: false } },
        })
      }
    >
      <LocaleProvider>
        <NotifierProvider>{node}</NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
}

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  setDisplayTimeZone('UTC')
  api.downloadConfigArchive.mockReset()
  api.exportConfigArchive.mockReset().mockResolvedValue(exportResult())
  api.fetchConfigAuditPage.mockReset().mockResolvedValue(auditPage([]))
  useSettingsConfigArchiveStore.getState().resetIdentity(null)
  setSettingsConfigArchiveNotifier(null)
  useSessionStore.setState({
    bootstrapped: true,
    bootstrapping: false,
    error: '',
    me: admin,
  })
})

afterEach(() => {
  cleanup()
  disposeSettingsConfigArchive()
  useSettingsConfigArchiveStore.getState().resetIdentity(null)
  localStorage.clear()
})

describe('config archive pure model', () => {
  it('projects the export result onto non-secret summary metadata', () => {
    const summary = configArchiveExportSummary(exportResult())
    expect(summary).toEqual({
      backendKind: 'postgresql',
      formatVersion: 1,
      payloadFingerprint: 'fedcba9876543210fedcba9876543210',
      size: 525,
    })
    expect(Object.keys(summary).sort()).toEqual([
      'backendKind',
      'formatVersion',
      'payloadFingerprint',
      'size',
    ])
    expect(shortFingerprint(summary.payloadFingerprint)).toBe(
      'fedcba9876543210',
    )
    expect(shortFingerprint('')).toBe('-')
    expect(shortFingerprint(null)).toBe('-')
  })

  it('keeps a real zero and a missing value apart in the audit row', () => {
    const locale = 'en-US' as const
    expect(formatAuditCount(0, locale)).toBe('0')
    expect(formatAuditCount(null, locale)).toBe('-')
    expect(formatAuditCount(undefined, locale)).toBe('-')

    const row = configArchiveAuditRow(
      auditEntry({ archive_bytes: 0 }),
      locale,
      'UTC',
    )
    expect(row.archiveBytes).toBe(0)
    expect(row.records).toBe(5)
    expect(row.actor).toBe('fixture-admin')
    expect(row.fingerprint).toBe('abcdef0123456789')
    expect(row.fullFingerprint).toBe(
      'abcdef0123456789abcdef0123456789abcdef0123456789',
    )
    expect(row.errorCode).toBe('-')
    expect(row.errorMessage).toBe('')

    const failed = configArchiveAuditRow(
      auditEntry({
        action: 'import',
        actor_login_name: null,
        archive_bytes: null,
        error_code: 'archive_decrypt_failed',
        error_message: 'configuration archive could not be decrypted',
        payload_fingerprint: null,
        success: false,
      }),
      locale,
      'UTC',
    )
    expect(failed.actor).toBe('-')
    expect(failed.archiveBytes).toBeNull()
    expect(failed.fingerprint).toBe('-')
    expect(failed.errorCode).toBe('archive_decrypt_failed')
    expect(failed.success).toBe(false)
  })

  it('formats the audit time with the active locale and display zone', () => {
    const entry = auditEntry({ created_at: '2026-10-23T12:00:00.000Z' })
    const utc = configArchiveAuditRow(entry, 'en-US', 'UTC')
    const tokyo = configArchiveAuditRow(entry, 'en-US', 'Asia/Tokyo')
    expect(utc.createdAt).not.toBe(tokyo.createdAt)
    expect(utc.createdAt).toContain('2026')
    const zh = configArchiveAuditRow(entry, 'zh-CN', 'UTC')
    expect(zh.createdAt).not.toBe(utc.createdAt)
  })

  it('sums the audited domain records', () => {
    expect(auditDomainRecords(auditEntry())).toBe(5)
    expect(auditDomainRecords(auditEntry({ domains: [] }))).toBe(0)
  })

  it('classifies the three distinct capability codes only', () => {
    expect(
      configArchiveCapability(
        capabilityError('sqlite_config_export_unavailable'),
      ),
    ).toBe('export')
    expect(
      configArchiveCapability(
        capabilityError('sqlite_config_audit_unavailable'),
      ),
    ).toBe('audit')
    expect(
      configArchiveCapability(
        capabilityError('sqlite_config_import_unavailable'),
      ),
    ).toBe('import')
    expect(
      configArchiveCapability(capabilityError('sqlite_settings_unavailable')),
    ).toBeNull()
    expect(configArchiveCapability(new Error('500 internal'))).toBeNull()
    expect(
      configArchiveCapability(new Error('sqlite_config_export_unavailable')),
    ).toBeNull()
  })

  it('re-reads only the last valid page when the window is past the total', () => {
    expect(configArchivePreviousPageFirst(20, 10, 16, 0)).toBe(10)
    expect(configArchivePreviousPageFirst(30, 10, 16, 0)).toBe(10)
    expect(configArchivePreviousPageFirst(10, 10, 16, 6)).toBeNull()
    expect(configArchivePreviousPageFirst(0, 10, 0, 0)).toBeNull()
    expect(configArchivePreviousPageFirst(0, 20, 16, 0)).toBeNull()
  })

  it('needs a non-empty passphrase and no export in flight', () => {
    expect(configArchiveExportEnabled({ passphrase: '', pending: false })).toBe(
      false,
    )
    expect(
      configArchiveExportEnabled({ passphrase: PASSPHRASE, pending: false }),
    ).toBe(true)
    expect(
      configArchiveExportEnabled({ passphrase: PASSPHRASE, pending: true }),
    ).toBe(false)
  })

  it('persists the page size under the archive scope with the standard options', () => {
    expect(CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_KEY).toBe(
      'prompt-ferry:table-page-size:config-archive-audit',
    )
    expect(configArchiveAuditPageSize('20')).toBe(20)
    expect(configArchiveAuditPageSize('50')).toBe(50)
    expect(configArchiveAuditPageSize('15')).toBe(10)
    expect(configArchiveAuditPageSize(null)).toBe(10)
  })
})

describe('config archive export store', () => {
  beforeEach(() => {
    store().initializeIdentity(admin)
  })

  it('sends only the passphrase, downloads once and refreshes the audit inside the claim', async () => {
    api.fetchConfigAuditPage.mockResolvedValue(
      auditPage([auditEntry({ audit_id: 2001 })]),
    )
    const download = vi.fn()
    const observed: Array<Record<string, string>> = []
    const release: Array<() => void> = []
    api.exportConfigArchive.mockImplementation(async () => {
      observed.push({ ...getPendingActions() })
      await new Promise<void>((resolve) => release.push(resolve))
      return exportResult()
    })

    store().setExportPassphrase(PASSPHRASE)
    const pending = store().exportArchive(download)
    await flushMicrotasks(2)
    expect(observed[0]?.[CONFIG_ARCHIVE_EXPORT_KEY]).toBe('export')

    // A second click while the first is in flight sends nothing.
    expect(await store().exportArchive(download)).toEqual({ executed: false })
    release.forEach((resolve) => resolve())
    await act(async () => {
      await pending
    })

    expect(api.exportConfigArchive).toHaveBeenCalledTimes(1)
    expect(api.exportConfigArchive).toHaveBeenCalledWith(PASSPHRASE)
    expect(download).toHaveBeenCalledTimes(1)
    expect(download.mock.calls[0]?.[0]?.byteLength).toBe(525)
    expect(api.fetchConfigAuditPage).toHaveBeenCalledTimes(1)
    // The audit refresh happened before the key was released.
    expect(store().exportPending).toBe(false)
    expect(store().exportSummary).toEqual({
      backendKind: 'postgresql',
      formatVersion: 1,
      payloadFingerprint: 'fedcba9876543210fedcba9876543210',
      size: 525,
    })
    expect(store().exportPassphrase).toBe('')
    expect(store().auditEntries).toHaveLength(1)
    expect(Object.hasOwn(getPendingActions(), CONFIG_ARCHIVE_EXPORT_KEY)).toBe(
      false,
    )
  })

  it('refuses an empty passphrase and never reaches the server', async () => {
    store().setExportPassphrase('')
    expect(await store().exportArchive(vi.fn())).toEqual({ executed: false })
    expect(api.exportConfigArchive).not.toHaveBeenCalled()
  })

  it('does not block or duplicate the future import key', async () => {
    const { runAsyncAction } = await import('@/lib/async-action')
    const release: Array<() => void> = []
    const held = runAsyncAction(
      'configArchive:import',
      () => new Promise<void>((resolve) => release.push(resolve)),
      'apply',
    )
    await flushMicrotasks(1)

    store().setExportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().exportArchive(vi.fn())
    })
    expect(api.exportConfigArchive).toHaveBeenCalledTimes(1)
    release.forEach((resolve) => resolve())
    await held
  })

  it('keeps the passphrase but reports one error when the server fails', async () => {
    const notifyApiError = vi.fn()
    setSettingsConfigArchiveNotifier({
      notifyApiError,
      notifyExportDone: vi.fn(),
      notifyImportApplied: vi.fn(),
    })
    api.exportConfigArchive.mockRejectedValueOnce(
      new Error('503 service unavailable'),
    )

    store().setExportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().exportArchive(vi.fn())
    })

    expect(notifyApiError).toHaveBeenCalledTimes(1)
    expect(store().exportFailed).toBe(true)
    expect(store().exportSummary).toBeNull()
    expect(store().exportPassphrase).toBe('')
    expect(api.fetchConfigAuditPage).not.toHaveBeenCalled()
    expect(store().exportUnavailable).toBe(false)
  })

  it('keeps a newer passphrase typed during the export', async () => {
    const write = deferred<ReturnType<typeof exportResult>>()
    api.exportConfigArchive.mockReturnValueOnce(write.promise)
    const download = vi.fn()

    store().setExportPassphrase(PASSPHRASE)
    const pending = store().exportArchive(download)
    await flushMicrotasks(2)
    store().setExportPassphrase('newer-typed-passphrase')
    write.resolve(exportResult())
    await act(async () => {
      await pending
    })

    expect(store().exportPassphrase).toBe('newer-typed-passphrase')
    expect(download).toHaveBeenCalledTimes(1)
  })

  it('reports the export capability answer and recovers on a retry', async () => {
    api.exportConfigArchive
      .mockRejectedValueOnce(
        capabilityError('sqlite_config_export_unavailable'),
      )
      .mockResolvedValueOnce(exportResult())

    store().setExportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().exportArchive(vi.fn())
    })
    expect(store().exportUnavailable).toBe(true)
    expect(store().exportUnavailableMessage).toBe(
      'sqlite_config_export_unavailable: this Admin API capability is not available here',
    )
    expect(store().auditUnavailable).toBe(false)

    store().setExportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().exportArchive(vi.fn())
    })
    expect(store().exportUnavailable).toBe(false)
    expect(store().exportSummary?.size).toBe(525)
  })

  it('drops the summary and the download when the identity changed in flight', async () => {
    const write = deferred<ReturnType<typeof exportResult>>()
    api.exportConfigArchive.mockReturnValueOnce(write.promise)
    const download = vi.fn()

    store().setExportPassphrase(PASSPHRASE)
    const pending = store().exportArchive(download)
    await flushMicrotasks(2)
    store().resetIdentity(operator)
    write.resolve(exportResult())
    await act(async () => {
      await pending
    })

    expect(download).not.toHaveBeenCalled()
    expect(store().exportSummary).toBeNull()
    expect(store().identityKey).toBe(configArchiveIdentityKey(operator))
    expect(store().exportPassphrase).toBe('')
  })

  it('clears every secret and the summary on unmount', async () => {
    api.fetchConfigAuditPage.mockResolvedValue(auditPage([auditEntry()]))
    await act(async () => {
      await store().refreshAudit()
    })
    store().setExportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().exportArchive(vi.fn())
    })
    expect(store().exportSummary).not.toBeNull()
    expect(store().auditEntries).toHaveLength(1)

    disposeSettingsConfigArchive()
    expect(store().exportPassphrase).toBe('')
    expect(store().exportSummary).toBeNull()
    expect(store().auditEntries).toEqual([])
    expect(store().auditTotal).toBe(0)
  })
})

describe('config archive audit store', () => {
  beforeEach(() => {
    store().initializeIdentity(admin)
  })

  it('reads the requested window and keeps the response contract', async () => {
    api.fetchConfigAuditPage.mockResolvedValue(
      auditPage([auditEntry()], 10, 20, 45),
    )
    await act(async () => {
      await store().refreshAudit(10, 20)
    })
    expect(api.fetchConfigAuditPage).toHaveBeenCalledWith(10, 20)
    expect(store().auditFirst).toBe(10)
    expect(store().auditRows).toBe(20)
    expect(store().auditTotal).toBe(45)
    expect(store().auditEntries).toHaveLength(1)
    expect(store().auditLoading).toBe(false)
  })

  it('re-reads the last valid page once when the window is past the total', async () => {
    api.fetchConfigAuditPage
      .mockResolvedValueOnce(auditPage([], 20, 10, 16))
      .mockResolvedValueOnce(
        auditPage([auditEntry({ audit_id: 7 })], 10, 10, 16),
      )

    await act(async () => {
      await store().refreshAudit(20, 10)
    })

    expect(api.fetchConfigAuditPage).toHaveBeenCalledTimes(2)
    expect(api.fetchConfigAuditPage).toHaveBeenLastCalledWith(10, 10)
    expect(store().auditFirst).toBe(10)
    expect(store().auditEntries).toHaveLength(1)
  })

  it('commits only the latest read and drops a stale identity', async () => {
    const first = deferred<ConfigAuditPage>()
    const second = deferred<ConfigAuditPage>()
    api.fetchConfigAuditPage
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise)

    void store().refreshAudit(0, 10)
    void store().refreshAudit(0, 10)
    await flushMicrotasks(2)
    second.resolve(auditPage([auditEntry({ audit_id: 2 })], 0, 10, 1))
    await flushMicrotasks(6)
    first.resolve(auditPage([auditEntry({ audit_id: 1 })], 0, 10, 1))
    await flushMicrotasks(6)
    expect(store().auditEntries.map((entry) => entry.audit_id)).toEqual([2])

    const stale = deferred<ConfigAuditPage>()
    api.fetchConfigAuditPage.mockReturnValueOnce(stale.promise)
    const read = store().refreshAudit()
    await flushMicrotasks(2)
    store().resetIdentity(operator)
    stale.resolve(auditPage([auditEntry({ audit_id: 3 })], 0, 10, 1))
    await act(async () => {
      await read
    })
    expect(store().auditEntries).toEqual([])
    expect(store().identityKey).toBe(configArchiveIdentityKey(operator))
  })

  it('reports a retryable failure and keeps the previous rows', async () => {
    api.fetchConfigAuditPage.mockResolvedValueOnce(auditPage([auditEntry()]))
    await act(async () => {
      await store().refreshAudit()
    })
    api.fetchConfigAuditPage.mockRejectedValueOnce(new Error('500 internal'))
    await act(async () => {
      await store().refreshAudit()
    })
    expect(store().auditFailed).toBe(true)
    expect(store().auditError).toBe('500 internal')
    expect(store().auditEntries).toHaveLength(1)
    expect(store().auditUnavailable).toBe(false)

    api.fetchConfigAuditPage.mockResolvedValueOnce(auditPage([auditEntry()]))
    await act(async () => {
      await store().refreshAudit()
    })
    expect(store().auditFailed).toBe(false)
  })

  it('marks only the audit capability code as unavailability', async () => {
    api.fetchConfigAuditPage.mockRejectedValueOnce(
      capabilityError('sqlite_config_audit_unavailable'),
    )
    await act(async () => {
      await store().refreshAudit()
    })
    expect(store().auditUnavailable).toBe(true)
    expect(store().auditUnavailableMessage).toBe(
      'sqlite_config_audit_unavailable: this Admin API capability is not available here',
    )
  })
})

describe('config archive export card', () => {
  beforeEach(() => {
    store().initializeIdentity(admin)
  })

  it('renders a password field, disables the export while empty, and leaks nothing', async () => {
    renderSurface(<ConfigArchiveExportCard />)
    await flushMicrotasks()

    const input = document.getElementById(
      'settings-config-archive-passphrase',
    ) as HTMLInputElement
    expect(input.getAttribute('type')).toBe('password')
    expect(input.getAttribute('autocomplete')).toBe('new-password')
    const button = document.getElementById(
      'settings-config-archive-export',
    ) as HTMLButtonElement
    expect(button.disabled).toBe(true)

    await act(async () => {
      fireEvent.change(input, { target: { value: PASSPHRASE } })
    })
    expect(button.disabled).toBe(false)
    const stored = Object.keys(localStorage).map((key) =>
      String(localStorage.getItem(key)),
    )
    expect(stored.join('\n')).not.toContain(PASSPHRASE)
    expect(document.body.textContent ?? '').not.toContain(PASSPHRASE)
    for (const node of document.querySelectorAll('[title]')) {
      expect(node.getAttribute('title') ?? '').not.toContain(PASSPHRASE)
    }
  })

  it('downloads through the fixed-name helper once and shows the summary', async () => {
    api.fetchConfigAuditPage.mockResolvedValue(auditPage([]))
    renderSurface(<ConfigArchiveExportCard />)
    await flushMicrotasks()

    await act(async () => {
      fireEvent.change(
        document.getElementById('settings-config-archive-passphrase')!,
        { target: { value: PASSPHRASE } },
      )
    })
    await act(async () => {
      fireEvent.click(
        document.getElementById('settings-config-archive-export')!,
      )
    })
    await flushMicrotasks()

    expect(api.downloadConfigArchive).toHaveBeenCalledTimes(1)
    expect(api.downloadConfigArchive.mock.calls[0]?.[0]?.byteLength).toBe(525)
    expect(api.CONFIG_ARCHIVE_FILENAME).toBe('prompt-ferry-config.pfce')
    expect(
      document.getElementById('settings-config-archive-export-fingerprint')
        ?.textContent,
    ).toBe('fedcba9876543210')
    expect(
      document
        .getElementById('settings-config-archive-export-fingerprint')
        ?.getAttribute('title'),
    ).toBe('fedcba9876543210fedcba9876543210')
    expect(document.body.textContent ?? '').toContain('525')
    expect(document.body.textContent ?? '').not.toContain(PASSPHRASE)
  })

  it('shows the capability warning and disables the export', async () => {
    store().initializeIdentity(admin)
    api.exportConfigArchive.mockRejectedValueOnce(
      capabilityError('sqlite_config_export_unavailable'),
    )
    renderSurface(<ConfigArchiveExportCard />)
    await flushMicrotasks()

    await act(async () => {
      fireEvent.change(
        document.getElementById('settings-config-archive-passphrase')!,
        { target: { value: PASSPHRASE } },
      )
    })
    await act(async () => {
      fireEvent.click(
        document.getElementById('settings-config-archive-export')!,
      )
    })
    await flushMicrotasks()

    expect(screen.getByText(/sqlite_config_export_unavailable/)).toBeTruthy()
    expect(
      (
        document.getElementById(
          'settings-config-archive-export',
        ) as HTMLButtonElement
      ).disabled,
    ).toBe(true)
  })
})

describe('config archive audit table', () => {
  beforeEach(() => {
    store().initializeIdentity(admin)
  })

  it('renders the metadata columns with a dash for null and 0 for zero', async () => {
    api.fetchConfigAuditPage.mockResolvedValue(
      auditPage([
        auditEntry({ audit_id: 1, archive_bytes: 0 }),
        auditEntry({
          action: 'import',
          actor_login_name: null,
          archive_bytes: null,
          audit_id: 2,
          error_code: 'archive_decrypt_failed',
          error_message: 'configuration archive could not be decrypted',
          payload_fingerprint: null,
          success: false,
        }),
      ]),
    )
    renderSurface(<SettingsConfigArchiveTab />)
    await flushMicrotasks()

    const rows = document.querySelectorAll('[data-audit-id]')
    expect(rows).toHaveLength(2)
    const first = rows[0]?.querySelectorAll('td')
    expect(first?.[1]?.textContent).toContain('fixture-admin')
    expect(first?.[2]?.textContent).toContain('postgresql')
    expect(first?.[3]?.textContent).toBe('0')
    expect(first?.[5]?.textContent).toBe('5')
    expect(first?.[6]?.textContent).toContain(copy.success)

    const second = rows[1]?.querySelectorAll('td')
    expect(second?.[1]?.textContent).toBe('-')
    expect(second?.[3]?.textContent).toBe('-')
    expect(second?.[4]?.textContent).toBe('-')
    expect(second?.[6]?.textContent).toContain(copy.failed)
    expect(second?.[7]?.textContent).toContain('archive_decrypt_failed')
  })

  it('shows the empty copy and the pager for the real total', async () => {
    api.fetchConfigAuditPage.mockResolvedValue(auditPage([], 0, 10, 0))
    renderSurface(<SettingsConfigArchiveTab />)
    await flushMicrotasks()
    expect(screen.getByText(copy.configArchiveAuditEmpty)).toBeTruthy()
  })

  it('reads the audit once on mount and never on a rerender', async () => {
    api.fetchConfigAuditPage.mockResolvedValue(auditPage([auditEntry()]))
    const view = renderSurface(<SettingsConfigArchiveTab />)
    await flushMicrotasks()
    expect(api.fetchConfigAuditPage).toHaveBeenCalledTimes(1)

    view.rerender(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <LocaleProvider>
          <NotifierProvider>
            <SettingsConfigArchiveTab />
          </NotifierProvider>
        </LocaleProvider>
      </QueryClientProvider>,
    )
    await flushMicrotasks()
    expect(api.fetchConfigAuditPage).toHaveBeenCalledTimes(1)
  })

  it('disables the passphrase and the audit controls while the export is in flight', async () => {
    api.fetchConfigAuditPage.mockResolvedValue(auditPage([auditEntry()]))
    const write = deferred<ReturnType<typeof exportResult>>()
    api.exportConfigArchive.mockReturnValueOnce(write.promise)
    renderSurface(<SettingsConfigArchiveTab />)
    await flushMicrotasks()

    await act(async () => {
      fireEvent.change(
        document.getElementById('settings-config-archive-passphrase')!,
        { target: { value: PASSPHRASE } },
      )
    })
    await act(async () => {
      fireEvent.click(
        document.getElementById('settings-config-archive-export')!,
      )
    })
    await flushMicrotasks(2)

    expect(
      (
        document.getElementById(
          'settings-config-archive-passphrase',
        ) as HTMLInputElement
      ).disabled,
    ).toBe(true)
    expect(
      (
        document.getElementById(
          'settings-config-archive-export',
        ) as HTMLButtonElement
      ).disabled,
    ).toBe(true)
    expect(document.querySelectorAll('[role="status"]').length).toBeGreaterThan(
      0,
    )

    // Settle the write so no claim outlives this test.
    write.resolve(exportResult())
    await flushMicrotasks(4)
  })
})

function previewResponse() {
  return {
    backend_kind: 'postgresql',
    domains: [
      {
        archive_records: 13,
        creates: 2,
        deletes: 0,
        name: 'endpoints',
        target_records: 13,
        unrecoverable_secrets: 1,
        updates: 11,
      },
      {
        archive_records: 12,
        creates: 0,
        deletes: 3,
        name: 'mcp_servers',
        target_records: 15,
        unrecoverable_secrets: 0,
        updates: 12,
      },
    ],
    exported_at: '2026-10-23T12:00:01.000Z',
    format_version: 1,
    payload_fingerprint: 'abcdef0123456789abcdef0123456789',
    warnings: ['endpoints: 1 record(s) carry an unrecoverable secret'],
  }
}

function appliedResponse() {
  return {
    backend_kind: 'postgresql',
    domains: [
      { name: 'endpoints', records: 13, unrecoverable_secrets: 1 },
      { name: 'mcp_servers', records: 12, unrecoverable_secrets: 0 },
    ],
    format_version: 1,
    payload_fingerprint: 'abcdef0123456789abcdef0123456789',
  }
}

function archiveFile(name = 'backup.pfce', size?: number) {
  const bytes = new Uint8Array(size ?? 64)
  bytes.set(new TextEncoder().encode('PROMPT_FERRY_SYNTHETIC_ARCHIVE'))
  return new File([bytes], name)
}

describe('config archive import model', () => {
  it('projects the server preview and applied answer without secrets', () => {
    const preview = configArchivePreview(previewResponse())
    expect(preview).toEqual({
      backendKind: 'postgresql',
      domains: [
        {
          archiveRecords: 13,
          creates: 2,
          deletes: 0,
          name: 'endpoints',
          targetRecords: 13,
          unrecoverableSecrets: 1,
          updates: 11,
        },
        {
          archiveRecords: 12,
          creates: 0,
          deletes: 3,
          name: 'mcp_servers',
          targetRecords: 15,
          unrecoverableSecrets: 0,
          updates: 12,
        },
      ],
      exportedAt: '2026-10-23T12:00:01.000Z',
      formatVersion: 1,
      payloadFingerprint: 'abcdef0123456789abcdef0123456789',
      warnings: ['endpoints: 1 record(s) carry an unrecoverable secret'],
    })
    for (const domain of preview.domains) {
      expect(domain.creates + domain.updates).toBe(domain.archiveRecords)
    }

    const applied = configArchiveApplied(appliedResponse())
    expect(applied.domains).toEqual([
      { name: 'endpoints', records: 13, unrecoverableSecrets: 1 },
      { name: 'mcp_servers', records: 12, unrecoverableSecrets: 0 },
    ])
    expect(applied.formatVersion).toBe(1)
  })

  it('enables the wizard only with the full input and no import in flight', () => {
    expect(
      configArchiveImportRequestEnabled({
        hasFile: true,
        passphrase: PASSPHRASE,
        pending: false,
      }),
    ).toBe(true)
    expect(
      configArchiveImportRequestEnabled({
        hasFile: false,
        passphrase: PASSPHRASE,
        pending: false,
      }),
    ).toBe(false)
    expect(
      configArchiveImportRequestEnabled({
        hasFile: true,
        passphrase: '',
        pending: false,
      }),
    ).toBe(false)
    expect(
      configArchiveImportRequestEnabled({
        hasFile: true,
        passphrase: PASSPHRASE,
        pending: true,
      }),
    ).toBe(false)
    expect(
      configArchiveApplyEnabled({ hasPreview: true, pending: false }),
    ).toBe(true)
    expect(
      configArchiveApplyEnabled({ hasPreview: false, pending: false }),
    ).toBe(false)
    expect(configArchiveApplyEnabled({ hasPreview: true, pending: true })).toBe(
      false,
    )
  })

  it('accepts only the archive extension and media type', () => {
    expect(CONFIG_ARCHIVE_FILE_ACCEPT).toBe('.pfce,application/octet-stream')
    expect(configArchiveFileSize(40960, 'en-US')).toBe('40,960')
    expect(configArchiveFileSize(40960, 'zh-CN')).toBe('40,960')
  })
})

describe('config archive import store', () => {
  beforeEach(() => {
    store().initializeIdentity(admin)
    api.readArchiveFile.mockImplementation((file: File) => file.arrayBuffer())
    api.previewConfigImportArchive
      .mockReset()
      .mockResolvedValue(previewResponse())
    api.applyConfigImportArchive
      .mockReset()
      .mockResolvedValue(appliedResponse())
  })

  it('keeps only the file name and size, and never the body', async () => {
    await act(async () => {
      await store().selectImportFile(archiveFile('backup.pfce', 40960))
    })
    expect(api.readArchiveFile).toHaveBeenCalledTimes(1)
    expect(store().importFile).toEqual({ name: 'backup.pfce', size: 40960 })
    const serialized = JSON.stringify(store())
    expect(serialized).not.toContain('PROMPT_FERRY_SYNTHETIC_ARCHIVE')
    expect(serialized).not.toContain('UFJPTVBUX0ZFUlJZX1NZTlRIRVRJQ19BUkNISVZF')
    expect(Object.keys(store())).not.toContain('importArchiveBase64')
  })

  it('accepts the same file twice and encodes a body above 32 KB', async () => {
    const big = archiveFile('big.pfce', 40960)
    await act(async () => {
      await store().selectImportFile(big)
    })
    expect(store().importFile?.size).toBe(40960)
    api.readArchiveFile.mockImplementation((file: File) => file.arrayBuffer())
    await act(async () => {
      await store().selectImportFile(big)
    })
    expect(api.readArchiveFile).toHaveBeenCalledTimes(2)

    api.previewConfigImportArchive.mockResolvedValueOnce(previewResponse())
    store().setImportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().previewImport()
    })
    const [base64] = api.previewConfigImportArchive.mock.calls[0] ?? []
    expect(typeof base64).toBe('string')
    expect((base64 as string).length).toBe(Math.ceil(40960 / 3) * 4)
    const decoded = atob(base64 as string)
    expect(decoded.length).toBe(40960)
    expect(decoded.startsWith('PROMPT_FERRY_SYNTHETIC_ARCHIVE')).toBe(true)
  })

  it('sends only the body and passphrase, commits the preview and never audits', async () => {
    await act(async () => {
      await store().selectImportFile(archiveFile())
    })
    store().setImportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().previewImport()
    })

    expect(api.previewConfigImportArchive).toHaveBeenCalledTimes(1)
    expect(api.previewConfigImportArchive.mock.calls[0]?.[1]).toBe(PASSPHRASE)
    expect(store().importPreview?.domains).toHaveLength(2)
    expect(store().importPreview?.warnings).toHaveLength(1)
    expect(api.applyConfigImportArchive).not.toHaveBeenCalled()
    expect(api.fetchConfigAuditPage).not.toHaveBeenCalled()
    expect(Object.hasOwn(getPendingActions(), CONFIG_ARCHIVE_IMPORT_KEY)).toBe(
      false,
    )
  })

  it('clears a rejected passphrase, keeps the file and reports once', async () => {
    await act(async () => {
      await store().selectImportFile(archiveFile())
    })
    const notifyApiError = vi.fn()
    setSettingsConfigArchiveNotifier({
      notifyApiError,
      notifyExportDone: vi.fn(),
      notifyImportApplied: vi.fn(),
    })
    api.previewConfigImportArchive.mockRejectedValueOnce({
      error: {
        code: 'archive_decrypt_failed',
        message: 'configuration archive could not be decrypted',
      },
    })
    store().setImportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().previewImport()
    })

    expect(notifyApiError).toHaveBeenCalledTimes(1)
    expect(store().importPassphrase).toBe('')
    expect(store().importFile?.name).toBe('backup.pfce')
    expect(store().importPreview).toBeNull()
    expect(store().importError).toBe(
      'archive_decrypt_failed: configuration archive could not be decrypted',
    )
    expect(store().importUnavailable).toBe(false)
  })

  it('marks the import capability code and recovers on a retry', async () => {
    await act(async () => {
      await store().selectImportFile(archiveFile())
    })
    api.previewConfigImportArchive.mockRejectedValueOnce(
      capabilityError('sqlite_config_import_unavailable'),
    )
    store().setImportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().previewImport()
    })
    expect(store().importUnavailable).toBe(true)
    expect(store().importUnavailableMessage).toContain(
      'sqlite_config_import_unavailable',
    )
    expect(store().importError).not.toContain(PASSPHRASE)

    store().setImportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().previewImport()
    })
    expect(store().importUnavailable).toBe(false)
    expect(store().importPreview).not.toBeNull()
  })

  it('opens the confirmation with zero requests and cancels without applying', async () => {
    await act(async () => {
      await store().selectImportFile(archiveFile())
    })
    store().setImportPassphrase(PASSPHRASE)
    // No preview yet: the apply action does nothing at all.
    store().requestApply()
    expect(store().confirmOpen).toBe(false)

    await act(async () => {
      await store().previewImport()
    })
    api.applyConfigImportArchive.mockClear()
    store().requestApply()
    expect(store().confirmOpen).toBe(true)
    expect(api.applyConfigImportArchive).not.toHaveBeenCalled()

    store().cancelApply()
    expect(store().confirmOpen).toBe(false)
    expect(store().importFile).toBeNull()
    expect(store().importPassphrase).toBe('')
    expect(store().importPreview).toBeNull()
    expect(api.applyConfigImportArchive).not.toHaveBeenCalled()
    expect(api.fetchConfigAuditPage).not.toHaveBeenCalled()
  })

  it('applies the same body, audits inside the claim and clears the flow', async () => {
    const notifyImportApplied = vi.fn()
    setSettingsConfigArchiveNotifier({
      notifyApiError: vi.fn(),
      notifyExportDone: vi.fn(),
      notifyImportApplied,
    })
    await act(async () => {
      await store().selectImportFile(archiveFile())
    })
    store().setImportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().previewImport()
    })
    const previewBody = api.previewConfigImportArchive.mock.calls[0]?.[0]
    api.fetchConfigAuditPage.mockResolvedValue(auditPage([auditEntry()]))
    store().requestApply()

    await act(async () => {
      await store().confirmApply()
    })

    expect(api.applyConfigImportArchive).toHaveBeenCalledTimes(1)
    expect(api.applyConfigImportArchive.mock.calls[0]?.[0]).toBe(previewBody)
    expect(api.applyConfigImportArchive.mock.calls[0]?.[1]).toBe(PASSPHRASE)
    expect(api.fetchConfigAuditPage).toHaveBeenCalledTimes(1)
    expect(store().importApplied?.domains).toHaveLength(2)
    expect(store().importApplied?.domains[0]?.records).toBe(13)
    expect(store().importFile).toBeNull()
    expect(store().importPassphrase).toBe('')
    expect(store().importPreview).toBeNull()
    expect(store().confirmOpen).toBe(false)
    expect(notifyImportApplied).toHaveBeenCalledTimes(1)
  })

  it('sends one request for a duplicate confirm and none for a declined one', async () => {
    await act(async () => {
      await store().selectImportFile(archiveFile())
    })
    store().setImportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().previewImport()
    })
    store().requestApply()

    // Hold the shared import claim: a second confirm must decline.
    const { runAsyncAction } = await import('@/lib/async-action')
    const release: Array<() => void> = []
    const held = runAsyncAction(
      CONFIG_ARCHIVE_IMPORT_KEY,
      () => new Promise<void>((resolve) => release.push(resolve)),
      'apply',
    )
    await flushMicrotasks(1)
    expect(await store().confirmApply()).toEqual({ executed: false })
    expect(api.applyConfigImportArchive).not.toHaveBeenCalled()
    release.forEach((resolve) => resolve())
    await held

    const write = deferred<ReturnType<typeof appliedResponse>>()
    api.applyConfigImportArchive.mockReturnValueOnce(write.promise)
    const first = store().confirmApply()
    await flushMicrotasks(2)
    expect(await store().confirmApply()).toEqual({ executed: false })
    expect(api.applyConfigImportArchive).toHaveBeenCalledTimes(1)
    write.resolve(appliedResponse())
    await act(async () => {
      await first
    })
    expect(store().importApplied).not.toBeNull()
  })

  it('keeps the confirmed wizard on a 503 and retries the same body', async () => {
    const notifyApiError = vi.fn()
    setSettingsConfigArchiveNotifier({
      notifyApiError,
      notifyExportDone: vi.fn(),
      notifyImportApplied: vi.fn(),
    })
    await act(async () => {
      await store().selectImportFile(archiveFile())
    })
    store().setImportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().previewImport()
    })
    const body = api.previewConfigImportArchive.mock.calls[0]?.[0]
    store().requestApply()

    api.applyConfigImportArchive.mockRejectedValueOnce(
      new Error('503 service unavailable'),
    )
    await act(async () => {
      await store().confirmApply()
    })
    expect(notifyApiError).toHaveBeenCalledTimes(1)
    expect(store().confirmOpen).toBe(true)
    expect(store().importFile?.name).toBe('backup.pfce')
    expect(store().importPassphrase).toBe(PASSPHRASE)
    expect(store().importPreview).not.toBeNull()
    expect(store().importApplied).toBeNull()

    api.fetchConfigAuditPage.mockResolvedValue(auditPage([auditEntry()]))
    await act(async () => {
      await store().confirmApply()
    })
    expect(api.applyConfigImportArchive).toHaveBeenCalledTimes(2)
    expect(api.applyConfigImportArchive.mock.calls[1]?.[0]).toBe(body)
    expect(store().importApplied).not.toBeNull()
    expect(store().importFile).toBeNull()
  })

  it('drops a preview superseded by a newer pick or passphrase', async () => {
    // A slow file read: the second pick lands first and only it may commit.
    const slowRead = deferred<ArrayBuffer>()
    api.readArchiveFile.mockReturnValueOnce(slowRead.promise)
    const firstPick = store().selectImportFile(archiveFile('older.pfce'))
    await flushMicrotasks(1)
    await act(async () => {
      await store().selectImportFile(archiveFile('newer.pfce'))
    })
    slowRead.resolve(new ArrayBuffer(8))
    await act(async () => {
      await firstPick
    })
    expect(store().importFile?.name).toBe('newer.pfce')

    store().setImportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().previewImport()
    })
    expect(store().importPreview).not.toBeNull()
    store().setImportPassphrase('another-synthetic-passphrase')
    expect(store().importPreview).toBeNull()
    expect(store().confirmOpen).toBe(false)
  })

  it('drops a superseded apply and a preview from another identity', async () => {
    await act(async () => {
      await store().selectImportFile(archiveFile())
    })
    store().setImportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().previewImport()
    })
    store().requestApply()

    const write = deferred<ReturnType<typeof appliedResponse>>()
    api.applyConfigImportArchive.mockReturnValueOnce(write.promise)
    api.fetchConfigAuditPage.mockResolvedValue(auditPage([]))
    const pending = store().confirmApply()
    await flushMicrotasks(2)
    store().resetIdentity(operator)
    write.resolve(appliedResponse())
    await act(async () => {
      await pending
    })
    expect(store().importApplied).toBeNull()
    expect(store().identityKey).toBe(configArchiveIdentityKey(operator))
  })

  it('keeps the export available while an import is in flight and the reverse', async () => {
    await act(async () => {
      await store().selectImportFile(archiveFile())
    })
    store().setImportPassphrase(PASSPHRASE)
    api.exportConfigArchive.mockResolvedValue(exportResult())

    const slowPreview = deferred<ReturnType<typeof previewResponse>>()
    api.previewConfigImportArchive.mockReturnValueOnce(slowPreview.promise)
    const previewing = store().previewImport()
    await flushMicrotasks(2)

    store().setExportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().exportArchive(vi.fn())
    })
    expect(api.exportConfigArchive).toHaveBeenCalledTimes(1)

    slowPreview.resolve(previewResponse())
    await act(async () => {
      await previewing
    })
    expect(store().importPreview).not.toBeNull()
  })

  it('clears every transient import secret and the file on unmount', async () => {
    await act(async () => {
      await store().selectImportFile(archiveFile())
    })
    store().setImportPassphrase(PASSPHRASE)
    await act(async () => {
      await store().previewImport()
    })
    disposeSettingsConfigArchive()
    expect(store().importFile).toBeNull()
    expect(store().importPassphrase).toBe('')
    expect(store().importPreview).toBeNull()
    expect(store().importApplied).toBeNull()
    expect(store().confirmOpen).toBe(false)
  })
})

describe('config archive import card', () => {
  beforeEach(() => {
    store().initializeIdentity(admin)
    api.readArchiveFile.mockImplementation((file: File) => file.arrayBuffer())
    api.previewConfigImportArchive
      .mockReset()
      .mockResolvedValue(previewResponse())
  })

  it('renders a file input, a password passphrase and disabled actions', async () => {
    renderSurface(<ConfigArchiveImportCard />)
    await flushMicrotasks()

    const input = document.getElementById(
      'settings-config-archive-file',
    ) as HTMLInputElement
    expect(input.getAttribute('type')).toBe('file')
    expect(input.getAttribute('accept')).toBe(CONFIG_ARCHIVE_FILE_ACCEPT)
    expect(
      (
        document.getElementById(
          'settings-config-archive-import-passphrase',
        ) as HTMLInputElement
      ).getAttribute('type'),
    ).toBe('password')
    expect(
      (
        document.getElementById(
          'settings-config-archive-preview',
        ) as HTMLButtonElement
      ).disabled,
    ).toBe(true)
    expect(
      (
        document.getElementById(
          'settings-config-archive-apply',
        ) as HTMLButtonElement
      ).disabled,
    ).toBe(true)
  })

  it('accepts the same file twice and never leaks the passphrase', async () => {
    renderSurface(<ConfigArchiveImportCard />)
    await flushMicrotasks()

    const input = document.getElementById(
      'settings-config-archive-file',
    ) as HTMLInputElement
    const file = archiveFile('backup.pfce')
    Object.defineProperty(input, 'files', { configurable: true, value: [file] })
    await act(async () => {
      fireEvent.change(input)
    })
    await flushMicrotasks()
    expect(input.value).toBe('')
    expect(api.readArchiveFile).toHaveBeenCalledTimes(1)

    Object.defineProperty(input, 'files', { configurable: true, value: [file] })
    await act(async () => {
      fireEvent.change(input)
    })
    await flushMicrotasks()
    expect(api.readArchiveFile).toHaveBeenCalledTimes(2)

    await act(async () => {
      fireEvent.change(
        document.getElementById('settings-config-archive-import-passphrase')!,
        { target: { value: PASSPHRASE } },
      )
    })
    const stored = Object.keys(localStorage).map((key) =>
      String(localStorage.getItem(key)),
    )
    expect(stored.join('\n')).not.toContain(PASSPHRASE)
    expect(document.body.textContent ?? '').not.toContain(PASSPHRASE)
    expect(document.body.textContent ?? '').not.toContain(
      'PROMPT_FERRY_SYNTHETIC_ARCHIVE',
    )
  })

  it('previews every field and warning through the composed tab', async () => {
    api.fetchConfigAuditPage.mockResolvedValue(auditPage([]))
    renderSurface(<SettingsConfigArchiveTab />)
    await flushMicrotasks()

    const input = document.getElementById(
      'settings-config-archive-file',
    ) as HTMLInputElement
    Object.defineProperty(input, 'files', {
      configurable: true,
      value: [archiveFile()],
    })
    await act(async () => {
      fireEvent.change(input)
    })
    await act(async () => {
      fireEvent.change(
        document.getElementById('settings-config-archive-import-passphrase')!,
        { target: { value: PASSPHRASE } },
      )
    })
    await act(async () => {
      fireEvent.click(
        document.getElementById('settings-config-archive-preview')!,
      )
    })
    await flushMicrotasks()

    const panel = document.getElementById(
      'settings-config-archive-preview-panel',
    )
    expect(panel).not.toBeNull()
    expect(panel?.textContent).toContain('postgresql')
    expect(panel?.textContent).toContain('abcdef0123456789')
    const endpointRow = document.querySelector(
      '[data-preview-domain="endpoints"]',
    )
    expect(endpointRow?.textContent).toContain('13')
    expect(endpointRow?.textContent).toContain('11')
    expect(endpointRow?.textContent).toContain('2')
    expect(
      document.getElementById('settings-config-archive-preview-warnings')
        ?.textContent,
    ).toContain('1 record(s) carry an unrecoverable secret')
    // A preview writes no audit row and sends no apply.
    expect(api.applyConfigImportArchive).not.toHaveBeenCalled()
    expect(api.fetchConfigAuditPage).toHaveBeenCalledTimes(1)
  })

  it('opens the destructive confirmation with zero writes and cancels cleanly', async () => {
    api.fetchConfigAuditPage.mockResolvedValue(auditPage([]))
    renderSurface(<SettingsConfigArchiveTab />)
    await flushMicrotasks()

    const input = document.getElementById(
      'settings-config-archive-file',
    ) as HTMLInputElement
    Object.defineProperty(input, 'files', {
      configurable: true,
      value: [archiveFile()],
    })
    await act(async () => {
      fireEvent.change(input)
    })
    await act(async () => {
      fireEvent.change(
        document.getElementById('settings-config-archive-import-passphrase')!,
        { target: { value: PASSPHRASE } },
      )
    })
    await act(async () => {
      fireEvent.click(
        document.getElementById('settings-config-archive-preview')!,
      )
    })
    await flushMicrotasks()
    await act(async () => {
      fireEvent.click(document.getElementById('settings-config-archive-apply')!)
    })
    await flushMicrotasks()

    const dialog = await screen.findByRole('dialog')
    expect(dialog.textContent).toContain(copy.configArchiveImportConfirmBody)
    expect(api.applyConfigImportArchive).not.toHaveBeenCalled()

    await act(async () => {
      fireEvent.click(
        document.getElementById('settings-config-archive-confirm-cancel')!,
      )
    })
    await flushMicrotasks()
    expect(api.applyConfigImportArchive).not.toHaveBeenCalled()
    expect(store().importFile).toBeNull()
    expect(store().importPassphrase).toBe('')
  })
})

describe('config archive confirm dialog', () => {
  it('disables both actions and blocks dismissal while the apply is in flight', async () => {
    store().initializeIdentity(admin)
    store().requestApply()
    // Simulate the pending apply state the store would hold.
    useSettingsConfigArchiveStore.setState({
      confirmOpen: true,
      importPending: true,
    })
    renderSurface(<ConfigArchiveImportConfirmDialog />)
    await flushMicrotasks()

    expect(
      (
        document.getElementById(
          'settings-config-archive-confirm-apply',
        ) as HTMLButtonElement
      ).disabled,
    ).toBe(true)
    expect(
      (
        document.getElementById(
          'settings-config-archive-confirm-cancel',
        ) as HTMLButtonElement
      ).disabled,
    ).toBe(true)
  })
})
