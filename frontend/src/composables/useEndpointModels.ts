import { ref, watch, type Ref } from 'vue'
import { formatApiError } from '@/api'
import { fetchEndpointModels } from '@/stores/endpoints-api'
import type { NativeApi } from '@/generated/admin-api'

/**
 * Shared, endpoint-keyed model listing.
 *
 * Every target row of the model route dialog asks for the same endpoint, so
 * one read per endpoint is enough: concurrent asks join the same request and
 * a successful answer is reused for the cache lifetime. A failed read is
 * never cached, so the next ask retries it.
 *
 * Issue #1032: an optional protocol selector scopes the listing to one catalog
 * (`chat`/`embeddings`); omitted keeps the endpoint's own protocol semantics.
 * The selector is part of the cache key, so a chat-only answer never satisfies
 * an embeddings ask for the same endpoint.
 */
const CACHE_LIFETIME_MS = 60_000

type CacheEntry = {
  models: readonly string[]
  storedAt: number
}

const cache = new Map<string, CacheEntry>()
const pending = new Map<string, Promise<readonly string[]>>()

function cacheKey(endpointId: string, nativeApi?: NativeApi | null): string {
  return `${endpointId}::${nativeApi ?? ''}`
}

function normalize(models: readonly string[] | undefined): string[] {
  const unique = new Set<string>()
  for (const model of models ?? []) {
    const trimmed = model.trim()
    if (trimmed) unique.add(trimmed)
  }
  return [...unique].sort()
}

function cachedModels(key: string): readonly string[] | undefined {
  const entry = cache.get(key)
  if (!entry) return undefined
  if (Date.now() - entry.storedAt > CACHE_LIFETIME_MS) {
    cache.delete(key)
    return undefined
  }
  return entry.models
}

function readEndpointModels(
  endpointId: string,
  nativeApi?: NativeApi | null,
): Promise<readonly string[]> {
  const key = cacheKey(endpointId, nativeApi)
  const cached = cachedModels(key)
  if (cached) return Promise.resolve(cached)
  const inflight = pending.get(key)
  if (inflight) return inflight
  const request = fetchEndpointModels(endpointId, nativeApi)
    .then((response) => {
      const models = normalize(response.models)
      cache.set(key, { models, storedAt: Date.now() })
      return models
    })
    .finally(() => {
      pending.delete(key)
    })
  pending.set(key, request)
  return request
}

/**
 * The model identifiers one endpoint advertises.
 *
 * The listing is a read: it never touches the draft the user typed and never
 * disables the input. A response that arrives after the endpoint changed is
 * dropped, so a slow answer for the previous endpoint can never populate the
 * newly selected one.
 */
export function useEndpointModels(
  endpointId: Ref<string | null | undefined>,
  nativeApi?: Ref<NativeApi | null | undefined>,
) {
  const models = ref<string[]>([])
  const loading = ref(false)
  const errorMessage = ref('')

  // The endpoint the current suggestions belong to. A suggestion list is only
  // ever shown for the endpoint it was read from.
  let activeEndpointId = ''
  // Issue #1032: the catalog selector the current suggestions belong to. A
  // protocol change is a different catalog, so the previous list is isolated
  // while the new one loads (the draft is untouched).
  let activeSelector = ''

  // Guards against a late answer for a replaced endpoint.
  let requestToken = 0

  async function refresh(): Promise<void> {
    const id = endpointId.value?.trim() ?? ''
    const selectorKey = nativeApi?.value ?? ''
    const token = (requestToken += 1)
    if (id !== activeEndpointId || selectorKey !== activeSelector) {
      // The endpoint identity or the requested protocol changed. The previous
      // suggestions are isolated right away, so a stale suggestion can never be
      // offered or selected while the new listing loads. The draft is untouched.
      activeEndpointId = id
      activeSelector = selectorKey
      models.value = []
      errorMessage.value = ''
    }
    if (!id) {
      loading.value = false
      return
    }
    const cached = cachedModels(cacheKey(id, nativeApi?.value))
    if (cached) {
      models.value = [...cached]
      loading.value = false
      return
    }
    loading.value = true
    errorMessage.value = ''
    try {
      const result = await readEndpointModels(id, nativeApi?.value)
      if (token !== requestToken) return
      models.value = [...result]
    } catch (cause) {
      if (token !== requestToken) return
      models.value = []
      errorMessage.value = formatApiError(cause)
    } finally {
      if (token === requestToken) loading.value = false
    }
  }

  const selector = nativeApi ?? ref<NativeApi | null>(null)

  watch(
    [endpointId, selector],
    () => {
      void refresh()
    },
    { immediate: true },
  )

  return {
    errorMessage,
    loading,
    models,
    refresh,
  }
}
