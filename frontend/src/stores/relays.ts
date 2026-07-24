import { ref } from 'vue'
import { defineStore } from 'pinia'
import {
  createRelay,
  deleteRelay,
  listRelays,
  reconnectRelay as requestRelayReconnect,
  updateRelay,
} from '../generated/admin-api'
import type {
  ManagedRelay,
  ManagedRelayPatchRequest,
  ManagedRelayRequest,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'

export const useRelaysStore = defineStore('relays', () => {
  const relays = ref<ManagedRelay[]>([])
  const loading = ref(false)
  const reconnectingRelayId = ref<string | null>(null)
  const first = ref(0)
  const rows = useStoredPageSize('relays', 10, STANDARD_PAGE_SIZE_OPTIONS)
  const total = ref(0)
  const connectedCount = ref(0)
  const enabledCount = ref(0)

  async function refresh(
    nextFirst = first.value,
    nextRows = rows.value,
  ): Promise<void> {
    first.value = nextFirst
    rows.value = nextRows
    loading.value = true
    try {
      const response = expectData(
        await listRelays<true>(
          withData({ query: { first: first.value, rows: rows.value } }),
        ),
      )
      relays.value = response.relays
      total.value = response.total
      first.value = response.first
      rows.value = response.rows
      connectedCount.value = response.connected_count
      enabledCount.value = response.enabled_count
      if (
        relays.value.length === 0 &&
        total.value > 0 &&
        first.value >= total.value
      ) {
        const previousFirst =
          Math.floor((total.value - 1) / rows.value) * rows.value
        await refresh(previousFirst, rows.value)
      }
    } finally {
      loading.value = false
    }
  }

  async function saveRelay(
    relayId: string | null,
    body: ManagedRelayRequest | ManagedRelayPatchRequest,
  ): Promise<ManagedRelay> {
    const saved = relayId
      ? expectData(
          await updateRelay<true>(
            withData({
              path: { relay_id: relayId },
              body: body as ManagedRelayPatchRequest,
            }),
          ),
        )
      : expectData(
          await createRelay<true>(
            withData({
              body: body as ManagedRelayRequest,
            }),
          ),
        )
    await refresh()
    return saved
  }

  async function removeRelay(relayId: string): Promise<void> {
    await deleteRelay<true>(withData({ path: { relay_id: relayId } }))
    await refresh()
  }

  async function reconnectRelay(relayId: string): Promise<ManagedRelay> {
    reconnectingRelayId.value = relayId
    try {
      const relay = expectData(
        await requestRelayReconnect<true>(
          withData({
            path: { relay_id: relayId },
          }),
        ),
      )
      await refresh()
      return relay
    } finally {
      reconnectingRelayId.value = null
    }
  }

  return {
    connectedCount,
    enabledCount,
    first,
    loading,
    reconnectRelay,
    reconnectingRelayId,
    refresh,
    relays,
    removeRelay,
    saveRelay,
    rows,
    total,
  }
})
