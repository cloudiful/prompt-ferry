import type { Ref } from 'vue'

import type {
  AsyncActionGuard,
  AsyncActionOutcome,
} from '@/composables/useAsyncAction'
import type {
  HostRole,
  RelayHostResponse,
  RelaySettingsResponse,
  RelayStatusResponse,
} from '@/generated/admin-api'
import {
  relayHost,
  relayRequestRestart,
  relaySetHostRole,
  relaySetSettings,
  relayStatus,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { recordActionKey } from '@/stores/record-action-types'

/**
 * Key namespace for the guarded Relay-host writes.
 *
 * The Relay host is this process, so there is exactly one of it and its id is
 * fixed. Role, settings, and restart share that one key: all three end in a
 * restart of the same process, so running two of them at once would describe a
 * configuration the operator never set.
 */
export const RELAY_HOST_KEY_PREFIX = 'host'

/** This process is the only Relay host, so its record id never varies. */
export const LOCAL_HOST_ID = 'local'

export function relayHostActionKey(): string {
  return recordActionKey(RELAY_HOST_KEY_PREFIX, LOCAL_HOST_ID)
}

type RelayHostSlice = {
  host: Ref<RelayHostResponse | null>
  settings: Ref<RelaySettingsResponse | null>
  status: Ref<RelayStatusResponse | null>
  error: Ref<string>
  savingRole: Ref<boolean>
  savingSettings: Ref<boolean>
  restarting: Ref<boolean>
}

export type RelayHostActionDeps = {
  guard: AsyncActionGuard
  host: RelayHostSlice
}

/**
 * Host-local Relay controls. Everything here talks to the Relay management
 * listener and stays available when no Worker is connected: Worker business
 * state is read-only display, never a gate.
 */
export function createRelayHostActions(deps: RelayHostActionDeps) {
  const { guard, host: hostState } = deps

  function saveRole(
    nextRole: HostRole,
  ): Promise<AsyncActionOutcome<RelayHostResponse>> {
    return guard.run(
      relayHostActionKey(),
      async () => {
        hostState.savingRole.value = true
        hostState.error.value = ''
        try {
          const saved = expectData(
            await relaySetHostRole<true>(
              withData({ body: { role: nextRole } }),
            ),
          )
          hostState.host.value = saved
          return saved
        } catch (cause) {
          hostState.error.value =
            cause instanceof Error ? cause.message : 'Failed to save host role'
          throw cause
        } finally {
          hostState.savingRole.value = false
        }
      },
      'role',
    )
  }

  function saveSettings(
    adminBind: string,
  ): Promise<AsyncActionOutcome<RelaySettingsResponse>> {
    // Trimmed before the claim, so the snapshot sent is the one the operator
    // confirmed rather than whatever the field holds when the request settles.
    const trimmed = adminBind.trim()
    return guard.run(
      relayHostActionKey(),
      async () => {
        hostState.savingSettings.value = true
        hostState.error.value = ''
        try {
          const saved = expectData(
            await relaySetSettings<true>(
              withData({
                body: trimmed ? { admin_bind: trimmed } : {},
              }),
            ),
          )
          hostState.settings.value = saved
          const [hostResponse, statusResponse] = await Promise.all([
            relayHost<true>(withData()),
            relayStatus<true>(withData()),
          ])
          hostState.host.value = expectData(hostResponse)
          hostState.status.value = expectData(statusResponse)
          return saved
        } catch (cause) {
          hostState.error.value =
            cause instanceof Error
              ? cause.message
              : 'Failed to save relay settings'
          throw cause
        } finally {
          hostState.savingSettings.value = false
        }
      },
      'settings',
    )
  }

  function requestRestart(): Promise<AsyncActionOutcome<void>> {
    return guard.run(
      relayHostActionKey(),
      async () => {
        hostState.restarting.value = true
        hostState.error.value = ''
        try {
          await relayRequestRestart<true>(withData())
        } catch (cause) {
          hostState.error.value =
            cause instanceof Error ? cause.message : 'Failed to request restart'
          throw cause
        } finally {
          hostState.restarting.value = false
        }
      },
      'restart',
    )
  }

  return { requestRestart, saveRole, saveSettings }
}

export type RelayHostActions = ReturnType<typeof createRelayHostActions>
