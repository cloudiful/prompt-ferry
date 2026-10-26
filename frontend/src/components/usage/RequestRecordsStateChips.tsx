import type { RequestRecordState } from '@/generated/admin-api'
import { Badge } from '@/components/ui/badge'
import { useLocale } from '@/lib/i18n'
import {
  requestRecordStateBadges,
  type RequestRecordStateSeverity,
} from '@/request-records'

export type UsageStateBadgeVariant = 'default' | 'secondary' | 'outline'

/**
 * The shared severity-to-Badge-variant mapping: a neutral state stays
 * secondary, a completed request is the filled default badge and a failure
 * keeps the outlined warning treatment. The state label is always rendered, so
 * the colour is never the only signal.
 */
export function usageStateBadgeVariant(
  severity: RequestRecordStateSeverity | 'neutral',
): UsageStateBadgeVariant {
  if (severity === 'success') return 'default'
  if (severity === 'warn') return 'outline'
  return 'secondary'
}

type RequestRecordsStateChipsProps = {
  record: {
    request_category: 'ai' | 'mcp'
    request_state: RequestRecordState
    ttft_ms?: number | null
  }
}

export function RequestRecordsStateChips({
  record,
}: RequestRecordsStateChipsProps) {
  const { t } = useLocale()
  return (
    <span className="inline-flex items-center gap-1 whitespace-nowrap">
      {requestRecordStateBadges(record).map((chip) => (
        <Badge
          data-usage-state-chip={chip.labelKey}
          key={chip.id}
          variant={usageStateBadgeVariant(chip.color)}
        >
          {t(chip.labelKey)}
        </Badge>
      ))}
    </span>
  )
}
