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
import { useAsyncAction } from '../composables/useAsyncAction'
import type { AsyncActionOutcome } from '../composables/useAsyncAction'

type LoginPayload = {
  login_name: string
  password: string
}

const AUTH_ACTION_KEY = 'session:authentication'

export const useSessionStore = defineStore('session', () => {
  const me = ref<MeResponse | null>(null)
  const status = ref<BridgeStatus | null>(null)
  const bootstrapped = ref(false)
  const bootstrapping = ref(false)
  const statusBusy = ref(false)
  const error = ref('')
  const statusError = ref('')
  const guard = useAsyncAction()
  const busy = computed(
    () => bootstrapping.value || guard.isPending(AUTH_ACTION_KEY),
  )
  let bootstrapGeneration = 0

  const isAdmin = computed(() => me.value?.is_admin ?? false)
  const loginName = computed(() => me.value?.login_name ?? '')
  const displayName = computed(() => me.value?.display_name ?? '')

  async function bootstrapAuth(force = false): Promise<boolean> {
    if (bootstrapped.value && !force) return true
    const generation = ++bootstrapGeneration
    bootstrapping.value = true
    error.value = ''
    try {
      const next = expectData(await authMe<true>(withData()))
      if (generation !== bootstrapGeneration) return false
      me.value = next
      bootstrapped.value = true
      return true
    } catch (cause) {
      if (generation !== bootstrapGeneration) return false
      me.value = null
      error.value =
        cause instanceof Error ? cause.message : 'Failed to load session'
      bootstrapped.value = true
      return false
    } finally {
      if (generation === bootstrapGeneration) bootstrapping.value = false
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

  function login(payload: LoginPayload): Promise<AsyncActionOutcome<void>> {
    const captured = {
      login_name: payload.login_name,
      password: payload.password,
    }
    return guard.run(
      AUTH_ACTION_KEY,
      async () => {
        error.value = ''
        try {
          await authLogin<true>(withData({ body: captured }))
          const loaded = await bootstrapAuth(true)
          if (!loaded) {
            throw new Error(error.value || 'Failed to load session')
          }
          await refreshBridgeStatus()
        } catch (cause) {
          error.value = cause instanceof Error ? cause.message : 'Login failed'
          throw cause
        }
      },
      'login',
    )
  }

  function logout(): Promise<AsyncActionOutcome<void>> {
    return guard.run(
      AUTH_ACTION_KEY,
      async () => {
        error.value = ''
        try {
          await authLogout<true>(withData())
        } finally {
          bootstrapGeneration += 1
          me.value = null
          status.value = null
          bootstrapped.value = false
          bootstrapping.value = false
          statusBusy.value = false
          statusError.value = ''
        }
      },
      'logout',
    )
  }

  return {
    bootstrapped,
    bootstrapAuth,
    busy,
    displayName,
    error,
    isAdmin,
    login,
    loginName,
    logout,
    me,
    refreshBridgeStatus,
    status,
    statusBusy,
    statusError,
  }
})
