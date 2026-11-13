import {
  endpointModels,
  quotaKeyObservationHistory,
  quotaKeyObservationTrend,
} from '../generated/admin-api'
import type {
  CanonicalQuotaWindowKind,
  EndpointModelsResponse,
  NativeApi,
  QuotaKeyObservationHistoryResponse,
  QuotaKeyObservationTrendResponse,
} from '../generated/admin-api'
import { expectData, withData } from '../api'

// Issue #993 P1: the model identifiers the endpoint's upstream advertises.
// Read-only discovery: the model override stays free-form, so a failed or
// empty listing never blocks editing.
// Issue #1032: the optional selector scopes the listing to one protocol
// (`chat`/`embeddings`); omitted keeps the endpoint's own protocol semantics
// (an `auto` endpoint returns the union of its catalogs).
export async function fetchEndpointModels(
  endpointId: string,
  nativeApi?: NativeApi | null,
): Promise<EndpointModelsResponse> {
  return expectData(
    await endpointModels<true>(
      withData({
        path: { endpoint_id: endpointId },
        query: nativeApi ? { native_api: nativeApi } : {},
      }),
    ),
  )
}

// Issue #863 P4: generic per-key quota observation history for every
// capable provider. Display-only; never part of routing or quota math.
// Issue #1207 P2: one CSV request carries every selected key, so filters stay
// server-side and never fan out into per-key requests.
export async function fetchKeyQuotaObservationHistory(
  endpointId: string,
  keyIds: readonly string[] = [],
  limit = 50,
  beforeId?: string,
): Promise<QuotaKeyObservationHistoryResponse> {
  return expectData(
    await quotaKeyObservationHistory<true>(
      withData({
        path: { endpoint_id: endpointId },
        query: {
          limit,
          ...keyFilterQuery(keyIds),
          ...(beforeId ? { before_id: beforeId } : {}),
        },
      }),
    ),
  )
}

// Issue #911 P2: bounded server-bucketed trend for the history chart. Read-only
// and range-fixed; the caller never chooses a bucket size or fetches every page.
export async function fetchKeyQuotaObservationTrend(
  endpointId: string,
  keyIds: readonly string[] = [],
  kinds: readonly CanonicalQuotaWindowKind[] = [],
  range: '24h' | '7d' | '30d' = '24h',
): Promise<QuotaKeyObservationTrendResponse> {
  return expectData(
    await quotaKeyObservationTrend<true>(
      withData({
        path: { endpoint_id: endpointId },
        query: {
          range,
          ...keyFilterQuery(keyIds),
          ...(kinds.length > 0 ? { kinds: kinds.join(',') } : {}),
        },
      }),
    ),
  )
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

// Issue #589 P2c: OpenAI Platform organization usage for the endpoint's
// separate Admin API Key. Display-only; never part of routing or quota math.
