import type { RequestRecordState } from '../generated/admin-api'
import type { RequestRecordRowView } from '../models'

export type RequestRecordFormatting = {
  formatCount: (value?: number | null) => string
  formatMs: (value?: number | null) => string
  formatPercent: (value?: number | null) => string
  formatOutputTokensPerSecond: (record: RequestRecordRowView) => string
  formatInputTokensPerSecond: (record: RequestRecordRowView) => string
  formatRequestStateLabel: (state: RequestRecordState) => string
  formatTime: (value: string) => string
  requestStateSeverity: (
    state: RequestRecordState,
  ) => 'secondary' | 'success' | 'warn'
  streamMs: (record: RequestRecordRowView) => number | null
}
