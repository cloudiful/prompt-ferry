import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useState,
} from 'react'
import { useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { LoaderCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { MultiSelect } from '@/components/ui/multi-select'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { TokenPlanQuotaWindowRow } from '@/components/endpoints/TokenPlanQuotaWindowRow'
import { quotaWindowLabel } from '@/components/endpoints/TokenPlanQuotaWindows'
import { toCanonicalQuotaRows } from '@/composables/useCanonicalQuotaWindows'
import type {
  CanonicalQuotaWindowKind,
  QuotaKeyObservationHistoryItem,
  QuotaKeyObservationTrendResponse,
  TokenPlanKeyUsage,
} from '@/generated/admin-api'
import { useLocale } from '@/lib/i18n'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { formatDateTime } from '@/format/datetime-core'
import {
  quotaKeyObservationHistoryQueryKey,
  quotaKeyObservationTrendOptions,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { quotaKeyObservationHistory } from '@/generated/admin-api'
import {
  QUOTA_TREND_KINDS,
  quotaTrendKindLabels,
  shortKeyId,
  type QuotaTrendRange,
} from '@/quota-observation-charts-core'

const KeyQuotaObservationChart = lazy(() =>
  import('@/components/endpoints/KeyQuotaObservationChart').then((module) => ({
    default: module.KeyQuotaObservationChart,
  })),
)

const PAGE_SIZE = 50
const DEFAULT_RETENTION_DAYS = 30
const KEY_SELECTION_MAX = 24
const TREND_RANGES: readonly QuotaTrendRange[] = ['24h', '7d', '30d']

type KeyOption = {
  keyId: string
  label: string
}

type HistorySelection = {
  endpointId: string
  keyIds: string[]
  kinds: CanonicalQuotaWindowKind[]
}

type ObservedKeys = {
  endpointId: string
  values: Map<string, string>
}

function keyFilterQuery(keyIds: readonly string[]): {
  key_id?: string
  key_ids?: string
} {
  const keys = [...new Set(keyIds)].sort()
  if (keys.length === 0) return {}
  if (keys.length === 1) return { key_id: keys[0] }
  return { key_ids: keys.join(',') }
}

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

function keyOptions(
  configured: readonly KeyOption[],
  observed: Map<string, string>,
): KeyOption[] {
  const labels = new Map<string, string>()
  for (const key of configured) labels.set(key.keyId, key.label)
  for (const [keyId, label] of observed) {
    if (!labels.has(keyId)) labels.set(keyId, label)
  }
  const counts = new Map<string, number>()
  for (const label of labels.values()) {
    counts.set(label, (counts.get(label) ?? 0) + 1)
  }
  return [...labels.entries()].map(([keyId, label]) => ({
    keyId,
    label:
      (counts.get(label) ?? 0) > 1 ? `${label} #${shortKeyId(keyId)}` : label,
  }))
}

function selectionSummary(
  selected: readonly string[],
  options: readonly { value: string; label: string }[],
  allLabel: string,
  countLabel: string,
): string {
  if (selected.length === 0) return allLabel
  const labels = options
    .filter((option) => selected.includes(option.value))
    .map((option) => option.label)
  if (labels.length === 0) return countLabel
  if (labels.length <= 2) return labels.join(', ')
  return countLabel
}

export type KeyQuotaObservationHistoryProps = {
  configuredKeys?: readonly KeyOption[]
  endpointId: string
}

export function KeyQuotaObservationHistory({
  configuredKeys = [],
  endpointId,
}: KeyQuotaObservationHistoryProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const kindLabels = useMemo(
    () => quotaTrendKindLabels(t as unknown as (key: string) => string),
    [t],
  )
  const [expanded, setExpanded] = useState(false)
  const [mode, setMode] = useState<'chart' | 'list'>('chart')
  const [range, setRange] = useState<QuotaTrendRange>('24h')
  const [selection, setSelection] = useState<HistorySelection>(() => ({
    endpointId,
    keyIds: [],
    kinds: [],
  }))
  const [observedKeys, setObservedKeys] = useState<ObservedKeys>(() => ({
    endpointId,
    values: new Map(),
  }))

  const activeKeyIds =
    selection.endpointId === endpointId ? selection.keyIds : []
  const activeKinds = selection.endpointId === endpointId ? selection.kinds : []
  const keyKey = activeKeyIds.join(',')
  const kindKey = activeKinds.join(',')

  const options = useMemo(
    () =>
      keyOptions(
        configuredKeys,
        observedKeys.endpointId === endpointId
          ? observedKeys.values
          : new Map(),
      ),
    [configuredKeys, endpointId, observedKeys],
  )
  const keySelectOptions = options.map((option) => ({
    value: option.keyId,
    label: option.label,
  }))
  const kindSelectOptions = QUOTA_TREND_KINDS.map((kind) => ({
    value: kind,
    label: kindLabels[kind],
  }))

  const chartActive = expanded && mode === 'chart'
  const listActive = expanded && mode === 'list'
  const keyIds = useMemo(() => (keyKey ? keyKey.split(',') : []), [keyKey])
  const kinds = useMemo(
    () => (kindKey ? (kindKey.split(',') as CanonicalQuotaWindowKind[]) : []),
    [kindKey],
  )
  const historyKeyFilter = useMemo(() => keyFilterQuery(keyIds), [keyIds])
  const trendKindFilter = kinds.length > 0 ? { kinds: kinds.join(',') } : {}

  const listQuery = useInfiniteQuery({
    queryKey: quotaKeyObservationHistoryQueryKey({
      path: { endpoint_id: endpointId },
      query: { limit: PAGE_SIZE, ...historyKeyFilter },
    }),
    queryFn: ({ pageParam, signal }) =>
      quotaKeyObservationHistory({
        path: { endpoint_id: endpointId },
        query: {
          limit: PAGE_SIZE,
          ...historyKeyFilter,
          ...(pageParam ? { before_id: pageParam } : {}),
        },
        signal,
        throwOnError: true,
      }),
    initialPageParam: null as string | null,
    getNextPageParam: (last) => last.next_cursor ?? null,
    enabled: Boolean(endpointId) && listActive,
  })

  const items = useMemo(
    () =>
      uniqueObservations(
        listQuery.data?.pages.flatMap((page) => page.items) ?? [],
      ),
    [listQuery.data],
  )
  const lastPage = listQuery.data?.pages.at(-1)
  const nextCursor = lastPage?.next_cursor ?? null
  const retentionDays = lastPage?.retention_days ?? DEFAULT_RETENTION_DAYS
  const listLoading = listQuery.isLoading
  const listLoadingMore = listQuery.isFetchingNextPage
  const listFailed = listQuery.isError
  const listLoaded = listQuery.isSuccess

  const trendQuery = useQuery({
    ...quotaKeyObservationTrendOptions({
      path: { endpoint_id: endpointId },
      query: {
        range,
        ...historyKeyFilter,
        ...trendKindFilter,
      },
    }),
    enabled: Boolean(endpointId) && chartActive,
  })
  const trend: QuotaKeyObservationTrendResponse | null = trendQuery.data ?? null
  const trendLoading = trendQuery.isFetching && !trendQuery.data
  const trendFailed = trendQuery.isError

  function loadListMore(): void {
    if (listQuery.hasNextPage && !listQuery.isFetchingNextPage) {
      void listQuery.fetchNextPage()
    }
  }

  function retryList(): void {
    if (items.length > 0 && nextCursor && listQuery.isError) {
      loadListMore()
      return
    }
    void listQuery.refetch()
  }

  useEffect(() => {
    if (selection.endpointId === endpointId) return
    setSelection({ endpointId, keyIds: [], kinds: [] })
    setObservedKeys({ endpointId, values: new Map() })
  }, [endpointId, selection.endpointId])

  useEffect(() => {
    if (observedKeys.endpointId !== endpointId) return
    const next = new Map(observedKeys.values)
    let changed = false
    for (const entry of trend?.series ?? []) {
      if (!next.has(entry.key_id)) {
        next.set(entry.key_id, entry.key_label)
        changed = true
      }
    }
    for (const item of items) {
      if (!next.has(item.key_id)) {
        next.set(item.key_id, item.key_label)
        changed = true
      }
    }
    if (changed) setObservedKeys({ endpointId, values: next })
  }, [endpointId, items, observedKeys, trend])

  const selectKeys = useCallback(
    (next: string[]) => {
      setSelection((current) => ({
        endpointId,
        keyIds: [...new Set(next)].sort(),
        kinds: current.endpointId === endpointId ? current.kinds : [],
      }))
    },
    [endpointId],
  )

  const selectKinds = useCallback(
    (next: string[]) => {
      const selected = new Set(next)
      setSelection((current) => ({
        endpointId,
        keyIds: current.endpointId === endpointId ? current.keyIds : [],
        kinds: QUOTA_TREND_KINDS.filter((kind) => selected.has(kind)),
      }))
    },
    [endpointId],
  )

  const visibleKinds = useMemo(() => new Set(activeKinds), [activeKinds])
  const visibleItems = useMemo(
    () =>
      visibleKinds.size === 0
        ? items
        : items.filter(
            (item) =>
              !item.ok ||
              item.windows.some((window) => visibleKinds.has(window.kind)),
          ),
    [items, visibleKinds],
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
    ).filter((row) => visibleKinds.size === 0 || visibleKinds.has(row.kind))
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
            <MultiSelect
              allLabel={t('quotaObservationAllKeys')}
              className="min-w-40 flex-1"
              label={t('quotaObservationKeyLabel')}
              limitLabel={t('quotaObservationKeyLimit', {
                count: KEY_SELECTION_MAX,
              })}
              maxSelected={KEY_SELECTION_MAX}
              onChange={selectKeys}
              options={keySelectOptions}
              selected={activeKeyIds}
              summary={selectionSummary(
                activeKeyIds,
                keySelectOptions,
                t('quotaObservationAllKeys'),
                t('quotaObservationSelectedCount', {
                  count: activeKeyIds.length,
                }),
              )}
            />
            <MultiSelect
              allLabel={t('quotaObservationAllKinds')}
              className="min-w-40"
              label={t('quotaObservationKindLabel')}
              onChange={selectKinds}
              options={kindSelectOptions}
              selected={activeKinds}
              summary={selectionSummary(
                activeKinds,
                kindSelectOptions,
                t('quotaObservationAllKinds'),
                t('quotaObservationSelectedCount', {
                  count: activeKinds.length,
                }),
              )}
            />
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
                onRetry={() => void trendQuery.refetch()}
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
              {listLoaded &&
              items.length > 0 &&
              visibleItems.length === 0 &&
              !listFailed ? (
                <p className="text-sm text-muted-foreground">
                  {t('quotaHistoryFilteredEmpty')}
                </p>
              ) : null}
              {visibleItems.length > 0 ? (
                <div className="grid min-w-0 gap-2">
                  {visibleItems.map((item) => {
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
                                label={quotaWindowLabel(
                                  row,
                                  t as unknown as TranslateFn,
                                )}
                                modelName={row.sourceModel}
                                nowMs={observedMs}
                                row={row}
                                showModelName
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
