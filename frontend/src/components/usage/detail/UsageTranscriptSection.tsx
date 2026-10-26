import { useEffect, useRef } from 'react'
import type { RequestRecordFullResponse } from '@/generated/admin-api'
import { useLocale } from '@/lib/i18n'
import { isRequestRecordTerminal } from '@/request-records'
import type { UsageDetailView } from '@/lib/usage-detail-model'
import { UsageTranscriptTimeline } from './UsageTranscriptTimeline'

type UsageTranscriptSectionProps = {
  detail: UsageDetailView | null
  loading: boolean
  loadingMore: boolean
  onLoadFirst: () => void
  onLoadMore: () => void
  requestFull: RequestRecordFullResponse | null
  visible: boolean
}

/**
 * The transcript wrapper.
 *
 * The first page is requested only while the dialog is visible, the record
 * says it has a stored request and nothing is loaded or loading; the store
 * coalesces any repeat, so the auto-trigger cannot issue a duplicate read.
 */
export function UsageTranscriptSection({
  detail,
  loading,
  loadingMore,
  onLoadFirst,
  onLoadMore,
  requestFull,
  visible,
}: UsageTranscriptSectionProps) {
  const { t } = useLocale()
  const loadFirst = useRef(onLoadFirst)
  loadFirst.current = onLoadFirst

  const recordId = detail?.record_id ?? null
  const hasFullRequest = detail?.has_full_request === true

  useEffect(() => {
    if (!visible || !hasFullRequest) return
    if (requestFull || loading) return
    loadFirst.current()
  }, [visible, hasFullRequest, recordId, requestFull, loading])

  const responsePendingText =
    detail && !isRequestRecordTerminal(detail.request_state)
      ? t('processingResponsePending')
      : ''

  return (
    <UsageTranscriptTimeline
      detail={detail}
      loading={loading}
      loadingMore={loadingMore}
      onLoadMore={onLoadMore}
      requestFull={requestFull}
      responsePendingText={responsePendingText}
    />
  )
}
