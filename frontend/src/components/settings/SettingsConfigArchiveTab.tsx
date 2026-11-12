import { useEffect, useRef, useState } from 'react'
import { useForm } from '@tanstack/react-form'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ConfigArchiveAuditTable } from '@/components/settings/ConfigArchiveAuditTable'
import { ConfigArchiveExportCard } from '@/components/settings/ConfigArchiveExportCard'
import { ConfigArchiveImportCard } from '@/components/settings/ConfigArchiveImportCard'
import { ConfigArchiveImportConfirmDialog } from '@/components/settings/ConfigArchiveImportConfirmDialog'
import { ConfigArchiveImportPreview } from '@/components/settings/ConfigArchiveImportPreview'
import { useStoredPageSize } from '@/components/shared/TablePagination'
import {
  buildImportRequest,
  downloadConfigArchive,
  encodeArchiveBase64,
  exportConfigArchive,
  readArchiveFile,
} from '@/api/config-archive'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import {
  configArchiveAuditOptions,
  configArchiveIdentityKey,
  refreshConfigArchiveAudit,
  type ConfigArchiveExportForm,
  type ConfigArchiveImportForm,
} from '@/lib/config-archive'
import {
  CONFIG_ARCHIVE_AUDIT_DEFAULT_ROWS,
  CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_OPTIONS,
  CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_SCOPE,
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
  type ConfigArchiveImportPreview as ConfigArchiveImportPreviewState,
} from '@/lib/config-archive-model'
import {
  importConfigMutation,
  previewConfigImportMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { formatEnvelopeCause } from '@/lib/settings-refresh'
import { useAsyncAction } from '@/lib/use-async-action'
import type { TablePageChange } from '@/models/pagination'

function formatCause(cause: unknown): string {
  return (
    formatEnvelopeCause(cause) ?? (cause instanceof Error ? cause.message : '')
  )
}

/**
 * The configuration archive tab: export, the import wizard and its audit trail.
 *
 * The audit trail is server data read through the generated query key. The two
 * passphrases, the selected file and its base64 body, the preview, the applied
 * summary and the confirmation live only in this component and its forms:
 * unmounting drops every secret and every byte reference. Preview and apply
 * share one resource key, so one cannot land inside the other, while the export
 * keeps its own key.
 */
export function SettingsConfigArchiveTab() {
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const { notifyApiError, notifySuccess } = useNotifier()
  const asyncAction = useAsyncAction()
  const me = useSessionStore((state) => state.me)
  const identityKey = configArchiveIdentityKey(me)
  const [auditFirst, setAuditFirst] = useState(0)
  const [auditRows, setAuditRows] = useStoredPageSize(
    CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_SCOPE,
    CONFIG_ARCHIVE_AUDIT_DEFAULT_ROWS,
    CONFIG_ARCHIVE_AUDIT_PAGE_SIZE_OPTIONS,
  )
  const [exportSummary, setExportSummary] =
    useState<ConfigArchiveExportSummary | null>(null)
  const [exportPending, setExportPending] = useState(false)
  const [exportFailed, setExportFailed] = useState(false)
  const [exportError, setExportError] = useState('')
  const [exportUnavailable, setExportUnavailable] = useState(false)
  const [exportUnavailableMessage, setExportUnavailableMessage] = useState('')
  const [importFile, setImportFile] = useState<ConfigArchiveImportFile | null>(
    null,
  )
  const [importPreview, setImportPreview] =
    useState<ConfigArchiveImportPreviewState | null>(null)
  const [importApplied, setImportApplied] =
    useState<ConfigArchiveImportApplied | null>(null)
  const [importPending, setImportPending] = useState(false)
  const [importFailed, setImportFailed] = useState(false)
  const [importError, setImportError] = useState('')
  const [importUnavailable, setImportUnavailable] = useState(false)
  const [importUnavailableMessage, setImportUnavailableMessage] = useState('')
  const [confirmOpen, setConfirmOpen] = useState(false)
  const archiveRef = useRef('')
  const importGeneration = useRef(0)
  const disposed = useRef(false)
  const notifiedErrors = useRef(new WeakSet<object>())
  const auditFirstRef = useRef(auditFirst)
  const auditRowsRef = useRef(auditRows)
  const previewRef = useRef(importPreview)
  const initializedIdentity = useRef('')

  auditFirstRef.current = auditFirst
  auditRowsRef.current = auditRows
  previewRef.current = importPreview

  const exportForm = useForm({
    defaultValues: { passphrase: '' },
    onSubmit: async ({ value }) => {
      await runExport(value.passphrase)
    },
  })
  const importForm = useForm({
    defaultValues: { passphrase: '' },
    onSubmit: async ({ value }) => {
      await runImportPreview(value.passphrase)
    },
  })

  const auditQuery = useQuery({
    ...configArchiveAuditOptions(auditFirst, auditRows),
    enabled: identityKey !== '',
    gcTime: 0,
  })
  const exportMutation = useMutation({
    mutationFn: (passphrase: string) => exportConfigArchive(passphrase),
  })
  const previewImportMutation = useMutation(previewConfigImportMutation())
  const applyImportMutation = useMutation(importConfigMutation())

  const importBusy = asyncAction.isPending(CONFIG_ARCHIVE_IMPORT_KEY)
  const exportGuardPending = asyncAction.isPending(CONFIG_ARCHIVE_EXPORT_KEY)
  const exporting = exportPending || exportGuardPending
  const importing = importPending || importBusy
  const auditEntries = auditQuery.data?.entries ?? []
  const auditTotal = auditQuery.data?.total ?? 0
  const auditLoading = auditQuery.isFetching
  const auditFailed = auditQuery.isError
  const auditError = auditQuery.error ? formatCause(auditQuery.error) : ''
  const auditCapability = configArchiveCapability(auditQuery.error)
  const auditUnavailable = auditCapability === 'audit'
  const auditUnavailableMessage =
    auditCapability === 'audit' && auditQuery.error
      ? configArchiveCapabilityMessage(auditQuery.error)
      : ''

  function reportApiError(cause: unknown): void {
    if (cause && typeof cause === 'object') {
      if (notifiedErrors.current.has(cause)) return
      notifiedErrors.current.add(cause)
    }
    notifyApiError(cause)
  }

  useEffect(() => {
    disposed.current = false
    return () => {
      disposed.current = true
      archiveRef.current = ''
    }
  }, [])

  useEffect(() => {
    if (auditQuery.error) reportApiError(auditQuery.error)
  }, [auditQuery.error])

  useEffect(() => {
    if (identityKey === '') {
      initializedIdentity.current = ''
      return
    }
    if (initializedIdentity.current === identityKey) return
    initializedIdentity.current = identityKey
    resetArchiveState()
  }, [identityKey])

  useEffect(() => {
    const page = auditQuery.data
    if (!page) return
    const previous = configArchivePreviousPageFirst(
      page.first,
      page.rows,
      page.total,
      page.entries.length,
    )
    if (previous !== null && previous !== auditFirstRef.current) {
      setAuditFirst(previous)
    }
  }, [auditQuery.data])

  function resetArchiveState(): void {
    ++importGeneration.current
    archiveRef.current = ''
    exportForm.reset()
    importForm.reset()
    setAuditFirst(0)
    setExportSummary(null)
    setExportPending(false)
    setExportFailed(false)
    setExportError('')
    setExportUnavailable(false)
    setExportUnavailableMessage('')
    setImportFile(null)
    setImportPreview(null)
    setImportApplied(null)
    setImportPending(false)
    setImportFailed(false)
    setImportError('')
    setImportUnavailable(false)
    setImportUnavailableMessage('')
    setConfirmOpen(false)
  }

  async function refreshAudit(
    first = auditFirstRef.current,
    rows = auditRowsRef.current,
  ) {
    const page = await refreshConfigArchiveAudit(queryClient, first, rows)
    if (disposed.current) return page
    if (page.first !== auditFirstRef.current) {
      auditFirstRef.current = page.first
      setAuditFirst(page.first)
    }
    if (page.rows > 0 && page.rows !== auditRowsRef.current) {
      auditRowsRef.current = page.rows
      setAuditRows(page.rows)
    }
    return page
  }

  async function refreshAuditFromTable(): Promise<void> {
    try {
      await refreshAudit()
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function runExport(passphrase: string): Promise<void> {
    if (identityKey === '') return
    if (!configArchiveExportEnabled({ passphrase, pending: exporting })) return
    setExportPending(true)
    setExportFailed(false)
    setExportError('')
    try {
      const outcome = await asyncAction.run(
        CONFIG_ARCHIVE_EXPORT_KEY,
        async () => {
          const result = await exportMutation.mutateAsync(passphrase)
          downloadConfigArchive(result.bytes)
          setExportSummary(configArchiveExportSummary(result))
          setExportFailed(false)
          setExportError('')
          setExportUnavailable(false)
          setExportUnavailableMessage('')
          try {
            await refreshAudit()
          } catch {
            // The archive was already produced; a failed trail read must not
            // turn it into a failed export.
          }
        },
        CONFIG_ARCHIVE_EXPORT_ORIGIN,
      )
      if (outcome.executed) notifySuccess(t('configArchiveExportDone'))
    } catch (cause) {
      const capability = configArchiveCapability(cause)
      setExportFailed(true)
      setExportError(formatCause(cause))
      setExportUnavailable(capability === 'export')
      setExportUnavailableMessage(
        capability === 'export' ? configArchiveCapabilityMessage(cause) : '',
      )
      reportApiError(cause)
    } finally {
      setExportPending(false)
      if (exportForm.getFieldValue('passphrase') === passphrase) {
        exportForm.setFieldValue('passphrase', '')
      }
    }
  }

  async function selectImportFile(file: File): Promise<void> {
    if (identityKey === '') return
    const generation = ++importGeneration.current
    archiveRef.current = ''
    setImportFile(null)
    setImportPreview(null)
    setImportApplied(null)
    setConfirmOpen(false)
    setImportError('')
    setImportFailed(false)
    setImportUnavailable(false)
    setImportUnavailableMessage('')
    importForm.setFieldValue('passphrase', '')
    const bytes = await readArchiveFile(file)
    if (disposed.current || generation !== importGeneration.current) return
    if (asyncAction.isPending(CONFIG_ARCHIVE_IMPORT_KEY)) return
    archiveRef.current = encodeArchiveBase64(bytes)
    setImportFile({ name: file.name, size: file.size })
  }

  function clearImportFlow(): void {
    ++importGeneration.current
    archiveRef.current = ''
    importForm.setFieldValue('passphrase', '')
    setImportFile(null)
    setImportPreview(null)
    setConfirmOpen(false)
    setImportError('')
    setImportFailed(false)
    setImportUnavailable(false)
    setImportUnavailableMessage('')
  }

  function removeImportFile(): void {
    if (importing) return
    clearImportFlow()
  }

  function changeImportPassphrase(): void {
    setConfirmOpen(false)
    setImportApplied(null)
    setImportError('')
    setImportFailed(false)
    setImportPreview(null)
  }

  async function runImportPreview(passphrase: string): Promise<void> {
    const archive = archiveRef.current
    if (
      identityKey === '' ||
      importFile === null ||
      archive === '' ||
      !configArchiveImportRequestEnabled({
        hasFile: true,
        passphrase,
        pending: importing,
      })
    ) {
      return
    }
    const generation = importGeneration.current
    setConfirmOpen(false)
    setImportApplied(null)
    setImportError('')
    setImportFailed(false)
    setImportPending(true)
    setImportPreview(null)
    setImportUnavailable(false)
    setImportUnavailableMessage('')
    try {
      await asyncAction.run(
        CONFIG_ARCHIVE_IMPORT_KEY,
        async () => {
          const preview = await previewImportMutation.mutateAsync({
            body: buildImportRequest(archive, passphrase),
          })
          if (disposed.current) return
          if (generation !== importGeneration.current) return
          if (importForm.getFieldValue('passphrase') !== passphrase) return
          setImportPreview(configArchivePreview(preview))
        },
        CONFIG_ARCHIVE_IMPORT_PREVIEW_ORIGIN,
      )
    } catch (cause) {
      if (!disposed.current) {
        const capability = configArchiveCapability(cause)
        setImportError(formatCause(cause))
        setImportFailed(true)
        setImportPreview(null)
        setImportUnavailable(capability === 'import')
        setImportUnavailableMessage(
          capability === 'import' ? configArchiveCapabilityMessage(cause) : '',
        )
        if (importForm.getFieldValue('passphrase') === passphrase) {
          importForm.setFieldValue('passphrase', '')
        }
        reportApiError(cause)
      }
    } finally {
      if (!disposed.current) setImportPending(false)
    }
  }

  function requestApply(): void {
    if (archiveRef.current === '') return
    if (
      !configArchiveApplyEnabled({
        hasPreview: importPreview !== null,
        pending: importing,
      })
    ) {
      return
    }
    setConfirmOpen(true)
  }

  function cancelApply(): void {
    if (importing) return
    setConfirmOpen(false)
    clearImportFlow()
  }

  async function confirmApply(): Promise<void> {
    const preview = importPreview
    const passphrase = importForm.getFieldValue('passphrase')
    const archive = archiveRef.current
    if (
      identityKey === '' ||
      preview === null ||
      passphrase === '' ||
      archive === ''
    ) {
      return
    }
    const generation = importGeneration.current
    setImportError('')
    setImportFailed(false)
    setImportPending(true)
    setImportUnavailable(false)
    setImportUnavailableMessage('')
    try {
      await asyncAction.run(
        CONFIG_ARCHIVE_IMPORT_KEY,
        async () => {
          const applied = await applyImportMutation.mutateAsync({
            body: buildImportRequest(archive, passphrase),
          })
          if (disposed.current) return
          try {
            await refreshAudit()
          } catch {
            // The import already landed; the trail read retries on its own.
          }
          if (disposed.current) return
          if (generation !== importGeneration.current) return
          if (previewRef.current !== preview) return
          setImportApplied(configArchiveApplied(applied))
          clearImportFlow()
          notifySuccess(t('configArchiveImportApplied'))
        },
        CONFIG_ARCHIVE_IMPORT_APPLY_ORIGIN,
      )
    } catch (cause) {
      if (!disposed.current) {
        const capability = configArchiveCapability(cause)
        setImportError(formatCause(cause))
        setImportFailed(true)
        setImportUnavailable(capability === 'import')
        setImportUnavailableMessage(
          capability === 'import' ? configArchiveCapabilityMessage(cause) : '',
        )
        reportApiError(cause)
      }
    } finally {
      if (!disposed.current) setImportPending(false)
    }
  }

  function changeAuditPage(change: TablePageChange): void {
    auditFirstRef.current = change.first
    auditRowsRef.current = change.rows
    setAuditFirst(change.first)
    setAuditRows(change.rows)
  }

  return (
    <section className="grid min-w-0 gap-3">
      <ConfigArchiveExportCard
        error={exportError}
        form={exportForm as ConfigArchiveExportForm}
        onClearUnavailable={() => {
          setExportError('')
          setExportFailed(false)
          setExportUnavailable(false)
          setExportUnavailableMessage('')
        }}
        pending={exporting}
        summary={exportSummary}
        unavailable={exportUnavailable}
        unavailableMessage={exportUnavailableMessage}
      />
      <ConfigArchiveImportCard
        error={importError}
        file={importFile}
        form={importForm as ConfigArchiveImportForm}
        onClearUnavailable={() => {
          setImportError('')
          setImportFailed(false)
          setImportUnavailable(false)
          setImportUnavailableMessage('')
        }}
        onPassphraseChange={changeImportPassphrase}
        onRemoveFile={removeImportFile}
        onRequestApply={requestApply}
        onSelectFile={(file) => void selectImportFile(file)}
        pending={importing}
        hasPreview={importPreview !== null}
        unavailable={importUnavailable}
        unavailableMessage={importUnavailableMessage}
      />
      <ConfigArchiveImportPreview
        applied={importApplied}
        preview={importPreview}
      />
      <ConfigArchiveAuditTable
        entries={auditEntries}
        error={auditError}
        failed={auditFailed}
        first={auditFirst}
        loading={auditLoading}
        onPage={changeAuditPage}
        onRefresh={() => void refreshAuditFromTable()}
        rows={auditRows}
        total={auditTotal}
        unavailable={auditUnavailable}
        unavailableMessage={auditUnavailableMessage}
      />
      <ConfigArchiveImportConfirmDialog
        error={importError}
        onApply={() => void confirmApply()}
        onCancel={cancelApply}
        open={confirmOpen}
        pending={importing}
        unavailable={importUnavailable}
      />
    </section>
  )
}
