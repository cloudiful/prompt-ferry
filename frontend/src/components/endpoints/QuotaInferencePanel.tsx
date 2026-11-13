import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { LoaderCircle } from 'lucide-react'
import {
  DataTable,
  type DataTableColumnDef,
} from '@/components/shared/DataTable'
import { Button } from '@/components/ui/button'
import type { Estimate, ModelRow } from '@/generated/admin-api'
import { quotaInferenceOptions } from '@/generated/admin-api/@tanstack/react-query.gen'
import { useLocale } from '@/lib/i18n'

const INFERENCE_TTL_MS = 10 * 60_000

function millions(value: number | null | undefined): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—'
  if (value > 0 && value < 0.005) return '<0.01 M'
  return `${value.toFixed(2)} M`
}

function percent(value: number | null | undefined): string {
  return typeof value === 'number' && Number.isFinite(value)
    ? `${(value * 100).toFixed(1)}%`
    : '—'
}

export function QuotaInferencePanel({
  endpointId,
  enabled,
}: {
  endpointId: string
  enabled: boolean
}) {
  const { t } = useLocale()
  const [expanded, setExpanded] = useState(false)
  const query = useQuery({
    ...quotaInferenceOptions({ path: { endpoint_id: endpointId } }),
    enabled: enabled && expanded && endpointId !== '',
    staleTime: INFERENCE_TTL_MS,
    refetchInterval: false,
  })

  function reason(value: string | null | undefined): string {
    switch (value) {
      case 'insufficient_signal':
        return t('inferenceInsufficient')
      case 'at97_no_sample':
        return t('inferenceNo97Sample')
      case 'storage_unavailable':
        return t('inferenceStorageUnavailable')
      case 'model_truncated':
      case 'block_overflow':
      case 'snapshot_overflow':
        return t('inferenceLimit')
      default:
        return t('inferenceUnidentifiable')
    }
  }

  function estimate(value: Estimate) {
    const available =
      value.capacity_millions != null &&
      Number.isFinite(value.capacity_millions)
    const range = `${t('inferenceRange')}: ${millions(value.lower_millions)} – ${millions(value.upper_millions)}`
    return (
      <span title={available ? range : reason(value.reason)}>
        {available
          ? millions(value.capacity_millions)
          : `— (${reason(value.reason)})`}
      </span>
    )
  }

  const columns: Array<DataTableColumnDef<ModelRow>> = [
    {
      accessorKey: 'model',
      header: t('inferenceModel'),
      cell: ({ row }) => (
        <span className="break-all">{row.original.model}</span>
      ),
    },
    {
      id: 'mixed',
      header: t('inferenceMixedTokens'),
      cell: ({ row }) => millions(row.original.mixed_tokens / 1_000_000),
    },
    {
      id: 'actual',
      header: t('inferenceActual'),
      cell: ({ row }) => estimate(row.original.actual),
    },
    {
      id: 'near97',
      header: t('inferenceNear97'),
      cell: ({ row }) => estimate(row.original.observed_near_97),
    },
    {
      id: 'details',
      header: t('inferenceDetails'),
      cell: ({ row }) => (
        <span
          className="whitespace-nowrap"
          title={`${t('inferenceFresh')}: ${row.original.fresh_tokens.toLocaleString()} · ${t('inferenceCacheRead')}: ${row.original.cache_read_tokens.toLocaleString()} · ${t('inferenceCacheWrite')}: ${row.original.cache_write_tokens.toLocaleString()} · ${t('inferenceOutput')}: ${row.original.output_tokens.toLocaleString()}`}
        >
          {t('inferenceCacheHit')}: {percent(row.original.cache_hit)} ·{' '}
          {t('inferenceOutput')}: {percent(row.original.output_share)}
        </span>
      ),
    },
  ]
  const data = query.data
  return (
    <details
      className="min-w-0 border-b"
      onToggle={(event) => setExpanded(event.currentTarget.open)}
    >
      <summary className="cursor-pointer px-1 py-2 font-semibold">
        {t('inferenceTitle')}
      </summary>
      <div className="grid min-w-0 gap-2 px-1 pb-3">
        {!enabled ? (
          <span className="text-muted-foreground">
            {t('inferenceUnsupported')}
          </span>
        ) : null}
        {enabled && query.isLoading ? (
          <span className="flex items-center gap-2 text-muted-foreground">
            <LoaderCircle aria-hidden="true" className="size-4 animate-spin" />
            {t('loading')}
          </span>
        ) : null}
        {enabled && query.isError ? (
          <div className="flex items-center gap-2 text-destructive">
            <span>{t('inferenceError')}</span>
            <Button
              disabled={query.isFetching}
              onClick={() => void query.refetch()}
              size="sm"
              variant="outline"
            >
              {query.isFetching ? (
                <LoaderCircle
                  aria-hidden="true"
                  className="size-4 animate-spin"
                />
              ) : null}
              {t('quotaHistoryRetry')}
            </Button>
          </div>
        ) : null}
        {enabled && data ? (
          <>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
              <span>
                {t('inferenceSharedPool')}: {estimate(data.account)}
              </span>
              {data.status === 'stale' ||
              data.current_refresh_failed ||
              query.isStale ||
              query.isError ||
              query.isFetching ? (
                <span className="text-amber-700 dark:text-amber-300">
                  {t('inferenceStale')}
                </span>
              ) : null}
              {data.status === 'insufficient_signal' ? (
                <span className="text-muted-foreground">
                  {t('inferenceInsufficient')}
                </span>
              ) : null}
              {query.isFetching ? (
                <LoaderCircle
                  aria-label={t('loading')}
                  className="size-4 animate-spin"
                />
              ) : null}
            </div>
            <DataTable
              columns={columns}
              data={data.models}
              emptyLabel={t('inferenceNoModels')}
              getRowId={(row) => row.model}
              label={t('inferenceTitle')}
            />
            <details className="text-muted-foreground">
              <summary className="cursor-pointer">
                {t('inferenceDetails')}
              </summary>
              <div className="flex flex-wrap gap-x-3 gap-y-1 pt-1">
                <span>
                  {t('inferenceSpan')}:{' '}
                  {data.span_start
                    ? new Date(data.span_start).toLocaleString()
                    : '—'}{' '}
                  – {data.as_of ? new Date(data.as_of).toLocaleString() : '—'}
                </span>
                <span>
                  {t('inferenceBlocks')}: {data.usable_blocks}
                </span>
                <span>
                  {t('inferenceConsumed')}: {data.consumed_pp.toFixed(1)} pp
                </span>
                <span>
                  {t('inferenceCalculated')}:{' '}
                  {new Date(data.calculated_at).toLocaleString()}
                  {data.cached ? ` (${t('inferenceCached')})` : ''}
                </span>
                <span>{t('inferenceWorkloadCaveat')}</span>
                <span>{t('inferenceSuccessOnly')}</span>
                {data.account_history_assumed_stable ? (
                  <span>{t('inferenceAccountAssumed')}</span>
                ) : null}
                {!data.account_identity_known ? (
                  <span>{t('inferenceIdentityUnknown')}</span>
                ) : null}
                {data.current_refresh_failed ? (
                  <span>{t('inferenceRefreshFailed')}</span>
                ) : null}
                {data.models_truncated ? (
                  <span>{t('inferenceTruncated')}</span>
                ) : null}
                <span>
                  {t('inferenceOmitted')}: {data.omitted_rows} ·{' '}
                  {t('inferenceUnknownTokens')}:{' '}
                  {data.unknown_tokens.toLocaleString()} ·{' '}
                  {t('inferenceOverflowTokens')}:{' '}
                  {data.overflow_tokens.toLocaleString()}
                </span>
              </div>
            </details>
          </>
        ) : null}
      </div>
    </details>
  )
}
