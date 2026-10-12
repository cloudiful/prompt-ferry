import type { Ref } from 'vue'

import type {
  AsyncActionGuard,
  AsyncActionOutcome,
} from '@/composables/useAsyncAction'
import type {
  ManagedRelay,
  ManagedRelayPatchRequest,
  ManagedRelayRequest,
} from '@/generated/admin-api'
import {
  createRelay,
  deleteRelay,
  reconnectRelay as requestRelayReconnect,
  updateRelay,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { capturePayload, recordActionKey } from '@/stores/record-action-types'

/**
 * Key namespace for the guarded relay writes.
 *
 * Save, delete, and reconnect of one relay share a gate. A reconnect is a write
 * in its own right — it changes the Worker's connection target — so it must not
 * race a save of the same relay, and must not be treated as a mere refresh.
 */
export const RELAY_KEY_PREFIX = 'relay'

export function relayActionKey(relayId: string | null): string {
  return recordActionKey(RELAY_KEY_PREFIX, relayId)
}

type RelaySlice = {
  relays: Ref<ManagedRelay[]>
  reconnectingRelayId: Ref<string | null>
}

export type RelayActionDeps = {
  guard: AsyncActionGuard
  refresh: () => Promise<void>
  relays: RelaySlice
}

/**
 * The relay writes.
 *
 * Remote `enabled` only ever controls the Worker's connection target; nothing
 * here calls the host role endpoint or changes this host's own role.
 */
export function createRelayActions(deps: RelayActionDeps) {
  const { guard, refresh, relays } = deps

  function saveRelay(
    relayId: string | null,
    body: ManagedRelayRequest | ManagedRelayPatchRequest,
  ): Promise<AsyncActionOutcome<ManagedRelay>> {
    const captured = capturePayload(body)
    return guard.run(
      relayActionKey(relayId),
      async () => {
        const saved = relayId
          ? expectData(
              await updateRelay<true>(
                withData({
                  path: { relay_id: relayId },
                  body: captured as ManagedRelayPatchRequest,
                }),
              ),
            )
          : expectData(
              await createRelay<true>(
                withData({ body: captured as ManagedRelayRequest }),
              ),
            )
        await refresh()
        return saved
      },
      'save',
    )
  }

  function removeRelay(relayId: string): Promise<AsyncActionOutcome<void>> {
    return guard.run(
      relayActionKey(relayId),
      async () => {
        await deleteRelay<true>(withData({ path: { relay_id: relayId } }))
        await refresh()
      },
      'delete',
    )
  }

  function reconnectRelay(
    relayId: string,
  ): Promise<AsyncActionOutcome<ManagedRelay>> {
    return guard.run(
      relayActionKey(relayId),
      async () => {
        relays.reconnectingRelayId.value = relayId
        try {
          const relay = expectData(
            await requestRelayReconnect<true>(
              withData({ path: { relay_id: relayId } }),
            ),
          )
          await refresh()
          return relay
        } finally {
          relays.reconnectingRelayId.value = null
        }
      },
      'reconnect',
    )
  }

  return { reconnectRelay, removeRelay, saveRelay }
}

export type RelayActions = ReturnType<typeof createRelayActions>
