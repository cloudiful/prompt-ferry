import { useEffect, useState } from 'react'
import {
  LoaderCircle,
  FileText,
  RefreshCw,
  ShieldCheck,
  Zap,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { SettingsCard } from '@/components/settings/SettingsCard'
import { StreamDeltaBatchingFields } from '@/components/shared/StreamDeltaBatchingFields'
import { formatApiError } from '@/api'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import {
  isRetiredEpochError,
  useSettingsRequests,
} from '@/lib/settings-requests'
import {
  REQUEST_CONTENT_LOGGING_MODES,
  SETTING_FORM_RANGES,
  parseSettingInteger,
} from '@/lib/settings-requests-model'
import { settingFormKey, type SettingForm } from '@/lib/settings-keys'

function PendingSpinner({ label }: { label: string }) {
  return (
    <span aria-label={label} className="inline-flex" role="status">
      <LoaderCircle
        aria-hidden="true"
        className="size-3.5 animate-spin text-muted-foreground"
      />
    </span>
  )
}

function RetrySection({
  disabled = false,
  label,
  loadingLabel,
  onRetry,
  pending = false,
}: {
  disabled?: boolean
  label: string
  loadingLabel: string
  onRetry: () => void
  pending?: boolean
}) {
  return (
    <Button
      aria-busy={pending}
      disabled={disabled}
      onClick={onRetry}
      size="xs"
      type="button"
      variant="outline"
    >
      {pending ? <PendingSpinner label={loadingLabel} /> : null}
      {label}
    </Button>
  )
}

/**
 * The admin requests settings: content logging, raw/approval retention, stream
 * delta batching and the model-route whitelist.
 *
 * Every control autosaves through the form's debounce, so the tab deliberately
 * exposes no save button and no success toast. The only explicit action is the
 * retry of a section whose write failed, which the hook keeps dirty until the
 * operator either edits again or retries.
 */
export function SettingsRequestsTab() {
  const { t } = useLocale()
  const { notifyApiError } = useNotifier()
  const settings = useSettingsRequests()
  const { form, values, isPending, pendingOrigin } = settings

  const retentionKey = settingFormKey('requestContentLogging')
  const retentionPending = isPending(retentionKey)
  const retentionOrigin = pendingOrigin(retentionKey)
  const streamPending = isPending(settingFormKey('streamDeltaBatching'))
  const whitelistPending = isPending(settingFormKey('modelRouteWhitelist'))
  const controlsReady = settings.loaded

  /** The retry the operator started, shown while it still owns its resource. */
  const [claimedRetry, setClaimedRetry] = useState<SettingForm | null>(null)
  const claimOrigin = claimedRetry
    ? pendingOrigin(settingFormKey(claimedRetry))
    : null
  const activeClaim = claimOrigin === claimedRetry ? claimedRetry : null

  // The claim ends with its write: a settled, refused or reassigned claim
  // leaves no retry spinner behind for a later autosave of the same section.
  useEffect(() => {
    if (claimedRetry === null) return
    if (claimOrigin !== claimedRetry) setClaimedRetry(null)
  }, [claimOrigin, claimedRetry])

  /** The control that started the grouped read. */
  const [readOrigin, setReadOrigin] = useState<
    'header' | 'initial' | 'loaded' | null
  >(null)
  const [readFailure, setReadFailure] = useState<unknown>(null)
  const refreshFailure = readFailure ?? settings.refreshError
  const initialReadPending = settings.refreshing && readOrigin === 'initial'

  function retry(form: SettingForm): void {
    setClaimedRetry(form)
    settings.retryFailedSection(form)
  }

  async function refreshSettings(
    origin: 'header' | 'initial' | 'loaded',
  ): Promise<void> {
    setReadOrigin(origin)
    try {
      await settings.refresh()
      setReadFailure(null)
    } catch (cause) {
      // A refresh whose session was retired is not a failure of the session
      // that is logged in now.
      if (isRetiredEpochError(cause)) return
      setReadFailure(cause)
      notifyApiError(cause)
    } finally {
      setReadOrigin(null)
    }
  }

  const refresh = (
    <HeaderActions>
      <Button
        aria-label={t('refresh')}
        disabled={settings.refreshBlocked}
        id="settings-requests-refresh"
        onClick={() => void refreshSettings('header')}
        size="sm"
        type="button"
        variant="outline"
      >
        {settings.refreshing && readOrigin === 'header' ? (
          <PendingSpinner label={t('loading')} />
        ) : (
          <RefreshCw aria-hidden="true" />
        )}
        {t('refresh')}
      </Button>
    </HeaderActions>
  )

  if ((settings.loadError && !settings.loaded) || initialReadPending) {
    return (
      <>
        {refresh}
        <div className="grid min-w-0 gap-2 rounded-lg border bg-card px-3 py-2.5">
          {settings.loadError ? (
            <p className="m-0 text-sm" role="alert">
              {t('error')}
            </p>
          ) : null}
          <div>
            <Button
              aria-busy={initialReadPending}
              disabled={settings.refreshing}
              onClick={() => void refreshSettings('initial')}
              size="sm"
              type="button"
              variant="outline"
            >
              {initialReadPending ? (
                <PendingSpinner label={t('loading')} />
              ) : null}
              {t('retry')}
            </Button>
          </div>
        </div>
      </>
    )
  }

  if (settings.loading && !settings.loaded) {
    return (
      <>
        {refresh}
        <p className="m-0 text-sm text-muted-foreground" role="status">
          {t('loading')}
        </p>
      </>
    )
  }

  return (
    <>
      {refresh}
      {refreshFailure ? (
        <div className="grid min-w-0 gap-2 rounded-lg border bg-card px-3 py-2.5">
          <p className="m-0 text-sm" role="alert">
            {formatApiError(refreshFailure)}
          </p>
          <div>
            <Button
              aria-busy={settings.refreshing && readOrigin === 'loaded'}
              disabled={settings.refreshBlocked}
              onClick={() => void refreshSettings('loaded')}
              size="sm"
              type="button"
              variant="outline"
            >
              {settings.refreshing && readOrigin === 'loaded' ? (
                <PendingSpinner label={t('loading')} />
              ) : null}
              {t('retry')}
            </Button>
          </div>
        </div>
      ) : null}
      <div className="grid min-w-0 gap-3 lg:grid-cols-2">
        <SettingsCard icon={FileText} title={t('contentLogging')}>
          <div aria-busy={retentionPending} className="grid min-w-0 gap-3">
            <form.Field name="requestContentLogging.mode">
              {(field) => (
                <div className="grid min-w-0 gap-1">
                  <Label
                    className="inline-flex items-center gap-1 text-xs text-muted-foreground"
                    htmlFor="settings-request-content-logging-mode"
                  >
                    {t('contentLoggingMode')}
                    {retentionOrigin === 'requestContentLogging' &&
                    activeClaim !== 'requestContentLogging' ? (
                      <PendingSpinner label={t('loading')} />
                    ) : null}
                  </Label>
                  <Select
                    onValueChange={(value) =>
                      field.handleChange(value as typeof field.state.value)
                    }
                    value={field.state.value}
                  >
                    <SelectTrigger
                      aria-label={t('contentLoggingMode')}
                      className="w-full min-w-0"
                      disabled={!controlsReady || retentionPending}
                      id="settings-request-content-logging-mode"
                      name="settings-request-content-logging-mode"
                      size="sm"
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {REQUEST_CONTENT_LOGGING_MODES.map((option) => (
                        <SelectItem key={option.value} value={option.value}>
                          {t(option.labelKey)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              )}
            </form.Field>
            <div className="grid min-w-0 gap-3 sm:grid-cols-2">
              <form.Field name="requestContentLogging.raw_retention_days">
                {(field) => (
                  <div className="grid min-w-0 gap-1">
                    <Label
                      className="text-xs text-muted-foreground"
                      htmlFor="settings-raw-retention-days"
                    >
                      {t('rawRetentionDays')}
                    </Label>
                    <Input
                      className="h-8 text-sm tabular-nums"
                      disabled={!controlsReady || retentionPending}
                      id="settings-raw-retention-days"
                      inputMode="numeric"
                      max={SETTING_FORM_RANGES.rawRetentionDays.max}
                      min={SETTING_FORM_RANGES.rawRetentionDays.min}
                      name="settings-raw-retention-days"
                      onChange={(event) => {
                        const parsed = parseSettingInteger(
                          event.target.value,
                          SETTING_FORM_RANGES.rawRetentionDays,
                        )
                        if (parsed !== null) field.handleChange(parsed)
                      }}
                      step={1}
                      type="number"
                      value={field.state.value}
                    />
                  </div>
                )}
              </form.Field>
              <form.Field name="usageRetention.approval_retention_days">
                {(field) => (
                  <div className="grid min-w-0 gap-1">
                    <Label
                      className="inline-flex items-center gap-1 text-xs text-muted-foreground"
                      htmlFor="settings-approval-retention-days"
                    >
                      {t('approvalRetentionDays')}
                      {retentionOrigin === 'usageRetention' &&
                      activeClaim !== 'usageRetention' ? (
                        <PendingSpinner label={t('loading')} />
                      ) : null}
                    </Label>
                    <Input
                      className="h-8 text-sm tabular-nums"
                      disabled={!controlsReady || retentionPending}
                      id="settings-approval-retention-days"
                      inputMode="numeric"
                      min={SETTING_FORM_RANGES.approvalRetentionDays.min}
                      name="settings-approval-retention-days"
                      onChange={(event) => {
                        const parsed = parseSettingInteger(
                          event.target.value,
                          SETTING_FORM_RANGES.approvalRetentionDays,
                        )
                        if (parsed !== null) field.handleChange(parsed)
                      }}
                      step={1}
                      type="number"
                      value={field.state.value ?? ''}
                    />
                    {settings.failed.usageRetention ? (
                      <div>
                        <RetrySection
                          disabled={retentionPending}
                          label={t('retry')}
                          loadingLabel={t('loading')}
                          onRetry={() => retry('usageRetention')}
                          pending={activeClaim === 'usageRetention'}
                        />
                      </div>
                    ) : null}
                  </div>
                )}
              </form.Field>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <p className="m-0 text-xs text-muted-foreground">
                {t('contentLoggingOff')}
              </p>
              {settings.failed.requestContentLogging ? (
                <RetrySection
                  disabled={retentionPending}
                  label={t('retry')}
                  loadingLabel={t('loading')}
                  onRetry={() => retry('requestContentLogging')}
                  pending={activeClaim === 'requestContentLogging'}
                />
              ) : null}
            </div>
          </div>
        </SettingsCard>

        <SettingsCard icon={ShieldCheck} title={t('modelRouteWhitelist')}>
          <div className="grid min-w-0 gap-2">
            <div className="flex items-center gap-2">
              <form.Field name="modelRouteWhitelist.enabled">
                {(field) => (
                  <Switch
                    aria-label={t('modelRouteWhitelist')}
                    checked={field.state.value}
                    disabled={!controlsReady || whitelistPending}
                    id="settings-model-route-whitelist-enabled"
                    name="settings-model-route-whitelist-enabled"
                    onCheckedChange={field.handleChange}
                  />
                )}
              </form.Field>
              {whitelistPending && activeClaim !== 'modelRouteWhitelist' ? (
                <PendingSpinner label={t('loading')} />
              ) : null}
              {settings.failed.modelRouteWhitelist ? (
                <RetrySection
                  disabled={whitelistPending}
                  label={t('retry')}
                  loadingLabel={t('loading')}
                  onRetry={() => retry('modelRouteWhitelist')}
                  pending={activeClaim === 'modelRouteWhitelist'}
                />
              ) : null}
            </div>
            <p className="m-0 text-xs leading-relaxed text-muted-foreground">
              {t('modelRouteWhitelistHint')}
            </p>
          </div>
        </SettingsCard>

        <SettingsCard icon={Zap} title={t('streamDeltaBatching')}>
          <div className="grid min-w-0 gap-2 lg:col-span-full">
            <div className="flex items-center gap-2">
              <form.Field name="streamDeltaBatching.enabled">
                {(field) => (
                  <Switch
                    aria-label={t('streamDeltaBatching')}
                    checked={field.state.value}
                    disabled={!controlsReady || streamPending}
                    id="settings-stream-delta-batching-enabled"
                    name="settings-stream-delta-batching-enabled"
                    onCheckedChange={field.handleChange}
                  />
                )}
              </form.Field>
              {streamPending && activeClaim !== 'streamDeltaBatching' ? (
                <PendingSpinner label={t('loading')} />
              ) : null}
              {settings.failed.streamDeltaBatching ? (
                <RetrySection
                  disabled={streamPending}
                  label={t('retry')}
                  loadingLabel={t('loading')}
                  onRetry={() => retry('streamDeltaBatching')}
                  pending={activeClaim === 'streamDeltaBatching'}
                />
              ) : null}
            </div>
            <fieldset
              aria-busy={streamPending}
              className="m-0 min-w-0 border-0 p-0"
              disabled={!controlsReady || streamPending}
            >
              <StreamDeltaBatchingFields
                disabled={!controlsReady || streamPending}
                form={values.streamDeltaBatching}
                onChange={(patch) =>
                  form.setFieldValue('streamDeltaBatching', {
                    ...values.streamDeltaBatching,
                    ...patch,
                  })
                }
              />
            </fieldset>
          </div>
        </SettingsCard>
      </div>
    </>
  )
}
