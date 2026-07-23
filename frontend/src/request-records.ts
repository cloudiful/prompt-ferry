import type { RequestRecordState } from './generated/admin-api'
import type { MessageKey } from './i18n'

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
): 'secondary' | 'success' | 'warn' {
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
