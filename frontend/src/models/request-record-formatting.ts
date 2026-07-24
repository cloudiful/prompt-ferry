import type { RequestRecordState } from '../generated/admin-api'
import type { RequestRecordRowView } from '../models'

export type RequestRecordTiming = Pick<
  RequestRecordRowView,
  | 'request_category'
  | 'request_state'
  | 'duration_ms'
  | 'ttft_ms'
  | 'input_tokens'
  | 'output_tokens'
>

export type RequestRecordFormatting = {
  formatCount: (value?: number | null) => string
  formatMs: (value?: number | null) => string
  formatPercent: (value?: number | null) => string
  formatOutputTokensPerSecond: (record: RequestRecordTiming) => string
  formatOutputRateMode: (
    record: RequestRecordTiming,
  ) => 'generation' | 'e2e' | null
  formatInputTokensPerSecond: (record: RequestRecordTiming) => string
  formatRequestStateLabel: (state: RequestRecordState) => string
  formatTime: (value: string) => string
  requestStateSeverity: (
    state: RequestRecordState,
  ) => 'secondary' | 'success' | 'warn'
  streamMs: (record: RequestRecordTiming) => number | null
}
