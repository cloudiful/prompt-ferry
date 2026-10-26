import type { RequestRecordListRow } from '@/generated/admin-api'
import {
  deriveModelDisplay,
  upstreamLabel,
} from '@/admin-mappers/request-records-core'

/**
 * A list row with its display-only labels.
 *
 * The date is deliberately not derived here: the React table formats both date
 * columns at render time from the current locale and display time zone, so a
 * zone change repaints without refetching.
 */
export type UsageRecordView = RequestRecordListRow & {
  actor: string
  model_key: string
  model_display: string
  target: string
  upstream_label: string
  session_short_id: string
  is_session_recognized: boolean
  is_first_turn: boolean
  user_key: string
}

export function createUsageRecordView(
  record: RequestRecordListRow,
): UsageRecordView {
  const label = upstreamLabel(record)
  const sessionRecognized = Boolean(record.conversation_id)
  return {
    ...record,
    actor: record.user_login_name || '-',
    is_first_turn: sessionRecognized && (record.conversation_seq ?? 1) <= 1,
    is_session_recognized: sessionRecognized,
    model_key: record.model || '-',
    model_display: deriveModelDisplay(record),
    session_short_id: record.conversation_id?.slice(0, 8) || '-',
    target: label,
    upstream_label: label,
    user_key: record.user_login_name || '-',
  }
}

/**
 * The end-to-end output rate is only meaningful for a completed AI request that
 * produced output over a positive duration; everything else is unknown.
 */
export function usageOutputRate(
  record: Pick<
    RequestRecordListRow,
    'request_category' | 'request_state' | 'output_tokens' | 'duration_ms'
  >,
): number | null {
  if (record.request_category !== 'ai') return null
  if (record.request_state !== 'completed') return null
  const output = record.output_tokens
  const duration = record.duration_ms
  if (output == null || output <= 0 || duration == null || duration <= 0) {
    return null
  }
  const rate = output / (duration / 1000)
  return Number.isFinite(rate) && rate > 0 ? rate : null
}

/** The rate above, rounded like the existing formatter, or `-`. */
export function formatUsageOutputRate(
  record: Parameters<typeof usageOutputRate>[0],
): string {
  const rate = usageOutputRate(record)
  if (rate === null) return '-'
  return rate >= 100 ? rate.toFixed(0) : rate.toFixed(1)
}
