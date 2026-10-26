import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useLocale } from '@/lib/i18n'
import type { RawObjectStorePresence } from '@/lib/settings-storage-model'

type SettingsStorageCredentialsProps = {
  disabled: boolean
  onChangeAccessKey: (value: string) => void
  onChangeSecretKey: (value: string) => void
  presence: RawObjectStorePresence
  valueAccessKey: string
  valueSecretKey: string
}

function PresenceBadge({
  present,
  label,
}: {
  present: boolean
  label: string
}) {
  return (
    <span
      className={
        present
          ? 'rounded border border-primary/40 px-1.5 py-0.5 text-[10px] text-primary'
          : 'rounded border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground'
      }
      data-secret-present={present ? 'true' : 'false'}
    >
      {label}
    </span>
  )
}

/**
 * The two S3 credentials.
 *
 * The stored values are write-only: the server reports presence only, so each
 * field shows a presence badge plus a password input for a replacement. A blank
 * input keeps the stored value, which is what the hint under the pair says — the
 * form has no clear control.
 */
export function SettingsStorageCredentials({
  disabled,
  onChangeAccessKey,
  onChangeSecretKey,
  presence,
  valueAccessKey,
  valueSecretKey,
}: SettingsStorageCredentialsProps) {
  const { t } = useLocale()
  const presentLabel = t('storageSecretPresent')
  const missingLabel = t('storageSecretMissing')

  return (
    <div className="grid min-w-0 gap-3 sm:grid-cols-2">
      <div className="grid min-w-0 gap-1">
        <div className="flex min-w-0 items-center gap-1.5">
          <Label
            className="text-xs font-medium"
            htmlFor="settings-storage-s3-access-key"
          >
            {t('storageS3AccessKey')}
          </Label>
          <PresenceBadge
            label={presence.has_s3_access_key ? presentLabel : missingLabel}
            present={presence.has_s3_access_key}
          />
        </div>
        <Input
          autoComplete="off"
          className="h-8 text-sm"
          disabled={disabled}
          id="settings-storage-s3-access-key"
          name="settings-storage-s3-access-key"
          onChange={(event) => onChangeAccessKey(event.target.value)}
          placeholder={t('storageSecretPlaceholder')}
          type="password"
          value={valueAccessKey}
        />
      </div>
      <div className="grid min-w-0 gap-1">
        <div className="flex min-w-0 items-center gap-1.5">
          <Label
            className="text-xs font-medium"
            htmlFor="settings-storage-s3-secret-key"
          >
            {t('storageS3SecretKey')}
          </Label>
          <PresenceBadge
            label={presence.has_s3_secret_key ? presentLabel : missingLabel}
            present={presence.has_s3_secret_key}
          />
        </div>
        <Input
          autoComplete="off"
          className="h-8 text-sm"
          disabled={disabled}
          id="settings-storage-s3-secret-key"
          name="settings-storage-s3-secret-key"
          onChange={(event) => onChangeSecretKey(event.target.value)}
          placeholder={t('storageSecretPlaceholder')}
          type="password"
          value={valueSecretKey}
        />
      </div>
    </div>
  )
}
