import { useRef } from 'react'
import { useStore } from '@tanstack/react-form'
import { FileUp, LoaderCircle, RefreshCw, TriangleAlert, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { SettingsCard } from '@/components/settings/SettingsCard'
import { useLocale } from '@/lib/i18n'
import {
  CONFIG_ARCHIVE_FILE_ACCEPT,
  configArchiveFileSize,
  configArchiveImportRequestEnabled,
  type ConfigArchiveImportFile,
} from '@/lib/config-archive-model'
import type { ConfigArchiveImportForm } from '@/lib/config-archive'

type ConfigArchiveImportCardProps = {
  error: string
  file: ConfigArchiveImportFile | null
  form: ConfigArchiveImportForm
  hasPreview: boolean
  onClearUnavailable: () => void
  onPassphraseChange: () => void
  onRemoveFile: () => void
  onRequestApply: () => void
  onSelectFile: (file: File) => void
  pending: boolean
  unavailable: boolean
  unavailableMessage: string
}

/**
 * The import card: choose one archive, type its passphrase, preview, then apply.
 *
 * The file input keeps nothing but the name and the size, and its DOM value is
 * cleared after every pick so choosing the same file again still fires a change
 * event. The passphrase is a secret held by the native form in memory only: it
 * is never written to storage, a URL, a header, a filename, a log or a title,
 * and the card never renders it back. The archive body never leaves the tab,
 * so nothing here can display it.
 */
export function ConfigArchiveImportCard({
  error,
  file,
  form,
  hasPreview,
  onClearUnavailable,
  onPassphraseChange,
  onRemoveFile,
  onRequestApply,
  onSelectFile,
  pending,
  unavailable,
  unavailableMessage,
}: ConfigArchiveImportCardProps) {
  const { t, locale } = useLocale()
  const inputRef = useRef<HTMLInputElement | null>(null)
  const passphrase = useStore(form.store, (state) => state.values.passphrase)

  const previewEnabled = configArchiveImportRequestEnabled({
    hasFile: file !== null,
    passphrase,
    pending,
  })

  function onPick(event: React.ChangeEvent<HTMLInputElement>) {
    const chosen = event.target.files?.[0]
    // Clear the native value first, so the same file can be chosen again and a
    // rejected pick never lingers in the control.
    event.target.value = ''
    if (!chosen) return
    onSelectFile(chosen)
  }

  return (
    <SettingsCard icon={FileUp} title={t('configArchiveImportTitle')}>
      <form
        className="grid min-w-0 gap-3"
        noValidate
        onSubmit={(event) => {
          event.preventDefault()
          event.stopPropagation()
          if (pending || unavailable) return
          void form.handleSubmit()
        }}
      >
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
                disabled={pending}
                id="settings-config-archive-file"
                name="settings-config-archive-file"
                onChange={onPick}
                ref={inputRef}
                type="file"
              />
              {file ? (
                <Button
                  aria-label={t('configArchiveRemoveFile')}
                  disabled={pending}
                  id="settings-config-archive-remove-file"
                  onClick={onRemoveFile}
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
            <form.Field name="passphrase">
              {(field) => (
                <Input
                  autoComplete="new-password"
                  className="h-8 text-sm"
                  disabled={pending}
                  id="settings-config-archive-import-passphrase"
                  name="settings-config-archive-import-passphrase"
                  onChange={(event) => {
                    onPassphraseChange()
                    field.handleChange(event.target.value)
                  }}
                  placeholder={t('configArchivePassphrasePlaceholder')}
                  type="password"
                  value={field.state.value}
                />
              )}
            </form.Field>
          </div>
        </div>

        <div className="flex min-w-0 flex-wrap items-center gap-2">
          {pending ? (
            <span
              aria-label={t('loading')}
              className="inline-flex"
              role="status"
            >
              <LoaderCircle
                aria-hidden="true"
                className="size-3.5 animate-spin text-muted-foreground"
              />
            </span>
          ) : null}
          <Button
            disabled={!previewEnabled || unavailable}
            id="settings-config-archive-preview"
            size="sm"
            type="submit"
            variant="outline"
          >
            {t('configArchivePreviewAction')}
          </Button>
          <Button
            disabled={!hasPreview || pending || unavailable}
            id="settings-config-archive-apply"
            onClick={onRequestApply}
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
              onClick={onClearUnavailable}
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
      </form>
    </SettingsCard>
  )
}
