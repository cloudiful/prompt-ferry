import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { FileText, LoaderCircle, ShieldCheck, Zap } from 'lucide-react'
import { Button } from '@/components/ui/button'
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
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { useAsyncAction } from '@/lib/use-async-action'
import {
  REQUEST_CONTENT_LOGGING_MODES,
  SETTING_FORM_RANGES,
  parseSettingInteger,
} from '@/lib/settings-requests-model'
import {
  armSettingsRequestsAutosave,
  disposeSettingsRequestsAutosave,
  setSettingsRequestsNotifier,
  useSettingsRequestsStore,
} from '@/lib/settings-requests'
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

function RetrySection({ form, label }: { form: SettingForm; label: string }) {
  const retryFailedSection = useSettingsRequestsStore(
    (state) => state.retryFailedSection,
  )
  return (
    <Button
      onClick={() => retryFailedSection(form)}
      size="xs"
      type="button"
      variant="outline"
    >
      {label}
    </Button>
  )
}

/**
 * The admin requests settings: content logging, raw/approval retention, stream
 * delta batching and the model-route whitelist.
 *
 * Every control autosaves through the store's debounce, so the tab deliberately
 * exposes no save button and no success toast. The only explicit action is the
 * retry of a section whose write failed, which the store keeps dirty until the
 * operator either edits again or retries.
 */
export function SettingsRequestsTab() {
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const { notifyApiError } = useNotifier()
  const { pendingOrigin, isPending } = useAsyncAction()

  const requestContentLogging = useSettingsRequestsStore(
    (state) => state.requestContentLogging,
  )
  const usageRetention = useSettingsRequestsStore(
    (state) => state.usageRetention,
  )
  const streamDeltaBatching = useSettingsRequestsStore(
    (state) => state.streamDeltaBatching,
  )
  const modelRouteWhitelist = useSettingsRequestsStore(
    (state) => state.modelRouteWhitelist,
  )
  const loaded = useSettingsRequestsStore((state) => state.loaded)
  const loading = useSettingsRequestsStore((state) => state.loading)
  const loadError = useSettingsRequestsStore((state) => state.loadError)
  const failed = useSettingsRequestsStore((state) => state.failed)
  const refresh = useSettingsRequestsStore((state) => state.refresh)
  const setRequestContentLoggingMode = useSettingsRequestsStore(
    (state) => state.setRequestContentLoggingMode,
  )
  const setRawRetentionDays = useSettingsRequestsStore(
    (state) => state.setRawRetentionDays,
  )
  const setApprovalRetentionDays = useSettingsRequestsStore(
    (state) => state.setApprovalRetentionDays,
  )
  const setStreamDeltaBatching = useSettingsRequestsStore(
    (state) => state.setStreamDeltaBatching,
  )
  const setModelRouteWhitelistEnabled = useSettingsRequestsStore(
    (state) => state.setModelRouteWhitelistEnabled,
  )

  useEffect(() => {
    setSettingsRequestsNotifier(notifyApiError)
    armSettingsRequestsAutosave()
    return () => {
      setSettingsRequestsNotifier(null)
      disposeSettingsRequestsAutosave()
    }
  }, [notifyApiError])

  const retentionKey = settingFormKey('requestContentLogging')
  const retentionPending = isPending(retentionKey)
  const retentionOrigin = pendingOrigin(retentionKey)
  const streamPending = isPending(settingFormKey('streamDeltaBatching'))
  const whitelistPending = isPending(settingFormKey('modelRouteWhitelist'))
  const controlsReady = loaded

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

  if (loading && !loaded) {
    return (
      <p className="m-0 text-sm text-muted-foreground" role="status">
        {t('loading')}
      </p>
    )
  }

  return (
    <div className="grid min-w-0 gap-3 lg:grid-cols-2">
      <SettingsCard icon={FileText} title={t('contentLogging')}>
        <div aria-busy={retentionPending} className="grid min-w-0 gap-3">
          <div className="grid min-w-0 gap-1">
            <Label
              className="inline-flex items-center gap-1 text-xs text-muted-foreground"
              htmlFor="settings-request-content-logging-mode"
            >
              {t('contentLoggingMode')}
              {retentionOrigin === 'requestContentLogging' ? (
                <PendingSpinner label={t('loading')} />
              ) : null}
            </Label>
            <Select
              onValueChange={(value) =>
                setRequestContentLoggingMode(
                  value as typeof requestContentLogging.mode,
                )
              }
              value={requestContentLogging.mode}
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
          <div className="grid min-w-0 gap-3 sm:grid-cols-2">
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
                  if (parsed !== null) setRawRetentionDays(parsed)
                }}
                step={1}
                type="number"
                value={requestContentLogging.raw_retention_days}
              />
            </div>
            <div className="grid min-w-0 gap-1">
              <Label
                className="inline-flex items-center gap-1 text-xs text-muted-foreground"
                htmlFor="settings-approval-retention-days"
              >
                {t('approvalRetentionDays')}
                {retentionOrigin === 'usageRetention' ? (
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
                  if (parsed !== null) setApprovalRetentionDays(parsed)
                }}
                step={1}
                type="number"
                value={usageRetention.approval_retention_days ?? ''}
              />
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <p className="m-0 text-xs text-muted-foreground">
              {t('contentLoggingOff')}
            </p>
            {failed.requestContentLogging ? (
              <RetrySection form="requestContentLogging" label={t('retry')} />
            ) : null}
          </div>
        </div>
      </SettingsCard>

      <SettingsCard icon={ShieldCheck} title={t('modelRouteWhitelist')}>
        <div className="grid min-w-0 gap-2">
          <div className="flex items-center gap-2">
            <Switch
              aria-label={t('modelRouteWhitelist')}
              checked={modelRouteWhitelist.enabled}
              disabled={!controlsReady || whitelistPending}
              id="settings-model-route-whitelist-enabled"
              name="settings-model-route-whitelist-enabled"
              onCheckedChange={(checked) =>
                setModelRouteWhitelistEnabled(checked === true)
              }
            />
            {whitelistPending ? <PendingSpinner label={t('loading')} /> : null}
            {failed.modelRouteWhitelist ? (
              <RetrySection form="modelRouteWhitelist" label={t('retry')} />
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
            <Switch
              aria-label={t('streamDeltaBatching')}
              checked={streamDeltaBatching.enabled}
              disabled={!controlsReady || streamPending}
              id="settings-stream-delta-batching-enabled"
              name="settings-stream-delta-batching-enabled"
              onCheckedChange={(checked) =>
                setStreamDeltaBatching({ enabled: checked === true })
              }
            />
            {streamPending ? <PendingSpinner label={t('loading')} /> : null}
            {failed.streamDeltaBatching ? (
              <RetrySection form="streamDeltaBatching" label={t('retry')} />
            ) : null}
          </div>
          <fieldset
            aria-busy={streamPending}
            className="m-0 min-w-0 border-0 p-0"
            disabled={!controlsReady || streamPending}
          >
            <StreamDeltaBatchingFields
              disabled={!controlsReady || streamPending}
              form={streamDeltaBatching}
              onChange={setStreamDeltaBatching}
            />
          </fieldset>
        </div>
      </SettingsCard>
    </div>
  )
}
