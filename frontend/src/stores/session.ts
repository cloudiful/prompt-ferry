import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import {
  authLogin,
  authLogout,
  authMe,
  bridgeStatus,
} from '../generated/admin-api'
import type { BridgeStatus, MeResponse } from '../generated/admin-api'
import { expectData, formatApiError, withData } from '../api'
import { useAsyncAction } from '../composables/useAsyncAction'
import type { AsyncActionOutcome } from '../composables/useAsyncAction'
import { i18n } from '../i18n/plugin'

type LoginPayload = {
  login_name: string
  password: string
}

const AUTH_ACTION_KEY = 'session:authentication'

function errorCode(cause: unknown): string | null {
  if (!cause || typeof cause !== 'object') return null
  const envelope = (cause as { error?: unknown }).error
  if (!envelope || typeof envelope !== 'object') return null
  const code = (envelope as { code?: unknown }).code
  return typeof code === 'string' ? code : null
}

function isUnauthorized(cause: unknown): boolean {
  return errorCode(cause) === 'unauthorized'
}

const translate = (i18n.global as unknown as { t: (key: string) => string }).t

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
      // A 401 on the session probe is the anonymous state, not a failure to
      // render the login page; real network and server errors stay visible.
      error.value = isUnauthorized(cause) ? '' : formatApiError(cause)
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
            if (!error.value) {
              error.value = translate('sessionLoadFailed')
            }
            throw new Error(error.value)
          }
          await refreshBridgeStatus()
        } catch (cause) {
          error.value = isUnauthorized(cause)
            ? translate('invalidCredentials')
            : formatApiError(cause)
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
