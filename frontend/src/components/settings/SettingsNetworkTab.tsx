import { LoaderCircle, RefreshCw, Save, Shield } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { SettingsCard } from '@/components/settings/SettingsCard'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { useSettingsNetwork } from '@/lib/settings-network'

type RelayFieldProps = {
  disabled: boolean
  help: string
  id: string
  label: string
  onChange: (value: string) => void
  placeholder: string
  value: string
}

/**
 * One CIDR list: a label, the monospace textarea, and the one sentence that
 * explains what the list governs.
 */
function RelayField({
  disabled,
  help,
  id,
  label,
  onChange,
  placeholder,
  value,
}: RelayFieldProps) {
  return (
    <div className="grid min-w-0 content-start gap-1.5">
      <Label className="text-xs font-medium" htmlFor={id}>
        {label}
      </Label>
      <Textarea
        className="min-h-28 resize-y font-mono text-xs"
        disabled={disabled}
        id={id}
        name={id}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        rows={5}
        spellCheck={false}
        value={value}
      />
      <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
        {help}
      </p>
    </div>
  )
}

/**
 * The admin network settings: the relay IP whitelist.
 *
 * This tab is saved by hand. The Save button claims the shared
 * `setting:relayIpWhitelist` key, so a double click sends one request and earns
 * one confirmation, and a save held off by the guard sends nothing at all. The
 * lists are validated by the server: the tab sends what the operator typed and
 * shows the server's answer.
 */
export function SettingsNetworkTab() {
  const { t } = useLocale()
  const { notifyApiError, notifySuccess } = useNotifier()
  const settings = useSettingsNetwork({
    onSaved: () => notifySuccess(t('saved')),
  })
  const { form } = settings

  const savePending = settings.pending
  const saveOrigin = settings.pendingOrigin
  const disabled = !settings.loaded || savePending

  const refresh = () => {
    void settings.refresh().catch(notifyApiError)
  }

  if (settings.loadError && !settings.loaded) {
    return (
      <>
        <HeaderActions>
          <Button
            aria-label={t('refresh')}
            disabled={settings.refreshBlocked}
            id="settings-network-refresh"
            onClick={refresh}
            size="sm"
            type="button"
            variant="outline"
          >
            <RefreshCw aria-hidden="true" />
            {t('refresh')}
          </Button>
        </HeaderActions>
        <div className="grid min-w-0 gap-2 rounded-lg border bg-card px-3 py-2.5">
          <p className="m-0 text-sm" role="alert">
            {t('error')}
          </p>
          <div>
            <Button onClick={refresh} size="sm" type="button" variant="outline">
              {t('retry')}
            </Button>
          </div>
        </div>
      </>
    )
  }

  return (
    <SettingsCard icon={Shield} title={t('relayIpWhitelist')}>
      <HeaderActions>
        <Button
          aria-label={t('refresh')}
          disabled={settings.refreshBlocked}
          id="settings-network-refresh"
          onClick={refresh}
          size="sm"
          type="button"
          variant="outline"
        >
          <RefreshCw aria-hidden="true" />
          {t('refresh')}
        </Button>
      </HeaderActions>
      {settings.loadError ? (
        <p
          className="m-0 rounded-lg border bg-card px-3 py-2.5 text-sm"
          role="alert"
        >
          {t('error')}
        </p>
      ) : null}
      <form
        className="grid min-w-0 gap-3"
        noValidate
        onSubmit={(event) => {
          event.preventDefault()
          event.stopPropagation()
          if (savePending) return
          void form.handleSubmit().catch(notifyApiError)
        }}
      >
        <div className="flex min-w-0 flex-wrap items-center justify-end gap-2">
          {settings.loading && !settings.loaded ? (
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
          {saveOrigin === 'settings' ? (
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
            disabled={disabled}
            id="settings-relay-ip-whitelist-save"
            size="sm"
            type="submit"
          >
            <Save aria-hidden="true" />
            {t('save')}
          </Button>
        </div>
        <div className="grid min-w-0 gap-3 md:grid-cols-2">
          <form.Field name="allowed_cidrs_text">
            {(field) => (
              <RelayField
                disabled={disabled}
                help={t('allowedCidrsHelp')}
                id="settings-allowed-cidrs"
                label={t('allowedCidrs')}
                onChange={field.handleChange}
                placeholder={t('allowedCidrsPlaceholder')}
                value={field.state.value}
              />
            )}
          </form.Field>
          <form.Field name="trusted_proxy_cidrs_text">
            {(field) => (
              <RelayField
                disabled={disabled}
                help={t('trustedProxyCidrsHelp')}
                id="settings-trusted-proxy-cidrs"
                label={t('trustedProxyCidrs')}
                onChange={field.handleChange}
                placeholder={t('trustedProxyCidrsPlaceholder')}
                value={field.state.value}
              />
            )}
          </form.Field>
        </div>
      </form>
    </SettingsCard>
  )
}
