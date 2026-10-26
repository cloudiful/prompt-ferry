/**
 * The configuration archive's state: two transient passphrases, the transient
 * archive body, the non-secret export and import summaries, and one paged audit
 * trail.
 *
 * Every secret here is in memory only. A passphrase is cleared as soon as the
 * operation it belongs to settles — on success and on failure alike, exactly
 * like the original flow — unless the operator typed a newer one meanwhile; the
 * selected file keeps only its name and size while its base64 body lives beside
 * it and is dropped by `clearImportFlow`, an identity change and unmount.
 *
 * Three claims keep the operations honest. The export is one guarded write under
 * `configArchive:export` with the download and the audit refresh inside it. The
 * preview and the confirmed apply share `configArchive:import`, so a preview
 * cannot land inside an apply of the same archive, while the export stays
 * independent of both. The audit trail is a read with its own generation, so the
 * latest answer wins and a reset identity can never be overwritten by a read
 * that started before it.
 */

import { create } from 'zustand'
import type { ConfigAuditEntry, MeResponse } from '@/generated/admin-api'
import {
  applyConfigImportArchive,
  downloadConfigArchive,
  encodeArchiveBase64,
  exportConfigArchive,
  fetchConfigAuditPage,
  previewConfigImportArchive,
  readArchiveFile,
} from '@/api/config-archive'
import {
  getPendingActions,
  runAsyncAction,
  type AsyncActionOutcome,
} from './async-action'
import {
  CONFIG_ARCHIVE_AUDIT_DEFAULT_ROWS,
  CONFIG_ARCHIVE_EXPORT_KEY,
  CONFIG_ARCHIVE_EXPORT_ORIGIN,
  CONFIG_ARCHIVE_IMPORT_APPLY_ORIGIN,
  CONFIG_ARCHIVE_IMPORT_KEY,
  CONFIG_ARCHIVE_IMPORT_PREVIEW_ORIGIN,
  configArchiveApplied,
  configArchiveApplyEnabled,
  configArchiveCapability,
  configArchiveCapabilityMessage,
  configArchiveExportEnabled,
  configArchiveExportSummary,
  configArchiveImportRequestEnabled,
  configArchivePreview,
  configArchivePreviousPageFirst,
  type ConfigArchiveExportSummary,
  type ConfigArchiveImportApplied,
  type ConfigArchiveImportFile,
  type ConfigArchiveImportPreview,
} from './config-archive-model'

type ConfigArchiveIdentity = Pick<
  MeResponse,
  'user_id' | 'login_name' | 'is_admin'
>

export function configArchiveIdentityKey(
  identity: ConfigArchiveIdentity | null,
): string {
  if (!identity) return ''
  return `${identity.user_id}:${identity.login_name}:${identity.is_admin}`
}

export type SettingsConfigArchiveState = {
  identityKey: string
  exportPassphrase: string
  exportPending: boolean
  exportSummary: ConfigArchiveExportSummary | null
  exportFailed: boolean
  exportError: string
  exportUnavailable: boolean
  exportUnavailableMessage: string
  auditEntries: ConfigAuditEntry[]
  auditFirst: number
  auditRows: number
  auditTotal: number
  auditLoading: boolean
  auditFailed: boolean
  auditError: string
  auditUnavailable: boolean
  auditUnavailableMessage: string
  importFile: ConfigArchiveImportFile | null
  importPassphrase: string
  importPending: boolean
  importFailed: boolean
  importError: string
  importUnavailable: boolean
  importUnavailableMessage: string
  importPreview: ConfigArchiveImportPreview | null
  importApplied: ConfigArchiveImportApplied | null
  confirmOpen: boolean
  initializeIdentity: (identity: ConfigArchiveIdentity | null) => void
  resetIdentity: (identity: ConfigArchiveIdentity | null) => void
  setExportPassphrase: (value: string) => void
  exportArchive: (
    download?: (bytes: ArrayBuffer) => void,
  ) => Promise<AsyncActionOutcome<void>>
  setAuditRows: (rows: number) => void
  refreshAudit: (first?: number, rows?: number) => Promise<void>
  selectImportFile: (file: File) => Promise<void>
  setImportPassphrase: (value: string) => void
  previewImport: () => Promise<AsyncActionOutcome<void>>
  requestApply: () => void
  cancelApply: () => void
  confirmApply: () => Promise<AsyncActionOutcome<void>>
  clearImportFlow: () => void
  removeImportFile: () => void
  clearExportUnavailable: () => void
  clearImportUnavailable: () => void
}

let auditGeneration = 0
let importGeneration = 0

/**
 * The selected archive's base64 body. It lives here, outside the observable
 * state, so no subscriber — and no devtools snapshot — can read the archive
 * contents; only the store's own preview and apply calls use it.
 */
let importArchiveBase64 = ''

/** Whether an import operation (preview or apply) holds the shared claim. */
function importBusy(): boolean {
  return Object.hasOwn(getPendingActions(), CONFIG_ARCHIVE_IMPORT_KEY)
}

let reportedNotifier: {
  notifyApiError: (cause: unknown) => void
  notifyExportDone: () => void
  notifyImportApplied: () => void
} | null = null

/**
 * Register the notifications the tab reports through, so the store never reaches
 * into a React context and the tab owns the notification surface.
 */
export function setSettingsConfigArchiveNotifier(
  notifier: {
    notifyApiError: (cause: unknown) => void
    notifyExportDone: () => void
    notifyImportApplied: () => void
  } | null,
): void {
  reportedNotifier = notifier
}

const EMPTY_AUDIT = {
  auditEntries: [] as ConfigAuditEntry[],
  auditFailed: false,
  auditError: '',
  auditFirst: 0,
  auditLoading: false,
  auditTotal: 0,
  auditUnavailable: false,
  auditUnavailableMessage: '',
}

const EMPTY_EXPORT = {
  exportError: '',
  exportFailed: false,
  exportPassphrase: '',
  exportPending: false,
  exportSummary: null as ConfigArchiveExportSummary | null,
  exportUnavailable: false,
  exportUnavailableMessage: '',
}

/**
 * The import wizard's transient state. `importApplied` is deliberately absent:
 * it is a non-secret result that survives the flow it came from.
 */
const EMPTY_IMPORT = {
  confirmOpen: false,
  importError: '',
  importFailed: false,
  importFile: null as ConfigArchiveImportFile | null,
  importPassphrase: '',
  importPending: false,
  importPreview: null as ConfigArchiveImportPreview | null,
  importUnavailable: false,
  importUnavailableMessage: '',
}

/**
 * Drop the tab: nothing in flight may commit, and every secret and every byte
 * reference leaves with it. The audit rows go too — they belong to the identity
 * that read them.
 */
export function disposeSettingsConfigArchive(): void {
  ++auditGeneration
  ++importGeneration
  importArchiveBase64 = ''
  useSettingsConfigArchiveStore.setState({
    ...EMPTY_EXPORT,
    ...EMPTY_AUDIT,
    ...EMPTY_IMPORT,
    importApplied: null,
  })
}

export const useSettingsConfigArchiveStore = create<SettingsConfigArchiveState>(
  (set, get) => ({
    identityKey: '',
    ...EMPTY_EXPORT,
    ...EMPTY_AUDIT,
    ...EMPTY_IMPORT,
    importApplied: null,
    auditRows: CONFIG_ARCHIVE_AUDIT_DEFAULT_ROWS,

    initializeIdentity(identity) {
      const key = configArchiveIdentityKey(identity)
      if (get().identityKey === key) return
      get().resetIdentity(identity)
    },

    resetIdentity(identity) {
      ++auditGeneration
      ++importGeneration
      importArchiveBase64 = ''
      set({
        identityKey: configArchiveIdentityKey(identity),
        ...EMPTY_EXPORT,
        ...EMPTY_AUDIT,
        ...EMPTY_IMPORT,
        importApplied: null,
        auditRows: get().auditRows,
      })
    },

    setExportPassphrase(value) {
      set({ exportPassphrase: value })
    },

    setAuditRows(rows) {
      set({ auditRows: rows })
    },

    async exportArchive(download = downloadConfigArchive) {
      const state = get()
      // Without an identity there is no operator to audit, and an empty
      // passphrase is disabled in the UI; neither may reach the server.
      if (state.identityKey === '') return { executed: false }
      const passphrase = state.exportPassphrase
      if (!configArchiveExportEnabled({ passphrase, pending: false })) {
        return { executed: false }
      }
      const identityKey = state.identityKey
      const currentIdentity = () => get().identityKey === identityKey

      set({ exportPending: true, exportFailed: false, exportError: '' })
      try {
        const outcome = await runAsyncAction(
          CONFIG_ARCHIVE_EXPORT_KEY,
          async () => {
            const result = await exportConfigArchive(passphrase)
            if (!currentIdentity()) return
            // The bytes are handed to the fixed-name download and dropped: only
            // the summary metadata stays in state.
            download(result.bytes)
            set({
              exportSummary: configArchiveExportSummary(result),
              exportFailed: false,
              exportError: '',
              exportUnavailable: false,
              exportUnavailableMessage: '',
            })
            // Inside the claim, so the trail the operator sees already carries
            // this export and a second click cannot race it.
            await get().refreshAudit()
          },
          CONFIG_ARCHIVE_EXPORT_ORIGIN,
        )
        if (outcome.executed) {
          if (currentIdentity()) reportedNotifier?.notifyExportDone()
          return outcome
        }
        return outcome
      } catch (cause) {
        if (currentIdentity()) {
          const capability = configArchiveCapability(cause)
          set({
            exportFailed: true,
            exportError: formatCause(cause),
            exportUnavailable: capability === 'export',
            exportUnavailableMessage:
              capability === 'export'
                ? configArchiveCapabilityMessage(cause)
                : '',
          })
          reportedNotifier?.notifyApiError(cause)
        }
        return { executed: false }
      } finally {
        if (currentIdentity()) {
          set({ exportPending: false })
          // The passphrase this attempt consumed is cleared on success and on
          // failure alike; a newer one typed during the write stays.
          if (get().exportPassphrase === passphrase) {
            set({ exportPassphrase: '' })
          }
        }
      }
    },

    clearExportUnavailable() {
      // The capability answer is a state, not a permanent gate: clearing it lets
      // the operator retry the same action against a backend that recovered.
      set({
        exportError: '',
        exportFailed: false,
        exportUnavailable: false,
        exportUnavailableMessage: '',
      })
    },

    clearImportUnavailable() {
      set({
        importError: '',
        importFailed: false,
        importUnavailable: false,
        importUnavailableMessage: '',
      })
    },

    clearImportFlow() {
      // Bumping the generation is what makes a superseded preview or apply
      // unable to commit; the applied summary is a result and stays.
      ++importGeneration
      importArchiveBase64 = ''
      set({ ...EMPTY_IMPORT })
    },

    removeImportFile() {
      if (importBusy()) return
      get().clearImportFlow()
    },

    async selectImportFile(file) {
      const state = get()
      if (state.identityKey === '') return
      // A pick supersedes whatever the wizard held: the generation moves, so an
      // answer belonging to the previous archive can never commit, and a read
      // that finishes during an import is dropped instead of replacing its body.
      const identityKey = state.identityKey
      const generation = ++importGeneration
      importArchiveBase64 = ''
      set({
        ...EMPTY_IMPORT,
        importApplied: null,
        importFile: null,
      })
      // Read first, commit only for this generation: a newer pick, an identity
      // change or a started import drops this answer.
      const bytes = await readArchiveFile(file)
      if (generation !== importGeneration) return
      if (get().identityKey !== identityKey) return
      if (importBusy()) return
      importArchiveBase64 = encodeArchiveBase64(bytes)
      set({ importFile: { name: file.name, size: file.size } })
    },

    setImportPassphrase(value) {
      // Changing the passphrase invalidates a preview produced with the old one,
      // and with it any confirmation the operator had opened.
      set({
        confirmOpen: false,
        importApplied: null,
        importError: '',
        importFailed: false,
        importPassphrase: value,
        importPreview: null,
      })
    },

    async previewImport() {
      const state = get()
      const file = state.importFile
      const passphrase = state.importPassphrase
      if (
        state.identityKey === '' ||
        file === null ||
        importArchiveBase64 === ''
      ) {
        return { executed: false }
      }
      if (
        !configArchiveImportRequestEnabled({
          hasFile: true,
          passphrase,
          pending: false,
        })
      ) {
        return { executed: false }
      }
      const identityKey = state.identityKey
      const generation = importGeneration
      const archive = importArchiveBase64
      set({
        confirmOpen: false,
        importApplied: null,
        importError: '',
        importFailed: false,
        importPending: true,
        importPreview: null,
        importUnavailable: false,
        importUnavailableMessage: '',
      })

      try {
        const outcome = await runAsyncAction(
          CONFIG_ARCHIVE_IMPORT_KEY,
          async () => {
            const preview = await previewConfigImportArchive(
              archive,
              passphrase,
            )
            if (get().identityKey !== identityKey) return
            if (generation !== importGeneration) return
            if (get().importPassphrase !== passphrase) return
            set({ importPreview: configArchivePreview(preview) })
          },
          CONFIG_ARCHIVE_IMPORT_PREVIEW_ORIGIN,
        )
        return outcome
      } catch (cause) {
        if (get().identityKey === identityKey) {
          const capability = configArchiveCapability(cause)
          set({
            importError: formatCause(cause),
            importFailed: true,
            importPreview: null,
            importUnavailable: capability === 'import',
            importUnavailableMessage:
              capability === 'import'
                ? configArchiveCapabilityMessage(cause)
                : '',
          })
          // A rejected passphrase must not linger, but the file stays so the
          // operator can correct it and retry; a newer one typed meanwhile is a
          // newer edit and survives.
          if (get().importPassphrase === passphrase) {
            set({ importPassphrase: '' })
          }
          reportedNotifier?.notifyApiError(cause)
        }
        return { executed: false }
      } finally {
        if (get().identityKey === identityKey) {
          set({ importPending: false })
        }
      }
    },

    requestApply() {
      const state = get()
      if (importArchiveBase64 === '') return
      if (
        !configArchiveApplyEnabled({
          hasPreview: state.importPreview !== null,
          pending: importBusy(),
        })
      ) {
        return
      }
      // Opening the confirmation is a local action: zero requests.
      set({ confirmOpen: true })
    },

    cancelApply() {
      if (importBusy()) return
      // Cancel writes nothing and leaves no wizard behind.
      set({ confirmOpen: false })
      get().clearImportFlow()
    },

    async confirmApply() {
      const state = get()
      const preview = state.importPreview
      const passphrase = state.importPassphrase
      if (state.identityKey === '' || preview === null || passphrase === '') {
        return { executed: false }
      }
      const archive = importArchiveBase64
      if (archive === '') return { executed: false }
      const identityKey = state.identityKey
      const generation = importGeneration
      set({
        importError: '',
        importFailed: false,
        importPending: true,
        importUnavailable: false,
        importUnavailableMessage: '',
      })

      try {
        const outcome = await runAsyncAction(
          CONFIG_ARCHIVE_IMPORT_KEY,
          async () => {
            // The same body the preview sent, held for the whole claim.
            const applied = await applyConfigImportArchive(archive, passphrase)
            if (get().identityKey !== identityKey) return
            // Inside the claim: the trail already carries this import.
            await get().refreshAudit()
            if (get().identityKey !== identityKey) return
            if (generation !== importGeneration) return
            if (get().importPreview !== preview) return
            set({ importApplied: configArchiveApplied(applied) })
            get().clearImportFlow()
            reportedNotifier?.notifyImportApplied()
          },
          CONFIG_ARCHIVE_IMPORT_APPLY_ORIGIN,
        )
        return outcome
      } catch (cause) {
        if (get().identityKey === identityKey) {
          // A failed apply keeps the confirmed wizard, the file, the body, the
          // passphrase and the preview, so Save can deliberately retry it.
          const capability = configArchiveCapability(cause)
          set({
            importError: formatCause(cause),
            importFailed: true,
            importUnavailable: capability === 'import',
            importUnavailableMessage:
              capability === 'import'
                ? configArchiveCapabilityMessage(cause)
                : '',
          })
          reportedNotifier?.notifyApiError(cause)
        }
        return { executed: false }
      } finally {
        if (get().identityKey === identityKey) {
          set({ importPending: false })
        }
      }
    },

    async refreshAudit(first = get().auditFirst, rows = get().auditRows) {
      const identityKey = get().identityKey
      if (identityKey === '') return
      const generation = ++auditGeneration
      set({ auditLoading: true, auditFailed: false, auditError: '' })
      try {
        let requestedFirst = first
        let page = await fetchConfigAuditPage(requestedFirst, rows)
        if (generation !== auditGeneration) return
        if (get().identityKey !== identityKey) return
        // An empty window at or past the total is not an empty trail: read the
        // last valid page once instead of showing nothing.
        const previous = configArchivePreviousPageFirst(
          page.first,
          page.rows,
          page.total,
          page.entries.length,
        )
        if (previous !== null) {
          requestedFirst = previous
          page = await fetchConfigAuditPage(previous, rows)
          if (generation !== auditGeneration) return
          if (get().identityKey !== identityKey) return
        }
        set({
          auditEntries: page.entries,
          auditFailed: false,
          auditError: '',
          auditFirst: page.first,
          auditLoading: false,
          auditRows: page.rows,
          auditTotal: page.total,
          auditUnavailable: false,
          auditUnavailableMessage: '',
        })
      } catch (cause) {
        if (generation !== auditGeneration) return
        if (get().identityKey !== identityKey) return
        const capability = configArchiveCapability(cause)
        set({
          auditFailed: true,
          auditError: formatCause(cause),
          auditLoading: false,
          auditUnavailable: capability === 'audit',
          auditUnavailableMessage:
            capability === 'audit' ? configArchiveCapabilityMessage(cause) : '',
        })
      }
    },
  }),
)

/** An error's own code and message, without any request payload. */
function formatCause(cause: unknown): string {
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
  if (cause instanceof Error) return cause.message
  return ''
}
