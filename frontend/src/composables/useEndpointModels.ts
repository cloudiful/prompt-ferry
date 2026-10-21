import { ref, watch, type Ref } from 'vue'
import { formatApiError } from '@/api'
import { fetchEndpointModels } from '@/stores/endpoints-api'

/**
 * Shared, endpoint-keyed model listing.
 *
 * Every target row of the model route dialog asks for the same endpoint, so
 * one read per endpoint is enough: concurrent asks join the same request and
 * a successful answer is reused for the cache lifetime. A failed read is
 * never cached, so the next ask retries it.
 */
const CACHE_LIFETIME_MS = 60_000

type CacheEntry = {
  models: readonly string[]
  storedAt: number
}

const cache = new Map<string, CacheEntry>()
const pending = new Map<string, Promise<readonly string[]>>()

function normalize(models: readonly string[] | undefined): string[] {
  const unique = new Set<string>()
  for (const model of models ?? []) {
    const trimmed = model.trim()
    if (trimmed) unique.add(trimmed)
  }
  return [...unique].sort()
}

function cachedModels(endpointId: string): readonly string[] | undefined {
  const entry = cache.get(endpointId)
  if (!entry) return undefined
  if (Date.now() - entry.storedAt > CACHE_LIFETIME_MS) {
    cache.delete(endpointId)
    return undefined
  }
  return entry.models
}

function readEndpointModels(endpointId: string): Promise<readonly string[]> {
  const cached = cachedModels(endpointId)
  if (cached) return Promise.resolve(cached)
  const inflight = pending.get(endpointId)
  if (inflight) return inflight
  const request = fetchEndpointModels(endpointId)
    .then((response) => {
      const models = normalize(response.models)
      cache.set(endpointId, { models, storedAt: Date.now() })
      return models
    })
    .finally(() => {
      pending.delete(endpointId)
    })
  pending.set(endpointId, request)
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
export function useEndpointModels(endpointId: Ref<string | null | undefined>) {
  const models = ref<string[]>([])
  const loading = ref(false)
  const errorMessage = ref('')

  // The endpoint the current suggestions belong to. A suggestion list is only
  // ever shown for the endpoint it was read from.
  let activeEndpointId = ''

  // Guards against a late answer for a replaced endpoint.
  let requestToken = 0

  async function refresh(): Promise<void> {
    const id = endpointId.value?.trim() ?? ''
    const token = (requestToken += 1)
    if (id !== activeEndpointId) {
      // The endpoint identity changed. The previous endpoint's suggestions are
      // isolated right away, so a stale suggestion can never be offered or
      // selected while the new listing loads. The draft is untouched.
      activeEndpointId = id
      models.value = []
      errorMessage.value = ''
    }
    if (!id) {
      loading.value = false
      return
    }
    const cached = cachedModels(id)
    if (cached) {
      models.value = [...cached]
      loading.value = false
      return
    }
    loading.value = true
    errorMessage.value = ''
    try {
      const result = await readEndpointModels(id)
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

  watch(
    endpointId,
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
