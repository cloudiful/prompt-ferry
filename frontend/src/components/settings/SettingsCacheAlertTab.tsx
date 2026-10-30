import { useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import {
  BellRing,
  LoaderCircle,
  Mail,
  Plus,
  RefreshCw,
  Save,
  TriangleAlert,
  X,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { SettingsCard } from '@/components/settings/SettingsCard'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import type { CacheAlertSmtpTls } from '@/generated/admin-api'
import type { MessageKey } from '@/i18n'
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
  cacheAlertSmtpIssues,
  parseCacheAlertInteger,
  parseCacheAlertNumber,
  type CacheAlertBounds,
  type CacheAlertForm,
  type CacheAlertSmtpIssue,
  type CacheAlertSmtpIssueCode,
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

/** The message each SMTP problem carries, in the tab's own copy. */
const SMTP_ISSUE_MESSAGES: Record<CacheAlertSmtpIssueCode, MessageKey> = {
  host_required: 'cacheAlertSmtpHostRequired',
  host_invalid: 'cacheAlertSmtpHostInvalid',
  from_required: 'cacheAlertSmtpFromRequired',
  from_invalid: 'cacheAlertSmtpFromInvalid',
  recipient_required: 'cacheAlertSmtpRecipientRequired',
  recipient_invalid: 'cacheAlertSmtpRecipientInvalid',
  credentials_paired: 'cacheAlertSmtpCredentialsPaired',
}

/** One field's problem, as an accessible error under the field. */
function FieldError({ id, message }: { id: string; message: string }) {
  return (
    <p
      className="m-0 text-[11px] leading-relaxed text-destructive"
      id={id}
      role="alert"
    >
      {message}
    </p>
  )
}

/**
 * The admin cache-alert policy.
 *
 * One card with the enabled switch and the manual Save in its header, the four
 * bounded numbers in a two-column grid, the DingTalk webhook and its write-only
 * secret, and the SMTP channel as a second group. The presence badges are the
 * server's answers; the mode badges describe the local drafts, because a blank
 * field keeps the stored secret and there is no clear control. An unavailable
 * backend replaces the form with its warning and a retry read that only touches
 * this resource.
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
  const setSmtpField = useSettingsCacheAlertStore((state) => state.setSmtpField)
  const setSmtpPasswordDraft = useSettingsCacheAlertStore(
    (state) => state.setSmtpPasswordDraft,
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

  const smtp = form.smtp
  const smtpIssues = cacheAlertSmtpIssues(form)
  const smtpInvalid = smtpIssues.length > 0
  const smtpPasswordPresent = smtp.has_password
  const smtpPasswordReplacing = smtp.password.mode === 'replace'

  const change = <
    K extends Exclude<keyof CacheAlertForm, 'has_dingtalk_secret' | 'smtp'>,
  >(
    field: K,
    value: CacheAlertForm[K],
  ) => setField(field, value)

  const issueFor = (
    field: CacheAlertSmtpIssue['field'],
    index?: number,
  ): CacheAlertSmtpIssue | undefined =>
    smtpIssues.find((issue) => issue.field === field && issue.index === index)

  const issueMessage = (issue?: CacheAlertSmtpIssue): string =>
    issue ? t(SMTP_ISSUE_MESSAGES[issue.code]) : ''

  const updateRecipient = (index: number, value: string) => {
    const next = [...smtp.to]
    next[index] = value
    setSmtpField('to', next)
  }

  const removeRecipient = (index: number) => {
    setSmtpField(
      'to',
      smtp.to.filter((_, position) => position !== index),
    )
  }

  const addRecipient = () => {
    setSmtpField('to', [...smtp.to, ''])
  }

  const tlsLabel = (tls: CacheAlertSmtpTls): string =>
    tls === 'start_tls'
      ? t('cacheAlertSmtpTlsStartTls')
      : t('cacheAlertSmtpTlsImplicit')

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
            disabled={disabled || smtpInvalid}
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
          <div className="grid min-w-0 gap-3">
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

            <div className="grid min-w-0 gap-3 border-t pt-3">
              <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
                <h3 className="m-0 inline-flex items-center gap-1.5 text-xs font-semibold">
                  <Mail
                    aria-hidden="true"
                    className="size-3.5 text-muted-foreground"
                  />
                  {t('cacheAlertSmtpTitle')}
                </h3>
                <div className="flex min-w-0 items-center gap-2">
                  <Switch
                    aria-label={t('cacheAlertSmtpEnabled')}
                    checked={smtp.enabled}
                    disabled={disabled}
                    id="settings-cache-alert-smtp-enabled"
                    name="settings-cache-alert-smtp-enabled"
                    onCheckedChange={(checked) =>
                      setSmtpField('enabled', checked === true)
                    }
                  />
                  <span className="text-xs font-medium">
                    {t('cacheAlertSmtpEnabled')}
                  </span>
                </div>
              </div>

              <div className="grid min-w-0 gap-3 sm:grid-cols-2">
                <div className="grid min-w-0 gap-1">
                  <Label
                    className="text-xs font-medium"
                    htmlFor="settings-cache-alert-smtp-host"
                  >
                    {t('cacheAlertSmtpHost')}
                  </Label>
                  <Input
                    aria-describedby={
                      issueFor('host')
                        ? 'settings-cache-alert-smtp-host-error'
                        : undefined
                    }
                    aria-invalid={issueFor('host') ? true : undefined}
                    autoComplete="off"
                    className="h-8 text-sm"
                    disabled={disabled}
                    id="settings-cache-alert-smtp-host"
                    name="settings-cache-alert-smtp-host"
                    onChange={(event) =>
                      setSmtpField('host', event.target.value)
                    }
                    placeholder={t('cacheAlertSmtpHostPlaceholder')}
                    value={smtp.host}
                  />
                  {issueFor('host') ? (
                    <FieldError
                      id="settings-cache-alert-smtp-host-error"
                      message={issueMessage(issueFor('host'))}
                    />
                  ) : (
                    <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
                      {t('cacheAlertSmtpHostHelp')}
                    </p>
                  )}
                </div>

                <NumberField
                  bounds={CACHE_ALERT_BOUNDS.smtp_port}
                  disabled={disabled}
                  help={t('cacheAlertSmtpPortHelp')}
                  id="settings-cache-alert-smtp-port"
                  integer
                  label={t('cacheAlertSmtpPort')}
                  onChange={(value) => setSmtpField('port', value)}
                  placeholder={t('cacheAlertSmtpPortPlaceholder')}
                  value={smtp.port}
                />

                <div className="grid min-w-0 gap-1">
                  <Label
                    className="text-xs font-medium"
                    htmlFor="settings-cache-alert-smtp-tls"
                  >
                    {t('cacheAlertSmtpTls')}
                  </Label>
                  <Select
                    onValueChange={(value) =>
                      setSmtpField('tls', value as CacheAlertSmtpTls)
                    }
                    value={smtp.tls}
                  >
                    <SelectTrigger
                      aria-label={t('cacheAlertSmtpTls')}
                      className="h-8 w-full"
                      disabled={disabled}
                      id="settings-cache-alert-smtp-tls"
                      name="settings-cache-alert-smtp-tls"
                      size="sm"
                    >
                      <SelectValue>{tlsLabel(smtp.tls)}</SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="implicit_tls">
                        {t('cacheAlertSmtpTlsImplicit')}
                      </SelectItem>
                      <SelectItem value="start_tls">
                        {t('cacheAlertSmtpTlsStartTls')}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                  <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
                    {t('cacheAlertSmtpTlsHelp')}
                  </p>
                </div>

                <div className="grid min-w-0 gap-1">
                  <Label
                    className="text-xs font-medium"
                    htmlFor="settings-cache-alert-smtp-from"
                  >
                    {t('cacheAlertSmtpFrom')}
                  </Label>
                  <Input
                    aria-describedby={
                      issueFor('from')
                        ? 'settings-cache-alert-smtp-from-error'
                        : undefined
                    }
                    aria-invalid={issueFor('from') ? true : undefined}
                    autoComplete="off"
                    className="h-8 text-sm"
                    disabled={disabled}
                    id="settings-cache-alert-smtp-from"
                    name="settings-cache-alert-smtp-from"
                    onChange={(event) =>
                      setSmtpField('from', event.target.value)
                    }
                    placeholder={t('cacheAlertSmtpFromPlaceholder')}
                    value={smtp.from}
                  />
                  {issueFor('from') ? (
                    <FieldError
                      id="settings-cache-alert-smtp-from-error"
                      message={issueMessage(issueFor('from'))}
                    />
                  ) : (
                    <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
                      {t('cacheAlertSmtpFromHelp')}
                    </p>
                  )}
                </div>

                <div className="grid min-w-0 gap-1 sm:col-span-2">
                  <Label
                    className="text-xs font-medium"
                    htmlFor="settings-cache-alert-smtp-to-0"
                  >
                    {t('cacheAlertSmtpTo')}
                  </Label>
                  {smtp.to.map((recipient, index) => {
                    const issue = issueFor('to', index)
                    const inputId = `settings-cache-alert-smtp-to-${index}`
                    const errorId = `${inputId}-error`
                    return (
                      <div key={index} className="grid min-w-0 gap-1">
                        <div className="flex min-w-0 items-center gap-1">
                          <Input
                            aria-describedby={issue ? errorId : undefined}
                            aria-invalid={issue ? true : undefined}
                            aria-label={t('cacheAlertSmtpRecipient', {
                              index: index + 1,
                            })}
                            autoComplete="off"
                            className="h-8 text-sm"
                            disabled={disabled}
                            id={inputId}
                            name={inputId}
                            onChange={(event) =>
                              updateRecipient(index, event.target.value)
                            }
                            placeholder={t('cacheAlertSmtpToPlaceholder')}
                            value={recipient}
                          />
                          {smtp.to.length > 1 ? (
                            <Button
                              aria-label={t('cacheAlertSmtpRemoveRecipient')}
                              disabled={disabled}
                              onClick={() => removeRecipient(index)}
                              size="icon-sm"
                              type="button"
                              variant="ghost"
                            >
                              <X aria-hidden="true" />
                            </Button>
                          ) : null}
                        </div>
                        {issue ? (
                          <FieldError
                            id={errorId}
                            message={issueMessage(issue)}
                          />
                        ) : null}
                      </div>
                    )
                  })}
                  {issueFor('to') ? (
                    <FieldError
                      id="settings-cache-alert-smtp-to-error"
                      message={issueMessage(issueFor('to'))}
                    />
                  ) : null}
                  <div>
                    <Button
                      disabled={disabled}
                      id="settings-cache-alert-smtp-add-recipient"
                      onClick={addRecipient}
                      size="xs"
                      type="button"
                      variant="outline"
                    >
                      <Plus aria-hidden="true" />
                      {t('cacheAlertSmtpAddRecipient')}
                    </Button>
                  </div>
                </div>

                <div className="grid min-w-0 gap-1">
                  <Label
                    className="text-xs font-medium"
                    htmlFor="settings-cache-alert-smtp-username"
                  >
                    {t('cacheAlertSmtpUsername')}
                  </Label>
                  <Input
                    autoComplete="off"
                    className="h-8 text-sm"
                    disabled={disabled}
                    id="settings-cache-alert-smtp-username"
                    name="settings-cache-alert-smtp-username"
                    onChange={(event) =>
                      setSmtpField('username', event.target.value)
                    }
                    placeholder={t('cacheAlertSmtpUsernamePlaceholder')}
                    value={smtp.username}
                  />
                  <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
                    {t('cacheAlertSmtpUsernameHelp')}
                  </p>
                </div>

                <div className="grid min-w-0 gap-1">
                  <div className="flex min-w-0 flex-wrap items-center gap-1.5">
                    <Label
                      className="text-xs font-medium"
                      htmlFor="settings-cache-alert-smtp-password"
                    >
                      {t('cacheAlertSmtpPassword')}
                    </Label>
                    <span
                      className={
                        smtpPasswordPresent
                          ? 'rounded border border-primary/40 px-1.5 py-0.5 text-[10px] text-primary'
                          : 'rounded border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground'
                      }
                      data-smtp-password-present={
                        smtpPasswordPresent ? 'true' : 'false'
                      }
                    >
                      {smtpPasswordPresent
                        ? t('cacheAlertSmtpPasswordSaved')
                        : t('cacheAlertSmtpPasswordNotSaved')}
                    </span>
                    <span
                      className={
                        smtpPasswordReplacing
                          ? 'rounded border border-warning/40 px-1.5 py-0.5 text-[10px] text-warning'
                          : 'rounded border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground'
                      }
                      data-smtp-password-mode={smtp.password.mode}
                    >
                      {smtpPasswordReplacing
                        ? t('cacheAlertSmtpPasswordReplacing')
                        : t('cacheAlertSmtpPasswordKeeping')}
                    </span>
                  </div>
                  <Input
                    autoComplete="off"
                    className="h-8 text-sm"
                    disabled={disabled}
                    id="settings-cache-alert-smtp-password"
                    name="settings-cache-alert-smtp-password"
                    onChange={(event) =>
                      setSmtpPasswordDraft(event.target.value)
                    }
                    placeholder={t('cacheAlertSmtpPasswordPlaceholder')}
                    type="password"
                    value={smtp.password.value ?? ''}
                  />
                  <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
                    {t('cacheAlertSmtpPasswordHelp')}
                  </p>
                  {issueFor('credentials') ? (
                    <FieldError
                      id="settings-cache-alert-smtp-credentials-error"
                      message={issueMessage(issueFor('credentials'))}
                    />
                  ) : null}
                  <p className="m-0 text-[11px] leading-relaxed text-warning">
                    {t('cacheAlertSmtpPasswordStored')}
                  </p>
                </div>
              </div>
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
