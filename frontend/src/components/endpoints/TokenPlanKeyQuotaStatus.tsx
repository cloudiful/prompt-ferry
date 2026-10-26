import { Badge } from '@/components/ui/badge'
import type { TokenPlanQuotaStatus } from '@/generated/admin-api'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { useLocale } from '@/lib/i18n'
import { formatDateTime } from '@/format/datetime-core'

export type TokenPlanKeyQuotaStatusProps = {
  status: TokenPlanQuotaStatus | null
}

export function TokenPlanKeyQuotaStatus({
  status,
}: TokenPlanKeyQuotaStatusProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const state = status?.state ?? 'unknown'
  const label =
    state === 'available'
      ? t('tokenPlanAvailable')
      : state === 'exhausted'
        ? t('tokenPlanUnavailable')
        : state === 'cooling'
          ? t('tokenPlanQuotaCooling')
          : state === 'rate_limited'
            ? t('tokenPlanRateLimited')
            : state === 'upstream_unavailable'
              ? t('tokenPlanUpstreamUnavailable')
              : t('tokenPlanQuotaUnknown')
  const observedAt = status?.observed_at
  const cooldownUntil = status?.cooldown_until
  const hints = [t('tokenPlanDefaultTextScope')]
  if (observedAt) {
    hints.push(
      t('quotaObservedAt', {
        time: formatDateTime(
          observedAt,
          locale,
          { dateStyle: 'medium', timeStyle: 'short' },
          timeZone.effective,
        ),
      }),
    )
  }
  if (status?.refresh_failed) hints.push(t('quotaRefreshFailedCached'))
  if (cooldownUntil) {
    hints.push(
      t('quotaRetryAt', {
        time: formatDateTime(
          cooldownUntil,
          locale,
          { dateStyle: 'medium', timeStyle: 'short' },
          timeZone.effective,
        ),
      }),
    )
  }
  const tone =
    state === 'available'
      ? 'text-emerald-700 dark:text-emerald-300'
      : state === 'exhausted'
        ? 'text-red-700 dark:text-red-300'
        : 'text-muted-foreground'

  return (
    <Badge
      className={tone}
      data-quota-key-status={state}
      title={hints.join(' · ')}
      variant="outline"
    >
      {label}
    </Badge>
  )
}
