import { useEffect, useMemo, useState } from 'react'
import { ChevronRight } from 'lucide-react'
import { Button } from '@/components/ui/button'
import type { RequestRecordFullResponse } from '@/generated/admin-api'
import { useLocale } from '@/lib/i18n'
import {
  createUsageTranscriptItems,
  type UsageDetailView,
} from '@/lib/usage-detail-model'
import { UsageJsonSyntax } from './UsageJsonSyntax'

type UsageTranscriptTimelineProps = {
  detail: UsageDetailView | null
  loading: boolean
  loadingMore: boolean
  onLoadMore: () => void
  requestFull: RequestRecordFullResponse | null
  responsePendingText?: string
}

/**
 * The raw transcript: one expandable item per stored message or assistant
 * payload, paged newest-first.
 *
 * Expanded item ids are pruned when the item set changes, and the sentinel plus
 * the show-more button both request the next page while a single load is in
 * flight, so an observer burst cannot issue duplicates.
 */
export function UsageTranscriptTimeline({
  detail,
  loading,
  loadingMore,
  onLoadMore,
  requestFull,
  responsePendingText = '',
}: UsageTranscriptTimelineProps) {
  const { t } = useLocale()
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set())
  const [sentinel, setSentinel] = useState<HTMLDivElement | null>(null)

  const items = useMemo(
    () =>
      createUsageTranscriptItems(detail, requestFull, {
        requestRawJson: t('requestRawJson'),
        responsePromptMeta: t('requestStateUpstreamProcessing'),
        sameAsTurn: t('sameAsTurn'),
      }),
    [detail, requestFull, t],
  )
  const itemKey = items.map((item) => item.id).join('|')

  useEffect(() => {
    const available = new Set(items.map((item) => item.id))
    setExpanded((current) => {
      const next = new Set([...current].filter((id) => available.has(id)))
      return next.size === current.size ? current : next
    })
    // The item identity set is the prune trigger.
  }, [itemKey])

  const hasMore = requestFull?.has_more === true
  const total = requestFull?.total_messages ?? null
  const loaded = requestFull?.messages.length ?? 0

  useEffect(() => {
    if (!sentinel || !hasMore || loadingMore) return
    if (typeof IntersectionObserver === 'undefined') return
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries[0]?.isIntersecting) return
        if (loadingMore) return
        onLoadMore()
      },
      { rootMargin: '240px' },
    )
    observer.observe(sentinel)
    return () => observer.disconnect()
  }, [sentinel, hasMore, loadingMore, onLoadMore])

  function toggle(id: string): void {
    setExpanded((current) => {
      const next = new Set(current)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  return (
    <section className="grid gap-3" data-usage-transcript-section>
      <div className="text-xs font-semibold text-muted-foreground">
        {t('rawMessages')}
        {total != null && (
          <span className="font-normal text-muted-foreground">
            {' '}
            ({loaded}/{total})
          </span>
        )}
      </div>

      {loading && !requestFull && items.length === 0 ? (
        <div className="text-xs text-muted-foreground">
          {t('requestFullLoading')}
        </div>
      ) : items.length === 0 ? (
        <div className="text-xs text-muted-foreground">
          {responsePendingText || t('noJsonViewData')}
        </div>
      ) : (
        <div className="grid gap-2">
          {items.map((item) => {
            const open = expanded.has(item.id)
            return (
              <div
                className="overflow-hidden rounded-lg border"
                data-usage-transcript-item={item.id}
                key={item.id}
              >
                <button
                  aria-expanded={open}
                  className="grid w-full grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3 bg-transparent px-4 py-3 text-left"
                  onClick={() => toggle(item.id)}
                  type="button"
                >
                  <ChevronRight
                    aria-hidden="true"
                    className={
                      open
                        ? 'size-4 rotate-90 text-muted-foreground transition-transform'
                        : 'size-4 text-muted-foreground transition-transform'
                    }
                  />
                  <div className="min-w-0 truncate text-sm font-medium">
                    {item.label}
                  </div>
                  <div className="truncate text-xs text-muted-foreground">
                    {item.meta}
                  </div>
                </button>
                {open && (
                  <div className="border-t px-4 py-3">
                    <UsageJsonSyntax value={item.value} />
                  </div>
                )}
              </div>
            )
          })}
          {hasMore && (
            <div className="grid justify-items-center gap-2 py-1">
              <div
                aria-hidden="true"
                className="h-px w-full"
                ref={setSentinel}
              />
              {loadingMore ? (
                <div className="text-xs text-muted-foreground">
                  {t('loading')}
                </div>
              ) : (
                <Button
                  onClick={onLoadMore}
                  size="xs"
                  type="button"
                  variant="ghost"
                >
                  {t('showMoreContent')}
                </Button>
              )}
            </div>
          )}
        </div>
      )}
    </section>
  )
}
