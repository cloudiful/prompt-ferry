import { defineStore } from 'pinia'
import { ref } from 'vue'
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
export function createConfigArchiveStore() {
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
    importPassphrase.value = ''
    importArchiveBase64.value = ''
    importFile.value = null
    importPreview.value = null
  }

  async function exportArchive(): Promise<void> {
    exporting.value = true
    try {
      const result = await exportConfigArchive(exportPassphrase.value)
      downloadConfigArchive(result.bytes)
      exportSummary.value = {
        backendKind: result.backendKind,
        formatVersion: result.formatVersion,
        payloadFingerprint: result.payloadFingerprint,
        size: result.bytes.byteLength,
      }
      await refreshConfigAudit()
    } finally {
      exportPassphrase.value = ''
      exporting.value = false
    }
  }

  async function selectImportFile(file: File): Promise<void> {
    importApplied.value = null
    importPreview.value = null
    const bytes = await readArchiveFile(file)
    importArchiveBase64.value = encodeArchiveBase64(bytes)
    importFile.value = { name: file.name, size: file.size }
  }

  async function previewImport(): Promise<void> {
    previewing.value = true
    try {
      importPreview.value = await previewConfigImportArchive(
        importArchiveBase64.value,
        importPassphrase.value,
      )
    } catch (cause) {
      // A rejected passphrase must not linger in memory.
      importPreview.value = null
      importPassphrase.value = ''
      throw cause
    } finally {
      previewing.value = false
    }
  }

  async function applyImport(): Promise<void> {
    applying.value = true
    try {
      importApplied.value = await applyConfigImportArchive(
        importArchiveBase64.value,
        importPassphrase.value,
      )
      await refreshConfigAudit()
    } finally {
      applying.value = false
      clearImportFlow()
    }
  }

  function cancelImport(): void {
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
