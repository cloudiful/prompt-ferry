import { useEffect, useRef } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { LoaderCircle, Save, Shield } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { SettingsCard } from '@/components/settings/SettingsCard'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import {
  disposeSettingsNetwork,
  setSettingsNetworkNotifier,
  useSettingsNetworkStore,
} from '@/lib/settings-network'
import {
  RELAY_IP_WHITELIST_KEY,
  type RelayIpWhitelistField,
} from '@/lib/settings-network-model'
import { useAsyncAction } from '@/lib/use-async-action'

type RelayFieldProps = {
  disabled: boolean
  field: RelayIpWhitelistField
  help: string
  id: string
  label: string
  onChange: (field: RelayIpWhitelistField, value: string) => void
  placeholder: string
  value: string
}

/**
 * One CIDR list: a label, the monospace textarea, and the one sentence that
 * explains what the list governs.
 *
 * The help text is written once, under the control; a tooltip repeating the same
 * sentence would add a second copy of the same guidance.
 */
function RelayField({
  disabled,
  field,
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
        onChange={(event) => onChange(field, event.target.value)}
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
  const queryClient = useQueryClient()
  const { notifyApiError, notifySuccess } = useNotifier()
  const { isPending, pendingOrigin } = useAsyncAction()

  const form = useSettingsNetworkStore((state) => state.form)
  const loaded = useSettingsNetworkStore((state) => state.loaded)
  const loading = useSettingsNetworkStore((state) => state.loading)
  const loadError = useSettingsNetworkStore((state) => state.loadError)
  const refresh = useSettingsNetworkStore((state) => state.refresh)
  const setField = useSettingsNetworkStore((state) => state.setField)
  const save = useSettingsNetworkStore((state) => state.save)

  const savedMessage = useRef<() => void>(() => {})
  savedMessage.current = () => notifySuccess(t('saved'))

  useEffect(() => {
    setSettingsNetworkNotifier({
      notifyError: (cause) => notifyApiError(cause),
      notifySaved: () => savedMessage.current(),
    })
    return () => {
      setSettingsNetworkNotifier(null)
      disposeSettingsNetwork()
    }
  }, [notifyApiError])

  const savePending = isPending(RELAY_IP_WHITELIST_KEY)
  const saveOrigin = pendingOrigin(RELAY_IP_WHITELIST_KEY)
  const disabled = !loaded || savePending

  if (loadError && !loaded) {
    return (
      <div className="grid min-w-0 gap-2 rounded-lg border bg-card px-3 py-2.5">
        <p className="m-0 text-sm" role="alert">
          {t('error')}
        </p>
        <div>
          <Button
            onClick={() => void refresh(queryClient)}
            size="sm"
            type="button"
            variant="outline"
          >
            {t('retry')}
          </Button>
        </div>
      </div>
    )
  }

  return (
    <SettingsCard icon={Shield} title={t('relayIpWhitelist')}>
      <div className="grid min-w-0 gap-3">
        <div className="flex min-w-0 flex-wrap items-center justify-end gap-2">
          {loading && !loaded ? (
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
            onClick={() => void save()}
            size="sm"
            type="button"
          >
            <Save aria-hidden="true" />
            {t('save')}
          </Button>
        </div>
        <div className="grid min-w-0 gap-3 md:grid-cols-2">
          <RelayField
            disabled={disabled}
            field="allowed_cidrs_text"
            help={t('allowedCidrsHelp')}
            id="settings-allowed-cidrs"
            label={t('allowedCidrs')}
            onChange={setField}
            placeholder={t('allowedCidrsPlaceholder')}
            value={form.allowed_cidrs_text}
          />
          <RelayField
            disabled={disabled}
            field="trusted_proxy_cidrs_text"
            help={t('trustedProxyCidrsHelp')}
            id="settings-trusted-proxy-cidrs"
            label={t('trustedProxyCidrs')}
            onChange={setField}
            placeholder={t('trustedProxyCidrsPlaceholder')}
            value={form.trusted_proxy_cidrs_text}
          />
        </div>
      </div>
    </SettingsCard>
  )
}
