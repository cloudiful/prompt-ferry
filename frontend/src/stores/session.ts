import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import {
  authLogin,
  authLogout,
  authMe,
  bridgeStatus,
} from '../generated/admin-api'
import type { BridgeStatus, MeResponse } from '../generated/admin-api'
import { expectData, withData } from '../api'

type LoginPayload = {
  login_name: string
  password: string
}

export const useSessionStore = defineStore('session', () => {
  const me = ref<MeResponse | null>(null)
  const status = ref<BridgeStatus | null>(null)
  const bootstrapped = ref(false)
  const busy = ref(false)
  const statusBusy = ref(false)
  const error = ref('')
  const statusError = ref('')

  const isAdmin = computed(() => me.value?.is_admin ?? false)
  const loginName = computed(() => me.value?.login_name ?? '')
  const displayName = computed(() => me.value?.display_name ?? '')

  async function bootstrapAuth(force = false): Promise<void> {
    if (bootstrapped.value && !force) return
    busy.value = true
    error.value = ''
    try {
      me.value = expectData(await authMe<true>(withData()))
      bootstrapped.value = true
    } catch (cause) {
      me.value = null
      error.value =
        cause instanceof Error ? cause.message : 'Failed to load session'
      bootstrapped.value = true
    } finally {
      busy.value = false
    }
  }

  async function refreshBridgeStatus(): Promise<void> {
    statusBusy.value = true
    statusError.value = ''
    try {
      status.value = expectData(await bridgeStatus<true>(withData()))
    } catch (cause) {
      status.value = null
      statusError.value =
        cause instanceof Error ? cause.message : 'Failed to load bridge status'
    } finally {
      statusBusy.value = false
    }
  }

  async function login(payload: LoginPayload): Promise<void> {
    busy.value = true
    error.value = ''
    try {
      await authLogin<true>(withData({ body: payload }))
      await bootstrapAuth(true)
      await refreshBridgeStatus()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : 'Login failed'
      throw cause
    } finally {
      busy.value = false
    }
  }

  async function logout(): Promise<void> {
    busy.value = true
    error.value = ''
    try {
      await authLogout<true>(withData())
    } finally {
      me.value = null
      status.value = null
      bootstrapped.value = false
      busy.value = false
      statusBusy.value = false
      statusError.value = ''
    }
  }

  return {
    busy,
    bootstrapped,
    displayName,
    error,
    isAdmin,
    login,
    loginName,
    logout,
    me,
    bootstrapAuth,
    refreshBridgeStatus,
    statusBusy,
    statusError,
    status,
  }
})
