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

  function reason(
    value: Estimate,
    near: boolean,
  ): { label: string; hint: string } {
    switch (value.reason) {
      case 'insufficient_signal':
        if (value.blocks == null || value.required_blocks == null) {
          return {
            label: t('inferenceReasonEvidence'),
            hint: t('inferenceReasonEvidenceHint'),
          }
        }
        if (value.blocks < value.required_blocks) {
          return near
            ? {
                label: t('inferenceReasonNear97Insufficient'),
                hint: t('inferenceReasonNear97InsufficientHint'),
              }
            : {
                label: t('inferenceReasonInsufficient'),
                hint: t('inferenceReasonInsufficientHint'),
              }
        }
        return near
          ? {
              label: t('inferenceReasonNear97Coverage'),
              hint: t('inferenceReasonNear97CoverageHint'),
            }
          : {
              label: t('inferenceReasonCoverage'),
              hint: t('inferenceReasonCoverageHint'),
            }
      case 'complexity':
        return {
          label: t('inferenceReasonComplexity'),
          hint: t('inferenceReasonComplexityHint'),
        }
      case 'model_mix_flat':
        return {
          label: t('inferenceReasonModelMixFlat'),
          hint: t('inferenceReasonModelMixFlatHint'),
        }
      case 'ill_conditioned':
        return {
          label: t('inferenceReasonIllConditioned'),
          hint: t('inferenceReasonIllConditionedHint'),
        }
      case 'poor_fit':
        return {
          label: t('inferenceReasonPoorFit'),
          hint: t('inferenceReasonPoorFitHint'),
        }
      case 'unstable':
        return {
          label: t('inferenceReasonUnstable'),
          hint: t('inferenceReasonUnstableHint'),
        }
      case 'attribution_unstable':
        return {
          label: t('inferenceReasonAttributionUnstable'),
          hint: t('inferenceReasonAttributionUnstableHint'),
        }
      case 'low_confidence':
        return {
          label: t('inferenceReasonLowConfidence'),
          hint: t('inferenceReasonLowConfidenceHint'),
        }
      case 'snapshot_overflow':
        return {
          label: t('inferenceReasonSnapshotOverflow'),
          hint: t('inferenceReasonSnapshotOverflowHint'),
        }
      case 'block_overflow':
        return {
          label: t('inferenceReasonBlockOverflow'),
          hint: t('inferenceReasonBlockOverflowHint'),
        }
      case 'model_truncated':
        return {
          label: t('inferenceReasonModelTruncated'),
          hint: t('inferenceReasonModelTruncatedHint'),
        }
      case 'at97_no_sample':
        return {
          label: t('inferenceReasonNo97Sample'),
          hint: t('inferenceReasonNo97SampleHint'),
        }
      case 'at97_unidentifiable':
        return {
          label: t('inferenceReasonUnidentifiable'),
          hint: t('inferenceReasonUnidentifiableHint'),
        }
      case 'storage_unavailable':
        return {
          label: t('inferenceReasonStorageUnavailable'),
          hint: t('inferenceReasonStorageUnavailableHint'),
        }
      default:
        return value.reason
          ? { label: value.reason, hint: t('inferenceReasonUnknownHint') }
          : {
              label: t('inferenceReasonUnknown'),
              hint: t('inferenceReasonUnknownHint'),
            }
    }
  }

  function estimate(value: Estimate, near = false) {
    const available =
      value.capacity_millions != null &&
      Number.isFinite(value.capacity_millions)
    const resolved = reason(value, near)
    const range = `${t('inferenceRange')}: ${millions(value.lower_millions)} – ${millions(value.upper_millions)}`
    const blocks =
      near && value.blocks != null && value.required_blocks != null
        ? t('inferenceNear97Detail', {
            blocks: value.blocks,
            required: value.required_blocks,
          })
        : null
    const title = available
      ? [range, blocks].filter(Boolean).join(' · ')
      : [resolved.hint, blocks].filter(Boolean).join(' ')
    return (
      <span title={title}>
        {available ? (
          millions(value.capacity_millions)
        ) : (
          <span className="text-muted-foreground">— {resolved.label}</span>
        )}
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
      id: 'usage',
      header: t('inferenceUsage'),
      cell: ({ row }) => {
        const model = row.original
        const detail = `${t('inferenceFresh')}: ${model.fresh_tokens.toLocaleString()} · ${t('inferenceCacheRead')}: ${model.cache_read_tokens.toLocaleString()} · ${t('inferenceCacheWrite')}: ${model.cache_write_tokens.toLocaleString()} · ${t('inferenceOutput')}: ${model.output_tokens.toLocaleString()} · ${t('inferenceCacheHit')}: ${percent(model.cache_hit)} · ${t('inferenceOutputShare')}: ${percent(model.output_share)}`
        return (
          <span title={detail}>{millions(model.mixed_tokens / 1_000_000)}</span>
        )
      },
    },
    {
      id: 'capacity',
      header: t('inferenceCapacity'),
      cell: ({ row }) => estimate(row.original.actual),
    },
    {
      id: 'near97',
      header: t('inferenceNear97'),
      cell: ({ row }) => estimate(row.original.observed_near_97, true),
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
