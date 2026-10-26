import { useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import {
  BellRing,
  LoaderCircle,
  RefreshCw,
  Save,
  TriangleAlert,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { SettingsCard } from '@/components/settings/SettingsCard'
import { Switch } from '@/components/ui/switch'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import {
  disposeSettingsCacheAlert,
  setSettingsCacheAlertNotifier,
  useSettingsCacheAlertStore,
} from '@/lib/settings-cache-alert'
import {
  CACHE_ALERT_BOUNDS,
  CACHE_ALERT_KEY,
  parseCacheAlertInteger,
  parseCacheAlertNumber,
  type CacheAlertBounds,
  type CacheAlertForm,
} from '@/lib/settings-cache-alert-model'
import { useAsyncAction } from '@/lib/use-async-action'

type NumberFieldProps = {
  bounds: CacheAlertBounds
  disabled: boolean
  help: string
  id: string
  integer?: boolean
  label: string
  onChange: (value: number) => void
  placeholder: string
  step?: number
  value: number
}

/**
 * A numeric field whose text is local until it parses.
 *
 * A value the parser rejects is never emitted: the text stays as typed so the
 * operator can keep correcting it, and the store keeps the last accepted value.
 * Leaving the field restores the text from that accepted value, so the visible
 * number and the saved number always agree once the field is left. The
 * threshold's decimals survive unchanged because the float parser keeps the
 * parsed value rather than rounding it to the step.
 */
function NumberField({
  bounds,
  disabled,
  help,
  id,
  integer = false,
  label,
  onChange,
  placeholder,
  step = 1,
  value,
}: NumberFieldProps) {
  const [text, setText] = useState(String(value))

  useEffect(() => {
    setText(String(value))
  }, [value])

  return (
    <div className="grid min-w-0 gap-1">
      <Label className="text-xs font-medium" htmlFor={id}>
        {label}
      </Label>
      <Input
        className="h-8 text-sm tabular-nums"
        disabled={disabled}
        id={id}
        inputMode={integer ? 'numeric' : 'decimal'}
        max={bounds.max}
        min={bounds.min}
        name={id}
        onBlur={() => setText(String(value))}
        onChange={(event) => {
          const raw = event.target.value
          setText(raw)
          const parsed = integer
            ? parseCacheAlertInteger(raw, bounds)
            : parseCacheAlertNumber(raw, bounds)
          if (parsed === null) return
          onChange(parsed)
        }}
        step={step}
        type="number"
        value={text}
      />
      <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
        {help}
      </p>
    </div>
  )
}

/**
 * The admin cache-alert policy.
 *
 * One card with the enabled switch and the manual Save in its header, the four
 * bounded numbers in a two-column grid, the DingTalk webhook and its write-only
 * secret. The presence badge is the server's answer; the mode badge describes the
 * local draft, because a blank field keeps the stored secret and there is no
 * clear control. An unavailable backend replaces the form with its warning and a
 * retry read that only touches this resource.
 */
export function SettingsCacheAlertTab() {
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const { notifyApiError, notifySuccess } = useNotifier()
  const { isPending, pendingOrigin } = useAsyncAction()

  const form = useSettingsCacheAlertStore((state) => state.form)
  const loaded = useSettingsCacheAlertStore((state) => state.loaded)
  const loading = useSettingsCacheAlertStore((state) => state.loading)
  const unavailable = useSettingsCacheAlertStore((state) => state.unavailable)
  const unavailableMessage = useSettingsCacheAlertStore(
    (state) => state.unavailableMessage,
  )
  const refresh = useSettingsCacheAlertStore((state) => state.refresh)
  const setField = useSettingsCacheAlertStore((state) => state.setField)
  const setSecretDraft = useSettingsCacheAlertStore(
    (state) => state.setSecretDraft,
  )
  const save = useSettingsCacheAlertStore((state) => state.save)

  const savedMessage = useRef<() => void>(() => {})
  savedMessage.current = () => notifySuccess(t('cacheAlertSaved'))

  useEffect(() => {
    setSettingsCacheAlertNotifier({
      notifyApiError: (cause) => notifyApiError(cause),
      notifySaved: () => savedMessage.current(),
    })
    return () => {
      setSettingsCacheAlertNotifier(null)
      disposeSettingsCacheAlert()
    }
  }, [notifyApiError, t])

  const savePending = isPending(CACHE_ALERT_KEY)
  const saveOrigin = pendingOrigin(CACHE_ALERT_KEY)
  const disabled = !loaded || unavailable || savePending
  const secretPresent = form.has_dingtalk_secret
  const secretReplacing = form.secret.mode === 'replace'

  const change = <
    K extends Exclude<keyof CacheAlertForm, 'has_dingtalk_secret'>,
  >(
    field: K,
    value: CacheAlertForm[K],
  ) => setField(field, value)

  return (
    <SettingsCard icon={BellRing} title={t('cacheAlertTitle')}>
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
          {saveOrigin === 'cacheAlert' ? (
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
          <Switch
            aria-label={t('cacheAlertEnabled')}
            checked={form.enabled}
            disabled={disabled}
            id="settings-cache-alert-enabled"
            name="settings-cache-alert-enabled"
            onCheckedChange={(checked) => change('enabled', checked === true)}
          />
          <Button
            disabled={disabled}
            id="settings-cache-alert-save"
            onClick={() => void save()}
            size="sm"
            type="button"
          >
            <Save aria-hidden="true" />
            {t('save')}
          </Button>
        </div>

        {unavailable ? (
          <div className="grid min-w-0 gap-1 rounded-md border border-warning/30 bg-warning/10 px-3 py-2">
            <p className="m-0 flex items-center gap-1.5 text-xs leading-relaxed text-muted-foreground">
              <TriangleAlert
                aria-hidden="true"
                className="size-3.5 shrink-0 text-warning"
              />
              {t('cacheAlertUnavailable')}
            </p>
            {unavailableMessage ? (
              <p className="m-0 text-[11px] text-muted-foreground">
                {unavailableMessage}
              </p>
            ) : null}
            <div className="mt-1">
              <Button
                id="settings-cache-alert-retry"
                onClick={() => void refresh(queryClient)}
                size="xs"
                type="button"
                variant="outline"
              >
                <RefreshCw aria-hidden="true" />
                {t('refresh')}
              </Button>
            </div>
          </div>
        ) : (
          <div className="grid min-w-0 gap-3 sm:grid-cols-2">
            <NumberField
              bounds={CACHE_ALERT_BOUNDS.window_minutes}
              disabled={disabled}
              help={t('cacheAlertWindowMinutesHelp')}
              id="settings-cache-alert-window-minutes"
              integer
              label={t('cacheAlertWindowMinutes')}
              onChange={(value) => change('window_minutes', value)}
              placeholder={t('cacheAlertWindowMinutesPlaceholder')}
              value={form.window_minutes}
            />
            <NumberField
              bounds={CACHE_ALERT_BOUNDS.min_turns}
              disabled={disabled}
              help={t('cacheAlertMinTurnsHelp')}
              id="settings-cache-alert-min-turns"
              integer
              label={t('cacheAlertMinTurns')}
              onChange={(value) => change('min_turns', value)}
              placeholder={t('cacheAlertMinTurnsPlaceholder')}
              value={form.min_turns}
            />
            <NumberField
              bounds={CACHE_ALERT_BOUNDS.threshold}
              disabled={disabled}
              help={t('cacheAlertThresholdHelp')}
              id="settings-cache-alert-threshold"
              label={t('cacheAlertThreshold')}
              onChange={(value) => change('threshold', value)}
              placeholder={t('cacheAlertThresholdPlaceholder')}
              step={0.05}
              value={form.threshold}
            />
            <NumberField
              bounds={CACHE_ALERT_BOUNDS.cooldown_minutes}
              disabled={disabled}
              help={t('cacheAlertCooldownMinutesHelp')}
              id="settings-cache-alert-cooldown-minutes"
              integer
              label={t('cacheAlertCooldownMinutes')}
              onChange={(value) => change('cooldown_minutes', value)}
              placeholder={t('cacheAlertCooldownMinutesPlaceholder')}
              value={form.cooldown_minutes}
            />

            <div className="grid min-w-0 gap-1 sm:col-span-2">
              <Label
                className="text-xs font-medium"
                htmlFor="settings-cache-alert-webhook-url"
              >
                {t('cacheAlertWebhookUrl')}
              </Label>
              <Input
                autoComplete="off"
                className="h-8 text-sm"
                disabled={disabled}
                id="settings-cache-alert-webhook-url"
                name="settings-cache-alert-webhook-url"
                onChange={(event) =>
                  change('dingtalk_webhook_url', event.target.value)
                }
                placeholder={t('cacheAlertWebhookUrlPlaceholder')}
                value={form.dingtalk_webhook_url}
              />
              <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
                {t('cacheAlertWebhookUrlHelp')}
              </p>
            </div>

            <div className="grid min-w-0 gap-1 sm:col-span-2">
              <div className="flex min-w-0 flex-wrap items-center gap-1.5">
                <Label
                  className="text-xs font-medium"
                  htmlFor="settings-cache-alert-secret"
                >
                  {t('cacheAlertSecret')}
                </Label>
                <span
                  className={
                    secretPresent
                      ? 'rounded border border-primary/40 px-1.5 py-0.5 text-[10px] text-primary'
                      : 'rounded border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground'
                  }
                  data-secret-present={secretPresent ? 'true' : 'false'}
                >
                  {secretPresent
                    ? t('cacheAlertSecretConfigured')
                    : t('cacheAlertSecretNotConfigured')}
                </span>
                <span
                  className={
                    secretReplacing
                      ? 'rounded border border-warning/40 px-1.5 py-0.5 text-[10px] text-warning'
                      : 'rounded border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground'
                  }
                  data-secret-mode={form.secret.mode}
                >
                  {secretReplacing
                    ? t('cacheAlertSecretReplacing')
                    : t('cacheAlertSecretKeeping')}
                </span>
              </div>
              <Input
                autoComplete="off"
                className="h-8 text-sm"
                disabled={disabled}
                id="settings-cache-alert-secret"
                name="settings-cache-alert-secret"
                onChange={(event) => setSecretDraft(event.target.value)}
                placeholder={t('cacheAlertSecretPlaceholder')}
                type="password"
                value={form.secret.value ?? ''}
              />
              <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
                {t('cacheAlertSecretHelp')}
              </p>
            </div>
          </div>
        )}

        {loaded && !unavailable ? (
          <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
            {t('cacheAlertHint')}
          </p>
        ) : null}
      </div>
    </SettingsCard>
  )
}
