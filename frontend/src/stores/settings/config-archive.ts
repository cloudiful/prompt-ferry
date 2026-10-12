import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { useAsyncAction } from '../../composables/useAsyncAction'
import type { AsyncActionOutcome } from '../../composables/useAsyncAction'
import {
  applyConfigImportArchive,
  downloadConfigArchive,
  encodeArchiveBase64,
  exportConfigArchive,
  fetchConfigAuditPage,
  previewConfigImportArchive,
  readArchiveFile,
} from '../../api/config-archive'
import type { ConfigArchiveExportResult } from '../../api/config-archive'
import type {
  ConfigAuditEntry,
  ConfigImportApplied,
  ConfigImportPreview,
} from '../../generated/admin-api'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../../table-pagination'

export type ConfigArchiveImportFile = {
  name: string
  size: number
}

export type ConfigArchiveExportSummary = Pick<
  ConfigArchiveExportResult,
  'backendKind' | 'formatVersion' | 'payloadFingerprint'
> & { size: number }

/**
 * Transient administrator configuration archive state. Passphrases and the
 * base64 archive body live only in these in-memory refs and are cleared when
 * an operation ends; nothing here is persisted.
 */
const EXPORT_KEY = 'configArchive:export'
const IMPORT_KEY = 'configArchive:import'

export function createConfigArchiveStore() {
  const guard = useAsyncAction()
  const exportPending = computed(() => guard.isPending(EXPORT_KEY))
  const importPending = computed(() => guard.isPending(IMPORT_KEY))
  let importGeneration = 0
  const exportPassphrase = ref('')
  const exportSummary = ref<ConfigArchiveExportSummary | null>(null)
  const exporting = ref(false)

  const importPassphrase = ref('')
  const importFile = ref<ConfigArchiveImportFile | null>(null)
  const importArchiveBase64 = ref('')
  const importPreview = ref<ConfigImportPreview | null>(null)
  const importApplied = ref<ConfigImportApplied | null>(null)
  const previewing = ref(false)
  const applying = ref(false)

  const auditEntries = ref<ConfigAuditEntry[]>([])
  const auditFirst = ref(0)
  const auditRows = useStoredPageSize(
    'config-archive-audit',
    10,
    STANDARD_PAGE_SIZE_OPTIONS,
  )
  const auditTotal = ref(0)
  const auditLoading = ref(false)

  function clearImportFlow(): void {
    importGeneration += 1
    importPassphrase.value = ''
    importArchiveBase64.value = ''
    importFile.value = null
    importPreview.value = null
  }

  function exportArchive(): Promise<AsyncActionOutcome<void>> {
    const passphrase = exportPassphrase.value
    return guard.run(
      EXPORT_KEY,
      async () => {
        exporting.value = true
        try {
          const result = await exportConfigArchive(passphrase)
          downloadConfigArchive(result.bytes)
          exportSummary.value = {
            backendKind: result.backendKind,
            formatVersion: result.formatVersion,
            payloadFingerprint: result.payloadFingerprint,
            size: result.bytes.byteLength,
          }
          await refreshConfigAudit()
        } finally {
          if (exportPassphrase.value === passphrase) {
            exportPassphrase.value = ''
          }
          exporting.value = false
        }
      },
      'export',
    )
  }

  async function selectImportFile(file: File): Promise<void> {
    if (importPending.value) return
    const generation = ++importGeneration
    importApplied.value = null
    importPreview.value = null
    const bytes = await readArchiveFile(file)
    if (generation !== importGeneration || importPending.value) return
    importArchiveBase64.value = encodeArchiveBase64(bytes)
    importFile.value = { name: file.name, size: file.size }
  }

  function previewImport(): Promise<AsyncActionOutcome<void>> {
    const archive = importArchiveBase64.value
    const passphrase = importPassphrase.value
    const generation = importGeneration
    return guard.run(
      IMPORT_KEY,
      async () => {
        previewing.value = true
        importPreview.value = null
        try {
          const preview = await previewConfigImportArchive(archive, passphrase)
          if (
            generation === importGeneration &&
            archive === importArchiveBase64.value &&
            passphrase === importPassphrase.value
          ) {
            importPreview.value = preview
          }
        } catch (cause) {
          // A rejected passphrase must not linger, but keep the selected file for
          // a corrected retry and leave the import wizard in place.
          if (generation === importGeneration) {
            importPreview.value = null
            importPassphrase.value = ''
          }
          throw cause
        } finally {
          previewing.value = false
        }
      },
      'preview',
    )
  }

  function applyImport(): Promise<AsyncActionOutcome<ConfigImportApplied>> {
    const archive = importArchiveBase64.value
    const passphrase = importPassphrase.value
    const preview = importPreview.value
    const generation = importGeneration
    if (!preview) return Promise.resolve({ executed: false })
    return guard.run(
      IMPORT_KEY,
      async () => {
        applying.value = true
        try {
          const applied = await applyConfigImportArchive(archive, passphrase)
          await refreshConfigAudit()
          if (
            generation === importGeneration &&
            archive === importArchiveBase64.value &&
            preview === importPreview.value
          ) {
            importApplied.value = applied
            clearImportFlow()
          }
          return applied
        } finally {
          applying.value = false
        }
      },
      'apply',
    )
  }

  function cancelImport(): void {
    if (importPending.value) return
    clearImportFlow()
  }

  function resetConfigArchiveSecrets(): void {
    exportPassphrase.value = ''
    clearImportFlow()
  }

  async function refreshConfigAudit(
    first = auditFirst.value,
    rows = auditRows.value,
  ): Promise<void> {
    auditLoading.value = true
    try {
      const page = await fetchConfigAuditPage(first, rows)
      auditEntries.value = page.entries
      auditTotal.value = page.total
      auditFirst.value = page.first
      auditRows.value = page.rows
      if (
        auditEntries.value.length === 0 &&
        auditTotal.value > 0 &&
        auditFirst.value >= auditTotal.value
      ) {
        const previousFirst =
          Math.floor((auditTotal.value - 1) / auditRows.value) * auditRows.value
        await refreshConfigAudit(previousFirst, auditRows.value)
      }
    } finally {
      auditLoading.value = false
    }
  }

  return {
    applying,
    applyImport,
    exportPending,
    importPending,
    isPending: guard.isPending,
    pendingActions: guard.pendingActions,
    auditEntries,
    auditFirst,
    auditLoading,
    auditRows,
    auditTotal,
    cancelImport,
    clearImportFlow,
    exportArchive,
    exportPassphrase,
    exportSummary,
    exporting,
    importApplied,
    importArchiveBase64,
    importFile,
    importPassphrase,
    importPreview,
    previewImport,
    previewing,
    refreshConfigAudit,
    resetConfigArchiveSecrets,
    selectImportFile,
  }
}

/**
 * Standalone settings sub-store: the archive tab owns its transient passphrases
 * and archive bytes without widening the shared settings store.
 */
export const useConfigArchiveStore = defineStore(
  'config-archive',
  createConfigArchiveStore,
)
