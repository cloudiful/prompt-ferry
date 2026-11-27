import { useEffect, useMemo, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { LoaderCircle } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { TokenPlanQuotaWindowRow } from '@/components/endpoints/TokenPlanQuotaWindowRow'
import { quotaWindowLabel } from '@/components/endpoints/TokenPlanQuotaWindows'
import { formatDateTime } from '@/format/datetime-core'
import { catalogQuotaQuery, TOKEN_PLAN_CACHE_TTL_MS } from '@/lib/quota-cache'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { useLocale } from '@/lib/i18n'
import {
  catalogQuotaGroups,
  catalogQuotaState,
} from '@/models/endpoints/quota'

export type CatalogQuotaProps = {
  endpointId: string
  endpointName: string
  onError?: (cause: unknown) => void
  onOpenChange: (open: boolean) => void
  open: boolean
}

/**
 * The cached quota of one visible endpoint, as the safe catalog answers it.
 *
 * The dialog shows what the server cache holds: the observation windows, the
 * currency and the remaining quota of every window, and the freshness of the
 * observation. It never names a key, never averages across keys and never
 * sums the windows into a synthetic total. The read never collects quota, so
 * a cold cache honestly reads as "no usage data" instead of a zero balance.
 */
export function CatalogQuota({
  endpointId,
  endpointName,
  onError,
  onOpenChange,
  open,
}: CatalogQuotaProps) {
  const { locale, t } = useLocale()
  const { effective: timeZone } = useDisplayTimeZone()
  const onErrorRef = useRef(onError)
  const [nowMs, setNowMs] = useState(() => Date.now())
  onErrorRef.current = onError

  const quotaQuery = useQuery({
    ...catalogQuotaQuery(endpointId),
    enabled: open && endpointId !== '',
    refetchOnWindowFocus: true,
    refetchInterval: TOKEN_PLAN_CACHE_TTL_MS,
  })

  useEffect(() => {
    if (!open || !endpointId || !quotaQuery.error) return
    onErrorRef.current?.(quotaQuery.error)
  }, [endpointId, open, quotaQuery.error])

  useEffect(() => {
    if (!open) return
    setNowMs(Date.now())
    const ticker = window.setInterval(() => setNowMs(Date.now()), 1000)
    return () => window.clearInterval(ticker)
  }, [open])

  const groups = useMemo(
    () => catalogQuotaGroups(quotaQuery.data, nowMs),
    [nowMs, quotaQuery.data],
  )
  const state = catalogQuotaState(quotaQuery.data, groups)

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent className="flex max-h-[calc(100dvh-2rem)] flex-col gap-3 overflow-hidden p-4 sm:max-w-[min(calc(100vw_-_2rem),44rem)] sm:p-6">
        <DialogHeader className="min-w-0 shrink-0 pr-8">
          <DialogTitle className="break-words">
            {t('tokenPlanUsage')} / {endpointName}
          </DialogTitle>
        </DialogHeader>
        <div
          className="grid min-h-0 min-w-0 flex-1 grid-cols-[minmax(0,1fr)] gap-4 overflow-y-auto overscroll-contain pr-1 text-xs"
          data-catalog-quota={endpointId}
          data-catalog-quota-state={state}
        >
          {quotaQuery.isPending ? (
            <div className="flex items-center gap-2 text-muted-foreground">
              <LoaderCircle
                aria-hidden="true"
                className="size-4 animate-spin"
              />
              <span>{t('loading')}</span>
            </div>
          ) : quotaQuery.isError ? (
            <div className="grid gap-2" role="alert">
              <span className="text-muted-foreground">
                {t('tokenPlanUsageFailed')}
              </span>
              <Button
                onClick={() => void quotaQuery.refetch()}
                size="sm"
                type="button"
                variant="outline"
              >
                {t('retry')}
              </Button>
            </div>
          ) : groups.length === 0 ? (
            <p className="text-muted-foreground">{t('tokenPlanNoUsage')}</p>
          ) : (
            groups.map((group, index) => (
              <section
                className="grid min-w-0 gap-2"
                data-catalog-quota-observation={index}
                key={index}
              >
                <div className="flex min-w-0 flex-wrap items-center gap-2">
                  {group.stale ? (
                    <Badge
                      className="w-fit text-amber-700 dark:text-amber-300"
                      variant="outline"
                    >
                      {t('quotaStaleObservation')}
                    </Badge>
                  ) : null}
                  <span className="break-words text-muted-foreground">
                    {group.stale
                      ? group.observedAt
                        ? formatDateTime(
                            group.observedAt,
                            locale,
                            undefined,
                            timeZone,
                          )
                        : t('tokenPlanUsageUnknown')
                      : `${t('quotaCachedObservation')}: ${
                          group.observedAt
                            ? formatDateTime(
                                group.observedAt,
                                locale,
                                undefined,
                                timeZone,
                              )
                            : t('tokenPlanUsageUnknown')
                        }`}
                  </span>
                </div>
                <div className="grid min-w-0 gap-3">
                  {group.rows.map((row, rowIndex) => (
                    <TokenPlanQuotaWindowRow
                      key={`${row.sourceId}:${rowIndex}`}
                      label={quotaWindowLabel(row, t)}
                      modelName={row.sourceModel}
                      muted={group.stale}
                      nowMs={nowMs}
                      quantitySuffix={
                        row.unit === 'currency' ? ' USD' : undefined
                      }
                      row={row}
                      showModelName={row.sourceModel !== null}
                    />
                  ))}
                </div>
              </section>
            ))
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
