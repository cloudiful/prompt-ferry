import {
  Database,
  LoaderCircle,
  RefreshCw,
  Save,
  TriangleAlert,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { SettingsCard } from '@/components/settings/SettingsCard'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { useSettingsSharedValkey } from '@/lib/settings-shared-valkey'

/**
 * The shared Valkey card: the database-owned URL and shared TTL.
 *
 * The stored URL is never echoed; an empty draft keeps it and the clear switch
 * is explicit. Save claims `setting:sharedValkey`, so a double click sends one
 * PATCH. The revisions show that every running worker keeps its applied
 * configuration until it restarts.
 */
export function SettingsSharedValkeyCard() {
  const { t } = useLocale()
  const { notifyApiError, notifySuccess } = useNotifier()
  const settings = useSettingsSharedValkey()
  const { form, response } = settings
  const savePending = settings.pending
  const disabled = !settings.loaded || savePending

  const refresh = () => {
    void settings.refresh().catch(notifyApiError)
  }

  const save = () => {
    void settings
      .save(form)
      .then((outcome) => {
        if (outcome.executed) notifySuccess(t('sharedValkeySaved'))
      })
      .catch(notifyApiError)
  }

  return (
    <SettingsCard icon={Database} title={t('sharedValkeyTitle')}>
      <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
        <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
          {t('sharedValkeyHint')}
        </p>
        <Button
          aria-label={t('refresh')}
          disabled={savePending}
          id="settings-shared-valkey-refresh"
          onClick={refresh}
          size="sm"
          type="button"
          variant="outline"
        >
          <RefreshCw aria-hidden="true" />
          {t('refresh')}
        </Button>
      </div>

      {settings.loadError ? (
        <p className="m-0 text-sm" role="alert">
          {t('error')}
        </p>
      ) : null}

      {settings.loading ? (
        <p
          aria-label={t('loading')}
          className="m-0 inline-flex items-center gap-1.5 text-xs text-muted-foreground"
          role="status"
        >
          <LoaderCircle aria-hidden="true" className="size-3.5 animate-spin" />
          {t('loading')}
        </p>
      ) : null}

      {response ? (
        <div className="grid min-w-0 gap-1 text-xs">
          <p className="m-0 font-medium">
            {response.configured
              ? t('sharedValkeyConfigured')
              : t('sharedValkeyNotConfigured')}
            {response.url_masked ? (
              <span className="ml-2 font-mono text-muted-foreground">
                {response.url_masked}
              </span>
            ) : null}
          </p>
          <p className="m-0 text-[11px] text-muted-foreground">
            {t('sharedValkeyRevision', {
              applied: String(response.applied_revision ?? '-'),
              desired: String(response.desired_revision ?? '-'),
            })}
          </p>
          {response.restart_required ? (
            <p className="m-0 flex items-center gap-1.5 rounded-md border border-warning/30 bg-warning/10 px-2 py-1 text-[11px] text-muted-foreground">
              <TriangleAlert
                aria-hidden="true"
                className="size-3.5 shrink-0 text-warning"
              />
              {t('sharedValkeyRestartRequired')}
            </p>
          ) : null}
          {response.workers_restart_required > 0 ? (
            <p className="m-0 text-[11px] text-muted-foreground">
              {t('sharedValkeyWorkersRestart', {
                count: String(response.workers_restart_required),
              })}
            </p>
          ) : null}
        </div>
      ) : null}

      <form
        className="grid min-w-0 gap-3"
        noValidate
        onSubmit={(event) => {
          event.preventDefault()
          event.stopPropagation()
          if (disabled) return
          save()
        }}
      >
        <div className="grid min-w-0 gap-3 sm:grid-cols-2">
          <div className="grid min-w-0 gap-1">
            <Label
              className="text-xs font-medium"
              htmlFor="settings-shared-valkey-url"
            >
              {t('sharedValkeyUrl')}
            </Label>
            <Input
              autoComplete="off"
              className="h-8 font-mono text-xs"
              disabled={disabled || form.clear}
              id="settings-shared-valkey-url"
              name="settings-shared-valkey-url"
              onChange={(event) => settings.setField('url', event.target.value)}
              placeholder={t('sharedValkeyUrlPlaceholder')}
              type="password"
              value={form.url}
            />
            <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
              {t('sharedValkeyUrlHelp')}
            </p>
          </div>
          <div className="grid min-w-0 gap-1">
            <Label
              className="text-xs font-medium"
              htmlFor="settings-shared-valkey-ttl"
            >
              {t('sharedValkeyTtl')}
            </Label>
            <Input
              className="h-8 text-sm"
              disabled={disabled}
              id="settings-shared-valkey-ttl"
              min={60}
              max={2592000}
              name="settings-shared-valkey-ttl"
              onChange={(event) =>
                settings.setField(
                  'ttl_seconds',
                  Number.parseInt(event.target.value, 10) || 0,
                )
              }
              type="number"
              value={form.ttl_seconds}
            />
            <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
              {t('sharedValkeyTtlHelp')}
            </p>
          </div>
        </div>

        <div className="flex min-w-0 items-center justify-between gap-3">
          <div className="flex min-w-0 items-center gap-2">
            <Switch
              aria-label={t('sharedValkeyClear')}
              checked={form.clear}
              disabled={disabled}
              id="settings-shared-valkey-clear"
              name="settings-shared-valkey-clear"
              onCheckedChange={(checked) =>
                settings.setField('clear', checked === true)
              }
            />
            <span className="inline-flex min-w-0 flex-col">
              <span className="text-xs font-medium">
                {t('sharedValkeyClear')}
              </span>
              <span className="text-[11px] text-muted-foreground">
                {t('sharedValkeyClearHelp')}
              </span>
            </span>
          </div>
          <Button
            aria-busy={savePending}
            disabled={disabled}
            id="settings-shared-valkey-save"
            size="sm"
            type="submit"
          >
            {savePending ? (
              <LoaderCircle
                aria-hidden="true"
                className="size-3.5 animate-spin"
              />
            ) : (
              <Save aria-hidden="true" />
            )}
            {t('save')}
          </Button>
        </div>
      </form>
    </SettingsCard>
  )
}
