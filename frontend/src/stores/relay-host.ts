import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import {
  relayGetSettings,
  relayHost,
  relayRequestRestart,
  relaySetHostRole,
  relaySetSettings,
  relayStatus,
} from '../generated/admin-api'
import type {
  HostRole,
  RelayHostResponse,
  RelaySettingsResponse,
  RelayStatusResponse,
} from '../generated/admin-api'
import { expectData, withData } from '../api'

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

  async function saveRole(nextRole: HostRole): Promise<RelayHostResponse> {
    savingRole.value = true
    error.value = ''
    try {
      const saved = expectData(
        await relaySetHostRole<true>(withData({ body: { role: nextRole } })),
      )
      host.value = saved
      return saved
    } catch (cause) {
      error.value =
        cause instanceof Error ? cause.message : 'Failed to save host role'
      throw cause
    } finally {
      savingRole.value = false
    }
  }

  async function saveSettings(
    adminBind: string,
  ): Promise<RelaySettingsResponse> {
    savingSettings.value = true
    error.value = ''
    try {
      const trimmed = adminBind.trim()
      const saved = expectData(
        await relaySetSettings<true>(
          withData({
            body: trimmed ? { admin_bind: trimmed } : {},
          }),
        ),
      )
      settings.value = saved
      const [hostResponse, statusResponse] = await Promise.all([
        relayHost<true>(withData()),
        relayStatus<true>(withData()),
      ])
      host.value = expectData(hostResponse)
      status.value = expectData(statusResponse)
      return saved
    } catch (cause) {
      error.value =
        cause instanceof Error ? cause.message : 'Failed to save relay settings'
      throw cause
    } finally {
      savingSettings.value = false
    }
  }

  async function requestRestart(): Promise<void> {
    restarting.value = true
    error.value = ''
    try {
      await relayRequestRestart<true>(withData())
    } catch (cause) {
      error.value =
        cause instanceof Error ? cause.message : 'Failed to request restart'
      throw cause
    } finally {
      restarting.value = false
    }
  }

  return {
    error,
    host,
    loading,
    pendingRole,
    relayReady,
    requestRestart,
    refresh,
    restartRequired,
    restarting,
    role,
    saveRole,
    saveSettings,
    savingRole,
    savingSettings,
    settings,
    status,
    workerConnected,
  }
})
