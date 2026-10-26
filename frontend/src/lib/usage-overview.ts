import type {
  RequestRecordOverviewBreakdownRow,
  RequestRecordOverviewModelBreakdown,
  RequestRecordOverviewUpstreamBreakdown,
} from '@/generated/admin-api'
import type { UsageCategory, UsagePerspective } from './usage-model'

export type UsageOverviewDrilldown = {
  endpoint_id?: string | null
  model?: string | null
  mcp_server_id?: string | null
  mcp_bearer_token_slot?: number | null
  mcp_scope?: 'service' | 'gateway' | 'all' | null
}

/**
 * The record filter a clicked distribution row stands for.
 *
 * A direct upstream row has no endpoint identity, so it returns `null` instead
 * of opening a misleading unfiltered record list. An MCP row carries the scope
 * it was grouped by, so the drilldown never infers a scope from a null server
 * id or a display label.
 */
export function usageDrilldownForPerspective(
  perspective: UsagePerspective,
  category: UsageCategory,
  row: RequestRecordOverviewBreakdownRow,
): UsageOverviewDrilldown | null {
  if (category === 'mcp') {
    const mcpServerId = row.mcp_server_id ?? null
    if (row.mcp_scope === 'gateway') {
      return { model: null, mcp_server_id: null, mcp_scope: 'gateway' }
    }
    if (mcpServerId) {
      return { model: null, mcp_server_id: mcpServerId, mcp_scope: 'service' }
    }
    return { model: row.label, mcp_server_id: null, mcp_scope: 'service' }
  }
  if (perspective === 'upstream') {
    return row.endpoint_id ? { endpoint_id: row.endpoint_id } : null
  }
  return { model: row.model ?? null, mcp_server_id: null }
}

/**
 * A trend bucket label.
 *
 * A day bucket is a logical UTC calendar date, so its label keeps that date
 * instead of moving onto the previous day in a negative-offset zone. An hour
 * bucket is an instant, so its label follows the display time zone.
 */
export function usageTrendBucketLabel(
  granularity: string | null | undefined,
  bucketAt: string,
  formatInstant: (value: string) => string,
  formatUtcDate: (value: string) => string,
): string {
  return granularity === 'day'
    ? formatUtcDate(bucketAt)
    : formatInstant(bucketAt)
}

/** One normalized row of the breakdown detail popover. */
export type UsageDetailEntry = {
  key: string
  endpoint: string
  model: string
  request_count: number
  request_share: number
  total_tokens: number | null
  token_share: number | null
  cache_rate: number | null
  error_rate: number | null
  avg_output_tokens_per_second: number | null
}

export type UsageDetailGroup = {
  entries: UsageDetailEntry[]
  /** The popover is offered only when there is more than one detail row. */
  show: boolean
}

function endpointLabel(entry: RequestRecordOverviewUpstreamBreakdown): string {
  if (entry.endpoint_name && entry.endpoint_name.length > 0)
    return entry.endpoint_name
  if (entry.endpoint_id && entry.endpoint_id.length > 0)
    return entry.endpoint_id.slice(0, 8)
  return '-'
}

function numberOrNull(value: number | null | undefined): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function totalOrNull(value: number | null | undefined): number | null {
  // A missing measurement must never become 0: only an explicit 0 folds.
  return typeof value === 'number' && !Number.isNaN(value) ? value : null
}

function upstreamEntry(
  entry: RequestRecordOverviewUpstreamBreakdown,
  rowLabel: string,
): UsageDetailEntry {
  return {
    key: `${entry.endpoint_id ?? 'direct'}:${entry.upstream_model ?? ''}`,
    endpoint: endpointLabel(entry),
    model:
      entry.upstream_model && entry.upstream_model.length > 0
        ? entry.upstream_model
        : rowLabel,
    request_count: entry.request_count,
    request_share: entry.request_share,
    total_tokens: totalOrNull(entry.total_tokens),
    token_share: numberOrNull(entry.token_share),
    cache_rate: numberOrNull(entry.cache_rate),
    error_rate: numberOrNull(entry.error_rate),
    avg_output_tokens_per_second: numberOrNull(
      entry.avg_output_tokens_per_second,
    ),
  }
}

function modelEntry(
  entry: RequestRecordOverviewModelBreakdown,
): UsageDetailEntry {
  return {
    key: entry.model,
    endpoint: '-',
    model: entry.model,
    request_count: entry.request_count,
    request_share: entry.request_share,
    total_tokens: totalOrNull(entry.total_tokens),
    token_share: numberOrNull(entry.token_share),
    cache_rate: numberOrNull(entry.cache_rate),
    error_rate: numberOrNull(entry.error_rate),
    avg_output_tokens_per_second: numberOrNull(
      entry.avg_output_tokens_per_second,
    ),
  }
}

/**
 * Normalize the detail rows for one distribution row.
 *
 * The model perspective lists its per-upstream rows and the upstream
 * perspective lists its per-model rows; every metric keeps its null rather than
 * being coerced to a number.
 */
export function usageDetailGroup(
  row: RequestRecordOverviewBreakdownRow,
  perspective: UsagePerspective,
): UsageDetailGroup {
  if (perspective === 'upstream') {
    const entries = (row.model_breakdown ?? []).map(modelEntry)
    return { entries, show: entries.length > 1 }
  }
  const entries = (row.upstream_breakdown ?? []).map((entry) =>
    upstreamEntry(entry, row.label),
  )
  return { entries, show: (row.upstream_count ?? 0) > 1 }
}
