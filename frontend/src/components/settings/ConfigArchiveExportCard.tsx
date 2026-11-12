import { useStore } from '@tanstack/react-form'
import { Button } from '@/components/ui/button'
import { LoaderCircle, RefreshCw } from 'lucide-react'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { SettingsCard } from '@/components/settings/SettingsCard'
import { Download, TriangleAlert } from 'lucide-react'
import { useLocale } from '@/lib/i18n'
import {
  configArchiveExportEnabled,
  formatAuditCount,
  shortFingerprint,
  type ConfigArchiveExportSummary,
} from '@/lib/config-archive-model'
import type { ConfigArchiveExportForm } from '@/lib/config-archive'

type ConfigArchiveExportCardProps = {
  error: string
  form: ConfigArchiveExportForm
  onClearUnavailable: () => void
  pending: boolean
  summary: ConfigArchiveExportSummary | null
  unavailable: boolean
  unavailableMessage: string
}

/**
 * The export card: one password input, one button, one non-secret summary.
 *
 * The passphrase is a secret held by the native form in memory only — it is
 * never written to storage, a URL, a log or a title, and the card never renders
 * a value back into a message. The summary reports the byte length and the
 * metadata the server sent; the archive bytes themselves only ever travel to
 * the fixed-name download.
 */
export function ConfigArchiveExportCard({
  error,
  form,
  onClearUnavailable,
  pending,
  summary,
  unavailable,
  unavailableMessage,
}: ConfigArchiveExportCardProps) {
  const { t, locale } = useLocale()
  const passphrase = useStore(form.store, (state) => state.values.passphrase)

  return (
    <SettingsCard icon={Download} title={t('configArchiveExportTitle')}>
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
        <div className="grid min-w-0 items-end gap-3 sm:grid-cols-2">
          <div className="grid min-w-0 gap-1">
            <Label
              className="text-xs font-medium"
              htmlFor="settings-config-archive-passphrase"
            >
              {t('configArchivePassphrase')}
            </Label>
            <form.Field name="passphrase">
              {(field) => (
                <Input
                  autoComplete="new-password"
                  className="h-8 text-sm"
                  disabled={pending || unavailable}
                  id="settings-config-archive-passphrase"
                  name="settings-config-archive-passphrase"
                  onChange={(event) => field.handleChange(event.target.value)}
                  placeholder={t('configArchivePassphrasePlaceholder')}
                  type="password"
                  value={field.state.value}
                />
              )}
            </form.Field>
          </div>
          <div className="flex min-w-0 items-center gap-2">
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
              disabled={
                !configArchiveExportEnabled({ passphrase, pending }) ||
                unavailable
              }
              id="settings-config-archive-export"
              size="sm"
              type="submit"
            >
              <Download aria-hidden="true" />
              {t('configArchiveExportAction')}
            </Button>
          </div>
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
              id="settings-config-archive-export-retry"
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
            id="settings-config-archive-export-error"
            role="alert"
          >
            {error}
          </p>
        ) : null}

        {summary ? (
          <dl className="m-0 flex min-w-0 flex-wrap items-center gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
            <div className="flex items-center gap-1">
              <dt>{t('configArchiveExportBackend')}</dt>
              <dd className="m-0 font-medium">{summary.backendKind}</dd>
            </div>
            <div className="flex items-center gap-1">
              <dt>{t('configArchiveExportFormatVersion')}</dt>
              <dd className="m-0 font-medium">
                {summary.formatVersion ?? '-'}
              </dd>
            </div>
            <div className="flex items-center gap-1">
              <dt>{t('configArchiveExportSize')}</dt>
              <dd className="m-0 font-medium tabular-nums">
                {formatAuditCount(summary.size, locale)}
              </dd>
            </div>
            <div className="flex items-center gap-1">
              <dt>{t('configArchiveExportFingerprint')}</dt>
              <dd
                className="m-0 font-mono"
                id="settings-config-archive-export-fingerprint"
                title={summary.payloadFingerprint}
              >
                {shortFingerprint(summary.payloadFingerprint)}
              </dd>
            </div>
          </dl>
        ) : null}

        <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
          {t('configArchiveExportFootnote')}
        </p>
      </form>
    </SettingsCard>
  )
}
