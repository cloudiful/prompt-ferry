import { ref } from 'vue'
import { defineStore } from 'pinia'
import { listRelays } from '../generated/admin-api'
import type { ManagedRelay } from '../generated/admin-api'
import { expectData, withData } from '../api'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'
import { useAsyncAction } from '../composables/useAsyncAction'
import { RELAY_KEY_PREFIX, createRelayActions } from './relays-actions'
import { pendingRowsById } from './record-action-types'
import { computed } from 'vue'

export const useRelaysStore = defineStore('relays', () => {
  const relays = ref<ManagedRelay[]>([])
  const loading = ref(false)
  const reconnectingRelayId = ref<string | null>(null)
  const first = ref(0)
  const rows = useStoredPageSize('relays', 10, STANDARD_PAGE_SIZE_OPTIONS)
  const total = ref(0)
  const connectedCount = ref(0)
  const enabledCount = ref(0)
  // Scoped Worker availability: the remote list is Worker business state.
  // A failed refresh keeps the last list and records the reason instead of
  // failing the page; it never touches the host-local role.
  const lastError = ref<string | null>(null)

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
      lastError.value = null
      if (
        relays.value.length === 0 &&
        total.value > 0 &&
        first.value >= total.value
      ) {
        const previousFirst =
          Math.floor((total.value - 1) / rows.value) * rows.value
        await refresh(previousFirst, rows.value)
      }
    } catch (cause) {
      lastError.value =
        cause instanceof Error ? cause.message : 'Failed to load relays'
      throw cause
    } finally {
      loading.value = false
    }
  }

  // Remote `enabled` only controls the Worker's connection target. It never
  // calls the host role endpoint and never changes this host's role.
  //
  // The guard lives here rather than at each call site, so a relay can be gated
  // against every operation that would write it.
  const guard = useAsyncAction()
  const actions = createRelayActions({
    guard,
    refresh: () => refresh(),
    relays: { relays, reconnectingRelayId },
  })

  // Per-row view of the guard. `loading` stays the fetch-only busy flag, so a
  // write never blanks the whole page.
  const relayPendingActions = computed(() =>
    pendingRowsById(guard.pendingActions.value, RELAY_KEY_PREFIX),
  )

  return {
    ...actions,
    connectedCount,
    enabledCount,
    first,
    isPending: guard.isPending,
    lastError,
    loading,
    pendingActions: guard.pendingActions,
    reconnectingRelayId,
    refresh,
    relayPendingActions,
    relays,
    rows,
    total,
  }
})
