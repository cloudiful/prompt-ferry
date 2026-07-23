import { computed, ref } from 'vue'
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

export const useRelaysStore = defineStore('relays', () => {
  const relays = ref<ManagedRelay[]>([])
  const loading = ref(false)
  const reconnectingRelayId = ref<string | null>(null)

  const connectedCount = computed(
    () => relays.value.filter((relay) => relay.connected).length,
  )
  const enabledCount = computed(
    () => relays.value.filter((relay) => relay.enabled).length,
  )

  async function refresh(): Promise<void> {
    loading.value = true
    try {
      const response = expectData(await listRelays<true>(withData()))
      relays.value = response.relays
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
    loading,
    reconnectRelay,
    reconnectingRelayId,
    refresh,
    relays,
    removeRelay,
    saveRelay,
  }
})
