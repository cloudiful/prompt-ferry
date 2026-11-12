import { useQuery } from '@tanstack/react-query'
import { Badge } from '@/components/ui/badge'
import { tokenPlanUsageOptions } from '@/generated/admin-api/@tanstack/react-query.gen'
import {
  TOKEN_PLAN_CACHE_TTL_MS,
  tokenPlanUsageSnapshot,
} from '@/lib/quota-cache'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { useLocale } from '@/lib/i18n'
import {
  computeTokenPlanBadges,
  tokenPlanBadgePills,
} from '@/models/endpoints/quota-badges'

export type QuotaBadgesProps = {
  density: 'table' | 'card'
  eligible: boolean
  endpointId: string
}

export function QuotaBadges({
  density,
  eligible,
  endpointId,
}: QuotaBadgesProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const usageQuery = useQuery({
    ...tokenPlanUsageOptions({ path: { endpoint_id: endpointId } }),
    enabled: eligible && endpointId !== '',
    staleTime: TOKEN_PLAN_CACHE_TTL_MS,
    refetchOnWindowFocus: true,
    refetchInterval: TOKEN_PLAN_CACHE_TTL_MS,
  })

  if (!eligible) return null

  const source = computeTokenPlanBadges(tokenPlanUsageSnapshot(usageQuery))
  const pills = tokenPlanBadgePills(
    source,
    t as unknown as TranslateFn,
    locale,
    timeZone.effective,
  )
  if (pills.length === 0)
    return <span className="text-muted-foreground">—</span>

  const wrapper = 'flex min-w-0 flex-wrap items-center gap-1'
  const pill =
    density === 'table'
      ? 'max-w-full truncate px-1.5 py-px text-[0.7rem]'
      : 'max-w-full truncate px-2 py-px text-[0.74rem]'

  return (
    <span className={wrapper} data-quota-badges={density}>
      {pills.map((item, index) => (
        <Badge
          className={pill}
          key={`${index}:${item.label}`}
          style={item.color ? { color: item.color } : undefined}
          title={item.title}
          variant="outline"
        >
          {item.label}
        </Badge>
      ))}
    </span>
  )
}
