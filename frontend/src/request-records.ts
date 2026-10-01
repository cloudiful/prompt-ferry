import type { RequestRecordState } from './generated/admin-api'
import type { MessageKey } from './i18n'
import type { RequestRecordTiming } from './models/request-record-formatting'

export type RequestRecordStateSeverity = 'secondary' | 'success' | 'warn'

export type RequestRecordStateBadge = {
  id: 'state'
  labelKey: MessageKey
  color: RequestRecordStateSeverity | 'neutral'
  variant?: 'subtle'
}

export function isRequestRecordTerminal(
  state?: RequestRecordState | null,
): boolean {
  return state === 'completed' || state === 'failed' || state === 'aborted'
}

export function requestRecordStateLabelKey(
  state: RequestRecordState,
): MessageKey {
  switch (state) {
    case 'received':
      return 'requestStateReceived'
    case 'awaiting_approval':
      return 'requestStateAwaitingApproval'
    case 'upstream_processing':
      return 'requestStateUpstreamProcessing'
    case 'completed':
      return 'requestStateCompleted'
    case 'failed':
      return 'requestStateFailed'
    case 'aborted':
      return 'requestStateAborted'
  }
}

export function requestRecordStateTagSeverity(
  state: RequestRecordState,
): RequestRecordStateSeverity {
  switch (state) {
    case 'received':
      return 'secondary'
    case 'awaiting_approval':
      return 'secondary'
    case 'upstream_processing':
      return 'secondary'
    case 'completed':
      return 'success'
    case 'failed':
      return 'warn'
    case 'aborted':
      return 'warn'
  }
}

/**
 * Issue #657 P2: the status capsule of one request row, shared by the AI HTTP
 * and MCP lists. A running `upstream_processing` row reads as waiting for a
 * response until the first meaningful output is recorded (`ttft_ms`), and as
 * streaming output afterwards; a terminal row keeps its own state.
 */
export function requestRecordStateBadges(
  record: RequestRecordTiming,
): RequestRecordStateBadge[] {
  const state = record.request_state
  return [
    {
      id: 'state',
      labelKey:
        state === 'upstream_processing'
          ? runningRequestStateLabelKey(record)
          : requestRecordStateLabelKey(state),
      color: requestRecordStateTagSeverity(state),
    },
  ]
}

/** Issue #657 P2: `ttft_ms` splits the running state into the two phases. */
function runningRequestStateLabelKey(record: RequestRecordTiming): MessageKey {
  return record.ttft_ms == null
    ? 'requestStateWaitingForResponse'
    : 'requestStateStreamingOutput'
}
