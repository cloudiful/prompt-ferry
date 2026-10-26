import type { NativeApi } from '@/generated/admin-api'
import { fetchEndpointModels } from '@/stores/endpoints-api'

export const ENDPOINT_MODEL_CATALOG_TTL_MS = 60_000

type CachedCatalog = { expiresAt: number; models: string[] }

const catalogs = new Map<string, CachedCatalog>()
const inFlight = new Map<string, Promise<string[]>>()

export function endpointModelCatalogKey(
  endpointId: string,
  nativeApi?: NativeApi | null,
): string {
  return `${endpointId}::${nativeApi ?? 'auto'}`
}

export function normalizeEndpointModels(models: readonly unknown[]): string[] {
  return [
    ...new Set(
      models
        .filter(
          (model): model is string =>
            typeof model === 'string' && model.trim() !== '',
        )
        .map((model) => model.trim()),
    ),
  ].sort((a, b) => a.localeCompare(b))
}

export async function loadEndpointModelCatalog(
  endpointId: string,
  nativeApi?: NativeApi | null,
  now = Date.now(),
): Promise<string[]> {
  const key = endpointModelCatalogKey(endpointId, nativeApi)
  const cached = catalogs.get(key)
  if (cached && cached.expiresAt > now) return cached.models
  const existing = inFlight.get(key)
  if (existing) return existing

  const request = fetchEndpointModels(endpointId, nativeApi).then(
    (response) => {
      const models = normalizeEndpointModels(response.models)
      catalogs.set(key, {
        models,
        expiresAt: Date.now() + ENDPOINT_MODEL_CATALOG_TTL_MS,
      })
      return models
    },
  )
  inFlight.set(key, request)
  try {
    return await request
  } finally {
    if (inFlight.get(key) === request) inFlight.delete(key)
  }
}

export function clearEndpointModelCatalogCache(): void {
  catalogs.clear()
  inFlight.clear()
}
