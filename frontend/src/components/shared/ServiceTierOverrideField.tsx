import { Input } from '@/components/ui/input'
import { SettingsFieldRow } from '@/components/shared/SettingsFieldRow'
import { useLocale } from '@/lib/i18n'
import { normalizeServiceTier } from '@/admin-mappers/forms/endpoint'

type ServiceTierOverrideFieldProps = {
  onChange: (value: string | null) => void
  value: string | null
}

export function ServiceTierOverrideField({
  onChange,
  value,
}: ServiceTierOverrideFieldProps) {
  const { t } = useLocale()

  return (
    <SettingsFieldRow
      hint={t('serviceTierHint')}
      label={t('serviceTier')}
      htmlFor="endpoint-service-tier"
    >
      <Input
        id="endpoint-service-tier"
        onChange={(event) =>
          onChange(normalizeServiceTier(event.currentTarget.value))
        }
        placeholder={t('proxyInherit')}
        value={value ?? ''}
      />
    </SettingsFieldRow>
  )
}
