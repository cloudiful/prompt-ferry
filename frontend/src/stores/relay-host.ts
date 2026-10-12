import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import {
  relayGetSettings,
  relayHost,
  relayStatus,
} from '../generated/admin-api'
import type {
  HostRole,
  RelayHostResponse,
  RelaySettingsResponse,
  RelayStatusResponse,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import { useAsyncAction } from '../composables/useAsyncAction'
import {
  RELAY_HOST_KEY_PREFIX,
  createRelayHostActions,
} from './relay-host-actions'
import { pendingRowsById } from './record-action-types'

// Host-local Relay controls. Everything here talks to the Relay management
// listener (`/api/v1/relay/*`) and stays available when no Worker is
// connected: Worker business state is read-only display, never a gate.
export const useRelayHostStore = defineStore('relay-host', () => {
  const host = ref<RelayHostResponse | null>(null)
  const status = ref<RelayStatusResponse | null>(null)
  const settings = ref<RelaySettingsResponse | null>(null)
  const loading = ref(false)
  const savingRole = ref(false)
  const savingSettings = ref(false)
  const restarting = ref(false)
  const error = ref('')

  const role = computed<HostRole | null>(() => host.value?.role ?? null)
  const pendingRole = computed<HostRole | null>(
    () => host.value?.pending_role ?? null,
  )
  const restartRequired = computed(
    () =>
      host.value?.restart_required === true ||
      status.value?.restart_required === true ||
      settings.value?.restart_required === true,
  )
  const workerConnected = computed(
    () => status.value?.worker.connected === true,
  )
  const relayReady = computed(() => status.value?.relay_ready === true)

  async function refresh(): Promise<void> {
    loading.value = true
    error.value = ''
    try {
      const [hostResponse, statusResponse, settingsResponse] =
        await Promise.all([
          relayHost<true>(withData()),
          relayStatus<true>(withData()),
          relayGetSettings<true>(withData()),
        ])
      host.value = expectData(hostResponse)
      status.value = expectData(statusResponse)
      settings.value = expectData(settingsResponse)
    } catch (cause) {
      host.value = null
      status.value = null
      settings.value = null
      error.value =
        cause instanceof Error ? cause.message : 'Failed to load relay host'
      throw cause
    } finally {
      loading.value = false
    }
  }

  // This host is one record, so it has one gate: a role change, a settings save
  // and a restart all end in the same process restarting.
  const guard = useAsyncAction()
  const actions = createRelayHostActions({
    guard,
    host: {
      error,
      host,
      restarting,
      savingRole,
      savingSettings,
      settings,
      status,
    },
  })
  const relayHostPendingActions = computed(() =>
    pendingRowsById(guard.pendingActions.value, RELAY_HOST_KEY_PREFIX),
  )

  return {
    ...actions,
    error,
    host,
    isPending: guard.isPending,
    loading,
    pendingActions: guard.pendingActions,
    pendingRole,
    relayHostPendingActions,
    relayReady,
    refresh,
    restartRequired,
    restarting,
    role,
    savingRole,
    savingSettings,
    settings,
    status,
    workerConnected,
  }
})
