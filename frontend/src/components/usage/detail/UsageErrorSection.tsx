import { Button } from '@/components/ui/button'
import { copyText } from '@/lib/clipboard'
import { useLocale } from '@/lib/i18n'
import {
  usageAbortFromStateKey,
  usageAbortReasonKey,
  usageResponseStartedKey,
  type UsageDetailView,
} from '@/lib/usage-detail-model'
import { UsagePreviewText } from './UsagePreviewText'

type UsageErrorSectionProps = {
  detail: UsageDetailView | null
  resetKey: string | number
}

/**
 * The shared error and abort block.
 *
 * The summary and the upstream body each have their own copy button that only
 * reports a local copied state; a copy failure stays retryable and the payload
 * never reaches a notification or a log.
 */
export function UsageErrorSection({
  detail,
  resetKey,
}: UsageErrorSectionProps) {
  const { t } = useLocale()

  if (!detail) return null
  const aborted = detail.request_state === 'aborted'
  const errorText = detail.error_message
    ? detail.error_code
      ? `${detail.error_code}: ${detail.error_message}`
      : detail.error_message
    : ''
  const upstreamBody = detail.upstream_error_body || ''

  if (!detail.error_message && !upstreamBody && !aborted) return null

  function copy(text: string): void {
    if (!text) return
    // A failed copy stays local and retryable: no notification carries the
    // payload and no log line records it.
    void copyText(text).catch(() => {})
  }

  return (
    <section
      className="grid gap-3 rounded border bg-muted p-3"
      data-usage-error-section
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-xs font-semibold text-muted-foreground">
          {t('errorDetails')}
        </h2>
        <div className="flex flex-wrap items-center gap-2">
          {detail.error_message && (
            <Button
              onClick={() => copy(errorText)}
              size="sm"
              type="button"
              variant="ghost"
            >
              {t('copy')}
            </Button>
          )}
          {upstreamBody && (
            <Button
              onClick={() => copy(upstreamBody)}
              size="sm"
              type="button"
              variant="ghost"
            >
              {t('copy')} {t('upstreamErrorBody')}
            </Button>
          )}
        </div>
      </div>

      {detail.error_message && (
        <div className="grid gap-1">
          <div className="text-xs font-semibold text-muted-foreground">
            {t('errorMessage')}
          </div>
          <div
            className={
              aborted
                ? 'rounded border bg-muted p-3 break-all text-muted-foreground'
                : 'rounded border border-destructive/40 bg-destructive/10 p-3 break-all text-destructive'
            }
            data-usage-error-summary
          >
            {errorText}
          </div>
        </div>
      )}

      {aborted && (
        <div className="grid gap-2">
          <div className="text-xs font-semibold text-muted-foreground">
            {t('abortDetails')}
          </div>
          <div className="grid gap-2 sm:grid-cols-3">
            <AbortField
              label={t('abortReason')}
              value={t(usageAbortReasonKey(detail.abort_reason))}
            />
            <AbortField
              label={t('abortFromState')}
              value={t(usageAbortFromStateKey(detail.abort_from_state))}
            />
            <AbortField
              label={t('abortResponseStarted')}
              value={t(usageResponseStartedKey(detail.abort_response_started))}
            />
          </div>
        </div>
      )}

      {upstreamBody ? (
        <div className="grid gap-2">
          <div className="text-xs font-semibold text-muted-foreground">
            {t('upstreamErrorBody')}
          </div>
          <UsagePreviewText
            allLabel={t('showFullContent')}
            collapseLabel={t('collapseContent')}
            emptyText={t('contentLoggingOff')}
            moreLabel={t('showMoreContent')}
            resetKey={resetKey}
            text={upstreamBody}
            truncatedLabel={t('truncatedPreview')}
          />
        </div>
      ) : (
        (detail.error_message || aborted) && (
          <div className="text-xs text-muted-foreground">
            {t('noUpstreamErrorBody')}
          </div>
        )
      )}
    </section>
  )
}

function AbortField({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid gap-1">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div>{value}</div>
    </div>
  )
}
