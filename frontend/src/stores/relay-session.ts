import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import {
  relayAuthLogin,
  relayAuthLogout,
  relayAuthMe,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import { useAsyncAction } from '../composables/useAsyncAction'
import type { AsyncActionOutcome } from '../composables/useAsyncAction'

// Relay-only management session. It authenticates with the host-local
// management token and never touches the Worker `session` store: callers must
// not read `session.me`/`isAdmin` to decide Relay host access.
const AUTH_ACTION_KEY = 'relay-session:authentication'

export const useRelaySessionStore = defineStore('relay-session', () => {
  const authenticated = ref(false)
  const bootstrapped = ref(false)
  const bootstrapping = ref(false)
  const error = ref('')
  const guard = useAsyncAction()
  const busy = computed(
    () => bootstrapping.value || guard.isPending(AUTH_ACTION_KEY),
  )
  let bootstrapGeneration = 0

  async function bootstrap(force = false): Promise<boolean> {
    if (bootstrapped.value && !force) return authenticated.value
    const generation = ++bootstrapGeneration
    bootstrapping.value = true
    error.value = ''
    try {
      const me = expectData(await relayAuthMe<true>(withData()))
      if (generation !== bootstrapGeneration) return false
      authenticated.value = me.authenticated === true
      bootstrapped.value = true
      return authenticated.value
    } catch (cause) {
      if (generation !== bootstrapGeneration) return false
      authenticated.value = false
      error.value =
        cause instanceof Error ? cause.message : 'Failed to load relay session'
      bootstrapped.value = true
      return false
    } finally {
      if (generation === bootstrapGeneration) bootstrapping.value = false
    }
  }

  function login(adminToken: string): Promise<AsyncActionOutcome<void>> {
    const capturedToken = adminToken
    return guard.run(
      AUTH_ACTION_KEY,
      async () => {
        error.value = ''
        try {
          await relayAuthLogin<true>(
            withData({ body: { admin_token: capturedToken } }),
          )
          const loaded = await bootstrap(true)
          if (!loaded) {
            throw new Error(error.value || 'Failed to load relay session')
          }
        } catch (cause) {
          authenticated.value = false
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
          await relayAuthLogout<true>(withData())
        } finally {
          bootstrapGeneration += 1
          authenticated.value = false
          bootstrapped.value = false
          bootstrapping.value = false
        }
      },
      'logout',
    )
  }

  return {
    authenticated,
    bootstrapped,
    bootstrap,
    busy,
    error,
    login,
    logout,
  }
})
