import { useEffect, useRef } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import {
  HardDrive,
  LoaderCircle,
  RefreshCw,
  Save,
  TriangleAlert,
} from 'lucide-react'
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
import { SettingsStorageCredentials } from '@/components/settings/SettingsStorageCredentials'
import type { RawObjectStoreBackend } from '@/generated/admin-api'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import {
  disposeSettingsStorage,
  setSettingsStorageNotifier,
  useSettingsStorageStore,
} from '@/lib/settings-storage'
import { BACKEND_OPTIONS, STORAGE_KEY } from '@/lib/settings-storage-model'
import { useAsyncAction } from '@/lib/use-async-action'

type TextFieldProps = {
  disabled: boolean
  help?: string
  id: string
  label: string
  onChange: (value: string) => void
  placeholder?: string
  value: string
}

function TextField({
  disabled,
  help,
  id,
  label,
  onChange,
  placeholder,
  value,
}: TextFieldProps) {
  return (
    <div className="grid min-w-0 gap-1">
      <Label className="text-xs font-medium" htmlFor={id}>
        {label}
      </Label>
      <Input
        autoComplete="off"
        className="h-8 text-sm"
        disabled={disabled}
        id={id}
        name={id}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        value={value}
      />
      {help ? (
        <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
          {help}
        </p>
      ) : null}
    </div>
  )
}

/**
 * The admin storage settings: the raw payload store.
 *
 * The card is one setting with three shapes. Local keeps one directory field,
 * S3 keeps the endpoint/bucket/region/prefix group with its two switches and the
 * two write-only credentials, and Disabled keeps only its explanation. The
 * backend select and Save live in the card header, the save is manual, and an
 * unavailable capability replaces the form with the warning and its retry read.
 */
export function SettingsStorageTab() {
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const { notifyApiError, notifyError, notifySuccess } = useNotifier()
  const { isPending, pendingOrigin } = useAsyncAction()

  const form = useSettingsStorageStore((state) => state.form)
  const presence = useSettingsStorageStore((state) => state.presence)
  const loaded = useSettingsStorageStore((state) => state.loaded)
  const loading = useSettingsStorageStore((state) => state.loading)
  const unavailable = useSettingsStorageStore((state) => state.unavailable)
  const unavailableMessage = useSettingsStorageStore(
    (state) => state.unavailableMessage,
  )
  const refresh = useSettingsStorageStore((state) => state.refresh)
  const setField = useSettingsStorageStore((state) => state.setField)
  const save = useSettingsStorageStore((state) => state.save)

  const savedMessage = useRef<() => void>(() => {})
  savedMessage.current = () => notifySuccess(t('storageSaved'))

  useEffect(() => {
    setSettingsStorageNotifier({
      notifyApiError: (cause) => notifyApiError(cause),
      notifyBucketRequired: () => notifyError(t('storageBucketRequired')),
      notifySaved: () => savedMessage.current(),
    })
    return () => {
      setSettingsStorageNotifier(null)
      disposeSettingsStorage()
    }
  }, [notifyApiError, notifyError, t])

  const savePending = isPending(STORAGE_KEY)
  const saveOrigin = pendingOrigin(STORAGE_KEY)
  const disabled = !loaded || unavailable || savePending

  const backendOptions = BACKEND_OPTIONS.map((option) => ({
    label: t(option.labelKey),
    value: option.value,
  }))

  return (
    <SettingsCard icon={HardDrive} title={t('storageTitle')}>
      <div className="grid min-w-0 gap-3">
        <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
          <div className="flex min-w-0 items-center gap-2">
            <Label className="sr-only" htmlFor="settings-storage-backend">
              {t('storageTitle')}
            </Label>
            <Select
              onValueChange={(value) =>
                setField('backend', value as RawObjectStoreBackend)
              }
              value={form.backend}
            >
              <SelectTrigger
                aria-label={t('storageTitle')}
                className="w-full min-w-32"
                disabled={disabled}
                id="settings-storage-backend"
                name="settings-storage-backend"
                size="sm"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {backendOptions.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex items-center gap-2">
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
            {saveOrigin === 'storage' ? (
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
              id="settings-storage-save"
              onClick={() => void save()}
              size="sm"
              type="button"
            >
              <Save aria-hidden="true" />
              {t('save')}
            </Button>
          </div>
        </div>

        {unavailable ? (
          <div className="grid min-w-0 gap-1 rounded-md border border-warning/30 bg-warning/10 px-3 py-2">
            <p className="m-0 flex items-center gap-1.5 text-xs leading-relaxed text-muted-foreground">
              <TriangleAlert
                aria-hidden="true"
                className="size-3.5 shrink-0 text-warning"
              />
              {t('storageUnavailable')}
            </p>
            {unavailableMessage ? (
              <p className="m-0 text-[11px] text-muted-foreground">
                {unavailableMessage}
              </p>
            ) : null}
            <div className="mt-1">
              <Button
                id="settings-storage-retry"
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
            {form.backend === 'local' ? (
              <TextField
                disabled={disabled}
                help={t('storageLocalHint')}
                id="settings-storage-local-dir"
                label={t('storageLocalDir')}
                onChange={(value) => setField('local_dir', value)}
                placeholder={t('storageLocalDirPlaceholder')}
                value={form.local_dir}
              />
            ) : null}

            {form.backend === 's3' ? (
              <div className="grid min-w-0 gap-3">
                <div className="grid min-w-0 gap-3 sm:grid-cols-2">
                  <TextField
                    disabled={disabled}
                    id="settings-storage-s3-endpoint"
                    label={t('storageS3Endpoint')}
                    onChange={(value) => setField('s3_endpoint', value)}
                    placeholder={t('storageS3EndpointPlaceholder')}
                    value={form.s3_endpoint}
                  />
                  <TextField
                    disabled={disabled}
                    id="settings-storage-s3-bucket"
                    label={`${t('storageS3Bucket')} *`}
                    onChange={(value) => setField('s3_bucket', value)}
                    placeholder={t('storageS3BucketPlaceholder')}
                    value={form.s3_bucket}
                  />
                  <TextField
                    disabled={disabled}
                    id="settings-storage-s3-region"
                    label={t('storageS3Region')}
                    onChange={(value) => setField('s3_region', value)}
                    placeholder={t('storageS3RegionPlaceholder')}
                    value={form.s3_region}
                  />
                  <TextField
                    disabled={disabled}
                    id="settings-storage-s3-prefix"
                    label={t('storageS3Prefix')}
                    onChange={(value) => setField('s3_prefix', value)}
                    placeholder={t('storageS3PrefixPlaceholder')}
                    value={form.s3_prefix}
                  />
                </div>

                <div className="grid min-w-0 gap-2 sm:grid-cols-2">
                  <div className="flex min-w-0 items-center gap-2">
                    <Switch
                      aria-label={t('storageS3AllowHttp')}
                      checked={form.s3_allow_http}
                      disabled={disabled}
                      id="settings-storage-s3-allow-http"
                      name="settings-storage-s3-allow-http"
                      onCheckedChange={(checked) =>
                        setField('s3_allow_http', checked === true)
                      }
                    />
                    <span className="inline-flex min-w-0 flex-col">
                      <span className="text-xs font-medium">
                        {t('storageS3AllowHttp')}
                      </span>
                      <span className="text-[11px] text-muted-foreground">
                        {t('storageS3AllowHttpHelp')}
                      </span>
                    </span>
                  </div>
                  <div className="flex min-w-0 items-center gap-2">
                    <Switch
                      aria-label={t('storageS3PathStyle')}
                      checked={form.s3_path_style}
                      disabled={disabled}
                      id="settings-storage-s3-path-style"
                      name="settings-storage-s3-path-style"
                      onCheckedChange={(checked) =>
                        setField('s3_path_style', checked === true)
                      }
                    />
                    <span className="inline-flex min-w-0 flex-col">
                      <span className="text-xs font-medium">
                        {t('storageS3PathStyle')}
                      </span>
                      <span className="text-[11px] text-muted-foreground">
                        {t('storageS3PathStyleHelp')}
                      </span>
                    </span>
                  </div>
                </div>

                <SettingsStorageCredentials
                  disabled={disabled}
                  onChangeAccessKey={(value) =>
                    setField('s3_access_key_draft', value)
                  }
                  onChangeSecretKey={(value) =>
                    setField('s3_secret_key_draft', value)
                  }
                  presence={presence}
                  valueAccessKey={form.s3_access_key_draft}
                  valueSecretKey={form.s3_secret_key_draft}
                />
                <p className="m-0 text-[11px] text-muted-foreground">
                  {t('storageSecretHint')}
                </p>
              </div>
            ) : null}

            {form.backend === 'disabled' ? (
              <p className="m-0 text-xs text-muted-foreground">
                {t('storageDisabledHint')}
              </p>
            ) : null}
          </div>
        )}
      </div>
    </SettingsCard>
  )
}
