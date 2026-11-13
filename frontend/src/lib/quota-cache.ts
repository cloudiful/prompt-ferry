import type { QueryClient } from '@tanstack/react-query'
import type { TokenPlanUsageResponse } from '@/generated/admin-api'
import { tokenPlanUsage } from '@/generated/admin-api'
import {
  quotaInferenceQueryKey,
  tokenPlanUsageQueryKey,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import type { TokenPlanFetchState } from '@/models/endpoints/quota'

export const TOKEN_PLAN_CACHE_TTL_MS = 60_000

export type TokenPlanUsageSnapshot = {
  state: TokenPlanFetchState
  usage: TokenPlanUsageResponse | null
}

export type TokenPlanUsageRefreshOutcome = {
  endpointId: string
  requested: boolean
  failed: boolean
  cause: unknown
}

type TokenPlanUsageSnapshotSource = {
  data: TokenPlanUsageResponse | undefined
  dataUpdatedAt: number
  isError: boolean
  isStale?: boolean
}

export function tokenPlanUsageSnapshot(
  source: TokenPlanUsageSnapshotSource,
  nowMs = Date.now(),
): TokenPlanUsageSnapshot {
  const usage = source.data ?? null
  if (usage === null) {
    return { state: source.isError ? 'error' : 'loading', usage: null }
  }
  const stale =
    source.isError ||
    source.isStale === true ||
    nowMs - source.dataUpdatedAt >= TOKEN_PLAN_CACHE_TTL_MS
  return { state: stale ? 'stale' : 'ready', usage }
}

function uniqueEndpointIds(endpointIds: Iterable<string>): string[] {
  return Array.from(new Set(endpointIds)).filter(Boolean)
}

function tokenPlanUsageQuery(endpointId: string, force: boolean) {
  return {
    queryKey: tokenPlanUsageQueryKey({ path: { endpoint_id: endpointId } }),
    queryFn: ({ signal }: { signal: AbortSignal }) =>
      tokenPlanUsage({
        path: { endpoint_id: endpointId },
        query: { force },
        signal,
        throwOnError: true,
      }),
  }
}

async function runEndpointPool(
  endpointIds: readonly string[],
  concurrency: number,
  run: (endpointId: string) => Promise<void>,
): Promise<void> {
  let index = 0
  const workers = Array.from({ length: Math.max(1, concurrency) }, async () => {
    while (index < endpointIds.length) {
      const endpointId = endpointIds[index++]
      if (endpointId === undefined) break
      await run(endpointId)
    }
  })
  await Promise.all(workers)
}

export async function prefetchTokenPlanUsageBatch(
  queryClient: QueryClient,
  endpointIds: Iterable<string>,
  concurrency = 4,
  isEligible: (endpointId: string) => boolean = () => true,
): Promise<void> {
  const ids = uniqueEndpointIds(endpointIds)
  await runEndpointPool(ids, concurrency, async (endpointId) => {
    if (!isEligible(endpointId)) return
    await queryClient.prefetchQuery({
      ...tokenPlanUsageQuery(endpointId, false),
      staleTime: TOKEN_PLAN_CACHE_TTL_MS,
    })
  })
}

export async function refreshTokenPlanUsageBatch(
  queryClient: QueryClient,
  endpointIds: Iterable<string>,
  concurrency = 4,
): Promise<TokenPlanUsageRefreshOutcome[]> {
  const ids = uniqueEndpointIds(endpointIds)
  const outcomes = new Map<string, TokenPlanUsageRefreshOutcome>()
  await runEndpointPool(ids, concurrency, async (endpointId) => {
    try {
      const query = tokenPlanUsageQuery(endpointId, true)
      // The forced read shares the canonical key with ordinary reads, so an
      // in-flight ordinary fetch would dedupe it and answer with cached data.
      // Await that fetch first, then run the forced one on the settled cache.
      const inFlight = queryClient
        .getQueryCache()
        .find({ queryKey: query.queryKey, exact: true })
      if (inFlight && inFlight.state.fetchStatus !== 'idle') {
        await inFlight.promise?.catch(() => undefined)
      }
      await queryClient.fetchQuery({ ...query, staleTime: 0 })
      outcomes.set(endpointId, {
        endpointId,
        requested: true,
        failed: false,
        cause: null,
      })
    } catch (cause) {
      outcomes.set(endpointId, {
        endpointId,
        requested: true,
        failed: true,
        cause,
      })
    } finally {
      await queryClient.invalidateQueries({
        queryKey: quotaInferenceQueryKey({ path: { endpoint_id: endpointId } }),
      })
    }
  })
  return ids.flatMap((endpointId) => {
    const outcome = outcomes.get(endpointId)
    return outcome ? [outcome] : []
  })
}
