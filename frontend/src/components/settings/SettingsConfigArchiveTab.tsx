import { useEffect, useRef } from 'react'
import { ConfigArchiveAuditTable } from '@/components/settings/ConfigArchiveAuditTable'
import { ConfigArchiveExportCard } from '@/components/settings/ConfigArchiveExportCard'
import { ConfigArchiveImportCard } from '@/components/settings/ConfigArchiveImportCard'
import { ConfigArchiveImportConfirmDialog } from '@/components/settings/ConfigArchiveImportConfirmDialog'
import { ConfigArchiveImportPreview } from '@/components/settings/ConfigArchiveImportPreview'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import {
  disposeSettingsConfigArchive,
  setSettingsConfigArchiveNotifier,
  useSettingsConfigArchiveStore,
} from '@/lib/config-archive'

/**
 * The configuration archive tab: export, the import wizard and its audit trail.
 *
 * Mounting it binds the identity the reads and the guarded writes belong to and
 * reads the first audit page once; a preview never refreshes the trail on its
 * own. Unmounting clears both transient passphrases, the archive body, the file,
 * the preview, the applied summary and the audit rows, so nothing survives on
 * the module that outlives the tab.
 */
export function SettingsConfigArchiveTab() {
  const { t } = useLocale()
  const { notifyApiError, notifySuccess } = useNotifier()
  const me = useSessionStore((state) => state.me)
  const identityKey = useSettingsConfigArchiveStore(
    (state) => state.identityKey,
  )
  const initializeIdentity = useSettingsConfigArchiveStore(
    (state) => state.initializeIdentity,
  )
  const refreshAudit = useSettingsConfigArchiveStore(
    (state) => state.refreshAudit,
  )

  const exportDone = useRef<() => void>(() => {})
  exportDone.current = () => notifySuccess(t('configArchiveExportDone'))
  const importApplied = useRef<() => void>(() => {})
  importApplied.current = () => notifySuccess(t('configArchiveImportApplied'))

  useEffect(() => {
    setSettingsConfigArchiveNotifier({
      notifyApiError: (cause) => notifyApiError(cause),
      notifyExportDone: () => exportDone.current(),
      notifyImportApplied: () => importApplied.current(),
    })
    return () => {
      setSettingsConfigArchiveNotifier(null)
      disposeSettingsConfigArchive()
    }
  }, [notifyApiError, t])

  useEffect(() => {
    initializeIdentity(me)
  }, [me, initializeIdentity])

  const loadedIdentity = useRef('')
  useEffect(() => {
    if (identityKey === '' || loadedIdentity.current === identityKey) return
    loadedIdentity.current = identityKey
    void refreshAudit()
  }, [identityKey, refreshAudit])

  return (
    <section className="grid min-w-0 gap-3">
      <ConfigArchiveExportCard />
      <ConfigArchiveImportCard />
      <ConfigArchiveImportPreview />
      <ConfigArchiveAuditTable />
      <ConfigArchiveImportConfirmDialog />
    </section>
  )
}
