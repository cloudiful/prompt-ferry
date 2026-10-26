import { useRef } from 'react'
import { FileUp, LoaderCircle, RefreshCw, TriangleAlert, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { SettingsCard } from '@/components/settings/SettingsCard'
import { useLocale } from '@/lib/i18n'
import {
  CONFIG_ARCHIVE_FILE_ACCEPT,
  CONFIG_ARCHIVE_IMPORT_KEY,
  configArchiveApplyEnabled,
  configArchiveFileSize,
  configArchiveImportRequestEnabled,
} from '@/lib/config-archive-model'
import { useSettingsConfigArchiveStore } from '@/lib/config-archive'
import { useAsyncAction } from '@/lib/use-async-action'

/**
 * The import card: choose one archive, type its passphrase, preview, then apply.
 *
 * The file input keeps nothing but the name and the size, and its DOM value is
 * cleared after every pick so choosing the same file again still fires a change
 * event. The passphrase is a secret in memory only: it is never written to
 * storage, a URL, a header, a filename, a log or a title, and the card never
 * renders it back. The archive body never leaves the store, so nothing here can
 * display it.
 */
export function ConfigArchiveImportCard() {
  const { t, locale } = useLocale()
  const inputRef = useRef<HTMLInputElement | null>(null)

  const file = useSettingsConfigArchiveStore((state) => state.importFile)
  const passphrase = useSettingsConfigArchiveStore(
    (state) => state.importPassphrase,
  )
  const preview = useSettingsConfigArchiveStore((state) => state.importPreview)
  const pending = useSettingsConfigArchiveStore((state) => state.importPending)
  const unavailable = useSettingsConfigArchiveStore(
    (state) => state.importUnavailable,
  )
  const unavailableMessage = useSettingsConfigArchiveStore(
    (state) => state.importUnavailableMessage,
  )
  const error = useSettingsConfigArchiveStore((state) => state.importError)
  const selectImportFile = useSettingsConfigArchiveStore(
    (state) => state.selectImportFile,
  )
  const setImportPassphrase = useSettingsConfigArchiveStore(
    (state) => state.setImportPassphrase,
  )
  const previewImport = useSettingsConfigArchiveStore(
    (state) => state.previewImport,
  )
  const requestApply = useSettingsConfigArchiveStore(
    (state) => state.requestApply,
  )
  const removeImportFile = useSettingsConfigArchiveStore(
    (state) => state.removeImportFile,
  )
  const clearImportUnavailable = useSettingsConfigArchiveStore(
    (state) => state.clearImportUnavailable,
  )
  const { isPending } = useAsyncAction()

  const busy = pending || isPending(CONFIG_ARCHIVE_IMPORT_KEY)
  const previewEnabled = configArchiveImportRequestEnabled({
    hasFile: file !== null,
    passphrase,
    pending: busy,
  })
  const applyEnabled = configArchiveApplyEnabled({
    hasPreview: preview !== null,
    pending: busy,
  })

  async function onPick(event: React.ChangeEvent<HTMLInputElement>) {
    const chosen = event.target.files?.[0]
    // Clear the native value first, so the same file can be chosen again and a
    // rejected pick never lingers in the control.
    event.target.value = ''
    if (!chosen) return
    await selectImportFile(chosen)
  }

  return (
    <SettingsCard icon={FileUp} title={t('configArchiveImportTitle')}>
      <div className="grid min-w-0 gap-3 sm:grid-cols-2">
        <div className="grid min-w-0 gap-1">
          <Label
            className="text-xs font-medium"
            htmlFor="settings-config-archive-file"
          >
            {t('configArchiveArchiveFile')}
          </Label>
          <div className="flex min-w-0 items-center gap-2">
            <Input
              accept={CONFIG_ARCHIVE_FILE_ACCEPT}
              className="h-8 min-w-0 text-sm"
              disabled={busy}
              id="settings-config-archive-file"
              name="settings-config-archive-file"
              onChange={(event) => void onPick(event)}
              ref={inputRef}
              type="file"
            />
            {file ? (
              <Button
                aria-label={t('configArchiveRemoveFile')}
                disabled={busy}
                id="settings-config-archive-remove-file"
                onClick={() => removeImportFile()}
                size="icon-sm"
                type="button"
                variant="outline"
              >
                <X aria-hidden="true" />
              </Button>
            ) : null}
          </div>
          {file ? (
            <p
              className="m-0 truncate text-[11px] text-muted-foreground"
              id="settings-config-archive-file-name"
              title={file.name}
            >
              {file.name} · {configArchiveFileSize(file.size, locale)}
            </p>
          ) : null}
        </div>

        <div className="grid min-w-0 gap-1">
          <Label
            className="text-xs font-medium"
            htmlFor="settings-config-archive-import-passphrase"
          >
            {t('configArchivePassphrase')}
          </Label>
          <Input
            autoComplete="new-password"
            className="h-8 text-sm"
            disabled={busy}
            id="settings-config-archive-import-passphrase"
            name="settings-config-archive-import-passphrase"
            onChange={(event) => setImportPassphrase(event.target.value)}
            placeholder={t('configArchivePassphrasePlaceholder')}
            type="password"
            value={passphrase}
          />
        </div>
      </div>

      <div className="flex min-w-0 flex-wrap items-center gap-2">
        {busy ? (
          <span aria-label={t('loading')} className="inline-flex" role="status">
            <LoaderCircle
              aria-hidden="true"
              className="size-3.5 animate-spin text-muted-foreground"
            />
          </span>
        ) : null}
        <Button
          disabled={!previewEnabled || unavailable}
          id="settings-config-archive-preview"
          onClick={() => void previewImport()}
          size="sm"
          type="button"
          variant="outline"
        >
          {t('configArchivePreviewAction')}
        </Button>
        <Button
          disabled={!applyEnabled || unavailable}
          id="settings-config-archive-apply"
          onClick={() => requestApply()}
          size="sm"
          type="button"
        >
          {t('configArchiveApplyAction')}
        </Button>
      </div>

      {unavailable ? (
        <p className="m-0 flex items-start gap-1.5 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-muted-foreground">
          <TriangleAlert
            aria-hidden="true"
            className="mt-0.5 size-3.5 shrink-0 text-warning"
          />
          <span className="min-w-0 break-words">
            {unavailableMessage
              ? `${t('error')}: ${unavailableMessage}`
              : t('error')}
          </span>
          <Button
            id="settings-config-archive-import-retry"
            onClick={() => clearImportUnavailable()}
            size="xs"
            type="button"
            variant="outline"
          >
            <RefreshCw aria-hidden="true" />
            {t('retry')}
          </Button>
        </p>
      ) : null}

      {!unavailable && error ? (
        <p
          className="m-0 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs leading-relaxed text-muted-foreground"
          id="settings-config-archive-import-error"
          role="alert"
        >
          {error}
        </p>
      ) : null}
    </SettingsCard>
  )
}
