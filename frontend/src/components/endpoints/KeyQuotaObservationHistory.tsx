import { lazy, Suspense, useCallback, useEffect, useRef, useState } from 'react'
import { LoaderCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { TokenPlanQuotaWindowRow } from '@/components/endpoints/TokenPlanQuotaWindowRow'
import { toCanonicalQuotaRows } from '@/composables/useCanonicalQuotaWindows'
import type {
  QuotaKeyObservationHistoryItem,
  QuotaKeyObservationTrendResponse,
  TokenPlanKeyUsage,
} from '@/generated/admin-api'
import { useLocale } from '@/lib/i18n'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { formatDateTime } from '@/format/datetime-core'
import {
  fetchKeyQuotaObservationHistory,
  fetchKeyQuotaObservationTrend,
} from '@/stores/endpoints-api'
import type { QuotaTrendRange } from '@/quota-observation-charts-core'

const KeyQuotaObservationChart = lazy(() =>
  import('@/components/endpoints/KeyQuotaObservationChart').then((module) => ({
    default: module.KeyQuotaObservationChart,
  })),
)

const PAGE_SIZE = 50
const DEFAULT_RETENTION_DAYS = 30
const ALL_KEYS = '__quota_history_all_keys__'
const TREND_RANGES: readonly QuotaTrendRange[] = ['24h', '7d', '30d']
type KeyOptionState = { endpointId: string; values: Map<string, string> }
type EndpointKeyScope = { endpointId: string; keyId: string | null }

function uniqueObservations(
  items: QuotaKeyObservationHistoryItem[],
): QuotaKeyObservationHistoryItem[] {
  const seen = new Set<string>()
  return items.filter((item) => {
    if (seen.has(item.observation_id)) return false
    seen.add(item.observation_id)
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

function rangeLabel(range: QuotaTrendRange, t: TranslateFn): string {
  return t(
    range === '24h'
      ? 'quotaObservationRange24h'
      : range === '7d'
        ? 'quotaObservationRange7d'
        : 'quotaObservationRange30d',
  )
}

export type KeyQuotaObservationHistoryProps = {
  endpointId: string
}

export function KeyQuotaObservationHistory({
  endpointId,
}: KeyQuotaObservationHistoryProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const [expanded, setExpanded] = useState(false)
  const [mode, setMode] = useState<'chart' | 'list'>('chart')
  const [range, setRange] = useState<QuotaTrendRange>('24h')
  const [scope, setScope] = useState<EndpointKeyScope>({
    endpointId,
    keyId: null,
  })
  const selectedKeyId = scope.endpointId === endpointId ? scope.keyId : null
  const [keyOptionState, setKeyOptionState] = useState<KeyOptionState>(() => ({
    endpointId,
    values: new Map(),
  }))
  const knownKeys =
    keyOptionState.endpointId === endpointId
      ? keyOptionState.values
      : new Map<string, string>()
  const currentScope = useRef({ endpointId, keyId: selectedKeyId })
  currentScope.current = { endpointId, keyId: selectedKeyId }

  const [items, setItems] = useState<QuotaKeyObservationHistoryItem[]>([])
  const [nextCursor, setNextCursor] = useState<string | null>(null)
  const [retentionDays, setRetentionDays] = useState(DEFAULT_RETENTION_DAYS)
  const [listLoading, setListLoading] = useState(false)
  const [listLoadingMore, setListLoadingMore] = useState(false)
  const [listFailed, setListFailed] = useState(false)
  const [listLoaded, setListLoaded] = useState(false)
  const listGeneration = useRef(0)

  const [trend, setTrend] = useState<QuotaKeyObservationTrendResponse | null>(
    null,
  )
  const [trendLoading, setTrendLoading] = useState(false)
  const [trendFailed, setTrendFailed] = useState(false)
  const trendGeneration = useRef(0)

  const chartActive = expanded && mode === 'chart'
  const listActive = expanded && mode === 'list'
  const activeTrendScope = useRef({
    endpointId,
    keyId: selectedKeyId,
    range,
    active: chartActive,
  })
  activeTrendScope.current = {
    endpointId,
    keyId: selectedKeyId,
    range,
    active: chartActive,
  }

  const clearList = useCallback(() => {
    listGeneration.current += 1
    setItems([])
    setNextCursor(null)
    setRetentionDays(DEFAULT_RETENTION_DAYS)
    setListLoading(false)
    setListLoadingMore(false)
    setListFailed(false)
    setListLoaded(false)
  }, [])

  const loadListFirstPage = useCallback(async () => {
    const id = endpointId
    const keyId = selectedKeyId
    if (!id) return
    const requestGeneration = ++listGeneration.current
    setItems([])
    setNextCursor(null)
    setListFailed(false)
    setListLoaded(false)
    setListLoadingMore(false)
    setListLoading(true)
    try {
      const page = await fetchKeyQuotaObservationHistory(id, keyId, PAGE_SIZE)
      const active = currentScope.current
      if (
        requestGeneration !== listGeneration.current ||
        active.endpointId !== id ||
        active.keyId !== keyId
      ) {
        return
      }
      setItems(uniqueObservations(page.items))
      setNextCursor(page.next_cursor ?? null)
      setRetentionDays(page.retention_days)
      setListLoaded(true)
    } catch {
      const active = currentScope.current
      if (
        requestGeneration === listGeneration.current &&
        active.endpointId === id &&
        active.keyId === keyId
      ) {
        setListFailed(true)
      }
    } finally {
      const active = currentScope.current
      if (
        requestGeneration === listGeneration.current &&
        active.endpointId === id &&
        active.keyId === keyId
      ) {
        setListLoading(false)
      }
    }
  }, [endpointId, selectedKeyId])

  const loadListMore = useCallback(async () => {
    const id = endpointId
    const keyId = selectedKeyId
    const cursor = nextCursor
    if (!id || !cursor || !listLoaded || listLoading || listLoadingMore) {
      return
    }
    const requestGeneration = listGeneration.current
    setListFailed(false)
    setListLoadingMore(true)
    try {
      const page = await fetchKeyQuotaObservationHistory(
        id,
        keyId,
        PAGE_SIZE,
        cursor,
      )
      const active = currentScope.current
      if (
        requestGeneration !== listGeneration.current ||
        active.endpointId !== id ||
        active.keyId !== keyId
      ) {
        return
      }
      setItems((current) => {
        const seen = new Set(current.map((item) => item.observation_id))
        return [
          ...current,
          ...page.items.filter((item) => {
            if (seen.has(item.observation_id)) return false
            seen.add(item.observation_id)
            return true
          }),
        ]
      })
      setNextCursor(page.next_cursor ?? null)
      setRetentionDays(page.retention_days)
    } catch {
      const active = currentScope.current
      if (
        requestGeneration === listGeneration.current &&
        active.endpointId === id &&
        active.keyId === keyId
      ) {
        setListFailed(true)
      }
    } finally {
      const active = currentScope.current
      if (
        requestGeneration === listGeneration.current &&
        active.endpointId === id &&
        active.keyId === keyId
      ) {
        setListLoadingMore(false)
      }
    }
  }, [
    endpointId,
    listLoaded,
    listLoading,
    listLoadingMore,
    nextCursor,
    selectedKeyId,
  ])

  const retryList = useCallback(() => {
    if (items.length > 0 && nextCursor) void loadListMore()
    else void loadListFirstPage()
  }, [items.length, loadListFirstPage, loadListMore, nextCursor])

  const loadTrend = useCallback(async () => {
    const id = endpointId
    const keyId = selectedKeyId
    const selectedRange = range
    if (!id) return
    const requestGeneration = ++trendGeneration.current
    setTrend(null)
    setTrendFailed(false)
    setTrendLoading(true)
    try {
      const result = await fetchKeyQuotaObservationTrend(
        id,
        keyId,
        selectedRange,
      )
      const active = activeTrendScope.current
      if (
        requestGeneration !== trendGeneration.current ||
        !active.active ||
        active.endpointId !== id ||
        active.keyId !== keyId ||
        active.range !== selectedRange
      ) {
        return
      }
      setTrend(result)
    } catch {
      const active = activeTrendScope.current
      if (
        requestGeneration === trendGeneration.current &&
        active.active &&
        active.endpointId === id &&
        active.keyId === keyId &&
        active.range === selectedRange
      ) {
        setTrendFailed(true)
      }
    } finally {
      const active = activeTrendScope.current
      if (
        requestGeneration === trendGeneration.current &&
        active.active &&
        active.endpointId === id &&
        active.keyId === keyId &&
        active.range === selectedRange
      ) {
        setTrendLoading(false)
      }
    }
  }, [endpointId, range, selectedKeyId])

  useEffect(() => {
    if (!chartActive) {
      trendGeneration.current += 1
      setTrend(null)
      setTrendLoading(false)
      setTrendFailed(false)
      return
    }
    void loadTrend()
    return () => {
      trendGeneration.current += 1
    }
  }, [chartActive, loadTrend])

  useEffect(() => {
    if (!expanded) {
      clearList()
      return
    }
    if (listActive && !listLoaded && !listLoading) {
      void loadListFirstPage()
    }
  }, [expanded, listActive])

  const previousScope = useRef({ endpointId, keyId: selectedKeyId })
  useEffect(() => {
    const previous = previousScope.current
    if (
      previous.endpointId === endpointId &&
      previous.keyId === selectedKeyId
    ) {
      return
    }
    previousScope.current = { endpointId, keyId: selectedKeyId }
    if (listActive) void loadListFirstPage()
    else clearList()
  }, [endpointId, selectedKeyId, listActive, loadListFirstPage, clearList])

  useEffect(() => {
    if (scope.endpointId !== endpointId) {
      setScope({ endpointId, keyId: null })
      setKeyOptionState({ endpointId, values: new Map() })
    }
  }, [endpointId, scope.endpointId, clearList])

  useEffect(() => {
    if (scope.endpointId !== endpointId) return
    const next = new Map(knownKeys)
    for (const entry of trend?.series ?? []) {
      if (!next.has(entry.key_id)) next.set(entry.key_id, entry.key_label)
    }
    for (const item of items) {
      if (!next.has(item.key_id)) next.set(item.key_id, item.key_label)
    }
    if (next.size !== knownKeys.size) {
      setKeyOptionState({ endpointId, values: next })
    }
  }, [endpointId, items, knownKeys, scope.endpointId, trend])

  useEffect(
    () => () => {
      listGeneration.current += 1
      trendGeneration.current += 1
    },
    [],
  )

  const selectKey = useCallback(
    (value: string) => {
      setScope({ endpointId, keyId: value === ALL_KEYS ? null : value })
    },
    [endpointId],
  )

  function rowsFor(item: QuotaKeyObservationHistoryItem) {
    const key = {
      key_id: item.key_id,
      key_label: item.key_label,
      ok: item.ok,
      model_remains: [],
      quota_windows: item.windows,
    } as TokenPlanKeyUsage
    const observedAt = Date.parse(item.observed_at)
    return toCanonicalQuotaRows(
      key,
      Number.isFinite(observedAt) ? observedAt : Date.now(),
    )
  }

  return (
    <details
      className="min-w-0 rounded-md border"
      onToggle={(event) => setExpanded(event.currentTarget.open)}
      open={expanded}
    >
      <summary className="cursor-pointer list-none px-3 py-2 font-medium [&::-webkit-details-marker]:hidden">
        {t('quotaObservationHistory')}
      </summary>
      {expanded ? (
        <div className="grid min-w-0 gap-3 border-t p-3">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <div
              aria-label={t('quotaObservationModeLabel')}
              className="inline-flex gap-1"
              role="group"
            >
              <Button
                aria-pressed={mode === 'chart'}
                onClick={() => setMode('chart')}
                type="button"
                variant={mode === 'chart' ? 'secondary' : 'ghost'}
              >
                {t('quotaObservationModeChart')}
              </Button>
              <Button
                aria-pressed={mode === 'list'}
                onClick={() => setMode('list')}
                type="button"
                variant={mode === 'list' ? 'secondary' : 'ghost'}
              >
                {t('quotaObservationModeList')}
              </Button>
            </div>
            <Select onValueChange={selectKey} value={selectedKeyId ?? ALL_KEYS}>
              <SelectTrigger
                aria-label={t('quotaObservationKeyLabel')}
                className="min-w-40 flex-1"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL_KEYS}>
                  {t('quotaObservationAllKeys')}
                </SelectItem>
                {[...knownKeys].map(([keyId, label]) => (
                  <SelectItem key={keyId} value={keyId}>
                    {label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {mode === 'chart' ? (
              <Select
                onValueChange={(value) => setRange(value as QuotaTrendRange)}
                value={range}
              >
                <SelectTrigger
                  aria-label={t('quotaObservationRangeLabel')}
                  className="w-40"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {TREND_RANGES.map((option) => (
                    <SelectItem key={option} value={option}>
                      {rangeLabel(option, t as unknown as TranslateFn)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : null}
          </div>

          {mode === 'chart' ? (
            <Suspense
              fallback={
                <div className="flex items-center gap-2 text-sm text-muted-foreground">
                  <LoaderCircle
                    aria-hidden="true"
                    className="size-4 animate-spin"
                  />
                  <span>{t('quotaTrendLoading')}</span>
                </div>
              }
            >
              <KeyQuotaObservationChart
                failed={trendFailed}
                loading={trendLoading}
                onRetry={() => void loadTrend()}
                range={range}
                trend={trend}
              />
            </Suspense>
          ) : listLoading ? (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <LoaderCircle
                aria-hidden="true"
                className="size-4 animate-spin"
              />
              <span>{t('quotaHistoryLoading')}</span>
            </div>
          ) : (
            <>
              {listFailed ? (
                <p className="text-sm text-destructive">
                  {t('quotaHistoryError')}{' '}
                  <Button
                    onClick={retryList}
                    size="sm"
                    type="button"
                    variant="link"
                  >
                    {t('quotaHistoryRetry')}
                  </Button>
                </p>
              ) : null}
              {listLoaded && items.length === 0 && !listFailed ? (
                <p className="text-sm text-muted-foreground">
                  {t('quotaHistoryEmpty', { days: retentionDays })}
                </p>
              ) : null}
              {items.length > 0 ? (
                <div className="grid min-w-0 gap-2">
                  {items.map((item) => {
                    const observedAt = Date.parse(item.observed_at)
                    const observedMs = Number.isFinite(observedAt)
                      ? observedAt
                      : Date.now()
                    const rows = rowsFor(item)
                    return (
                      <article
                        className="grid min-w-0 gap-2 rounded-md border p-3"
                        data-quota-observation-id={item.observation_id}
                        key={item.observation_id}
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
                          <span className="break-words text-muted-foreground">
                            {item.key_label}
                          </span>
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
                          {!item.ok && item.error_code ? (
                            <span className="break-words text-amber-700 dark:text-amber-300">
                              {item.error_code}
                            </span>
                          ) : null}
                        </header>
                        {rows.length > 0 ? (
                          <div className="grid min-w-0 gap-2">
                            {rows.map((row) => (
                              <TokenPlanQuotaWindowRow
                                key={`${row.sourceId}:${row.sourceModel ?? ''}`}
                                label={row.sourceModel ?? row.sourceId}
                                nowMs={observedMs}
                                row={row}
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
                  disabled={listLoadingMore}
                  onClick={() => void loadListMore()}
                  type="button"
                  variant="outline"
                >
                  {listLoadingMore ? (
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
      ) : null}
    </details>
  )
}
