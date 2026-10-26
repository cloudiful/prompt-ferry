import { useCallback, useEffect, useRef, useState } from 'react'
import { LoaderCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import type {
  QuotaSnapshotHistoryItem,
  SubscriptionWindowUsage,
} from '@/generated/admin-api'
import { useLocale } from '@/lib/i18n'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { formatDateTime } from '@/format/datetime-core'
import { formatResetDuration } from '@/models/endpoints/token-plan-window'
import {
  subscriptionWindowLabel,
  subscriptionWindowProgressColor,
  subscriptionWindowRemainingPercent,
  subscriptionWindowResetMs,
} from '@/models/endpoints/subscription-window'
import { fetchQuotaSnapshotHistory } from '@/stores/endpoints-api'

const PAGE_SIZE = 50
const DEFAULT_RETENTION_DAYS = 30

type HistoryPage = Awaited<ReturnType<typeof fetchQuotaSnapshotHistory>>

function uniqueSnapshots(
  items: QuotaSnapshotHistoryItem[],
): QuotaSnapshotHistoryItem[] {
  const seen = new Set<string>()
  return items.filter((item) => {
    if (seen.has(item.snapshot_id)) return false
    seen.add(item.snapshot_id)
    return true
  })
}

function sourceLabel(source: string, t: TranslateFn): string {
  switch (source) {
    case 'manual':
      return t('quotaSourceManual')
    case 'request':
      return t('quotaSourceRequest')
    case 'periodic':
      return t('quotaSourcePeriodic')
    default:
      return t('quotaSourceUnknown')
  }
}

function SnapshotWindowRow({
  nowMs,
  observationMs,
  window,
}: {
  nowMs: number
  observationMs?: number
  window: SubscriptionWindowUsage
}) {
  const { t } = useLocale()
  const remaining = subscriptionWindowRemainingPercent(window)
  const resetMs = subscriptionWindowResetMs(window, nowMs, observationMs)
  const translate = t as unknown as TranslateFn
  const label = subscriptionWindowLabel(window, translate)
  const percentage = remaining === null ? null : remaining
  const resetLabel = formatResetDuration(resetMs, translate)

  return (
    <div
      className="grid min-w-0 gap-1.5 sm:grid-cols-[minmax(7rem,auto)_minmax(0,1fr)_minmax(8.5rem,auto)] sm:items-center sm:gap-3"
      data-subscription-window={window.source_window}
    >
      <span className="break-words text-muted-foreground">{label}</span>
      {percentage === null ? (
        <span aria-hidden="true" className="h-1.5" />
      ) : (
        <div
          aria-label={label}
          aria-valuemax={100}
          aria-valuemin={0}
          aria-valuenow={percentage}
          className="h-1.5 w-full overflow-hidden rounded-full bg-muted"
          role="progressbar"
        >
          <div
            className="h-full rounded-full"
            style={{
              backgroundColor: subscriptionWindowProgressColor(percentage),
              width: `${percentage}%`,
            }}
          />
        </div>
      )}
      <div className="flex min-w-0 items-center justify-between gap-2 text-xs sm:justify-end">
        <span className="text-muted-foreground">{resetLabel}</span>
        <span className="shrink-0 font-semibold">
          {percentage === null
            ? '—'
            : `${t('quotaRemainingPercent')} ${percentage.toFixed(1)}%`}
        </span>
      </div>
    </div>
  )
}

export type SubscriptionQuotaHistoryProps = {
  endpointId: string
  nowMs: number
  refreshKey: string
}

export function SubscriptionQuotaHistory({
  endpointId,
  nowMs,
  refreshKey,
}: SubscriptionQuotaHistoryProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const [expanded, setExpanded] = useState(false)
  const [items, setItems] = useState<QuotaSnapshotHistoryItem[]>([])
  const [nextCursor, setNextCursor] = useState<string | null>(null)
  const [retentionDays, setRetentionDays] = useState(DEFAULT_RETENTION_DAYS)
  const [loading, setLoading] = useState(false)
  const [loadingMore, setLoadingMore] = useState(false)
  const [failed, setFailed] = useState(false)
  const [loaded, setLoaded] = useState(false)
  const generation = useRef(0)
  const previousRefreshKey = useRef(refreshKey)

  const clear = useCallback(() => {
    generation.current += 1
    setItems([])
    setNextCursor(null)
    setRetentionDays(DEFAULT_RETENTION_DAYS)
    setLoading(false)
    setLoadingMore(false)
    setFailed(false)
    setLoaded(false)
  }, [])

  const loadFirstPage = useCallback(async () => {
    if (!endpointId) return
    const requestGeneration = ++generation.current
    setItems([])
    setNextCursor(null)
    setFailed(false)
    setLoaded(false)
    setLoadingMore(false)
    setLoading(true)
    try {
      const page: HistoryPage = await fetchQuotaSnapshotHistory(
        endpointId,
        PAGE_SIZE,
      )
      if (requestGeneration !== generation.current || endpointId === '') {
        return
      }
      setItems(uniqueSnapshots(page.items))
      setNextCursor(page.next_cursor ?? null)
      setRetentionDays(page.retention_days)
      setLoaded(true)
    } catch {
      if (requestGeneration === generation.current) setFailed(true)
    } finally {
      if (requestGeneration === generation.current) setLoading(false)
    }
  }, [endpointId])

  const loadMore = useCallback(async () => {
    const cursor = nextCursor
    if (!endpointId || !cursor || !loaded || loading || loadingMore) return
    const requestGeneration = generation.current
    setFailed(false)
    setLoadingMore(true)
    try {
      const page = await fetchQuotaSnapshotHistory(
        endpointId,
        PAGE_SIZE,
        cursor,
      )
      if (requestGeneration !== generation.current) return
      setItems((current) => {
        const seen = new Set(current.map((item) => item.snapshot_id))
        return [
          ...current,
          ...page.items.filter((item) => {
            if (seen.has(item.snapshot_id)) return false
            seen.add(item.snapshot_id)
            return true
          }),
        ]
      })
      setNextCursor(page.next_cursor ?? null)
      setRetentionDays(page.retention_days)
    } catch {
      if (requestGeneration === generation.current) setFailed(true)
    } finally {
      if (requestGeneration === generation.current) setLoadingMore(false)
    }
  }, [endpointId, loaded, loading, loadingMore, nextCursor])

  const retry = useCallback(() => {
    if (items.length > 0 && nextCursor) void loadMore()
    else void loadFirstPage()
  }, [items.length, loadFirstPage, loadMore, nextCursor])

  useEffect(() => {
    if (expanded) void loadFirstPage()
    else clear()
  }, [expanded, endpointId, clear, loadFirstPage])

  useEffect(() => {
    if (previousRefreshKey.current === refreshKey) return
    previousRefreshKey.current = refreshKey
    if (expanded && (loaded || failed)) void loadFirstPage()
  }, [refreshKey, expanded, loaded, failed, loadFirstPage])

  useEffect(
    () => () => {
      generation.current += 1
    },
    [],
  )

  return (
    <details
      className="min-w-0 rounded-md border"
      onToggle={(event) => setExpanded(event.currentTarget.open)}
      open={expanded}
    >
      <summary className="cursor-pointer list-none px-3 py-2 font-medium [&::-webkit-details-marker]:hidden">
        {t('quotaSnapshotHistory')}
      </summary>
      <div className="grid min-w-0 gap-3 border-t p-3">
        {loading ? (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <LoaderCircle aria-hidden="true" className="size-4 animate-spin" />
            <span>{t('quotaHistoryLoading')}</span>
          </div>
        ) : (
          <>
            {failed ? (
              <p className="text-sm text-destructive">
                {t('quotaHistoryError')}{' '}
                <Button onClick={retry} size="sm" type="button" variant="link">
                  {t('quotaHistoryRetry')}
                </Button>
              </p>
            ) : null}
            {loaded && items.length === 0 && !failed ? (
              <p className="text-sm text-muted-foreground">
                {t('quotaHistoryEmpty', { days: retentionDays })}
              </p>
            ) : null}
            {items.length > 0 ? (
              <div className="grid min-w-0 gap-2">
                {items.map((item) => {
                  const observationMs = Date.parse(item.observed_at)
                  const safeObservationMs = Number.isFinite(observationMs)
                    ? observationMs
                    : undefined
                  return (
                    <article
                      className="grid min-w-0 gap-2 rounded-md border p-3"
                      data-quota-snapshot-id={item.snapshot_id}
                      key={item.snapshot_id}
                    >
                      <header className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 text-xs">
                        <time className="font-medium">
                          {formatDateTime(
                            item.observed_at,
                            locale,
                            { dateStyle: 'medium', timeStyle: 'short' },
                            timeZone.effective,
                          )}
                        </time>
                        {item.plan_type ? (
                          <span className="break-words text-muted-foreground">
                            {item.plan_type}
                          </span>
                        ) : null}
                        <span className="rounded-full border px-2 py-0.5 text-muted-foreground">
                          {sourceLabel(
                            item.source,
                            t as unknown as TranslateFn,
                          )}
                        </span>
                        {item.limit_reached ? (
                          <span className="text-amber-700 dark:text-amber-300">
                            {t('quotaHistoryLimitReached')}
                          </span>
                        ) : null}
                      </header>
                      {item.windows.length > 0 ? (
                        <div className="grid min-w-0 gap-2">
                          {item.windows.map((window, index) => (
                            <SnapshotWindowRow
                              key={`${window.source_window}:${index}`}
                              nowMs={nowMs}
                              observationMs={safeObservationMs}
                              window={window}
                            />
                          ))}
                        </div>
                      ) : (
                        <span className="text-sm text-muted-foreground">
                          {t('quotaHistoryNoWindowDetails')}
                        </span>
                      )}
                    </article>
                  )
                })}
              </div>
            ) : null}
            {nextCursor ? (
              <Button
                disabled={loadingMore}
                onClick={() => void loadMore()}
                type="button"
                variant="outline"
              >
                {loadingMore ? (
                  <LoaderCircle
                    aria-hidden="true"
                    className="size-4 animate-spin"
                  />
                ) : null}
                {t('quotaHistoryLoadMore')}
              </Button>
            ) : null}
          </>
        )}
      </div>
    </details>
  )
}
