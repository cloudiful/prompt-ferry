import { ref } from 'vue'
import { defineStore } from 'pinia'
import {
  relayAuthLogin,
  relayAuthLogout,
  relayAuthMe,
} from '../generated/admin-api'
import { expectData, withData } from '../api'

// Relay-only management session. It authenticates with the host-local
// management token and never touches the Worker `session` store: callers must
// not read `session.me`/`isAdmin` to decide Relay host access.
export const useRelaySessionStore = defineStore('relay-session', () => {
  const authenticated = ref(false)
  const bootstrapped = ref(false)
  const busy = ref(false)
  const error = ref('')

  async function bootstrap(force = false): Promise<boolean> {
    if (bootstrapped.value && !force) return authenticated.value
    busy.value = true
    error.value = ''
    try {
      const me = expectData(await relayAuthMe<true>(withData()))
      authenticated.value = me.authenticated === true
      bootstrapped.value = true
      return authenticated.value
    } catch (cause) {
      authenticated.value = false
      error.value =
        cause instanceof Error ? cause.message : 'Failed to load relay session'
      bootstrapped.value = true
      return false
    } finally {
      busy.value = false
    }
  }

  async function login(adminToken: string): Promise<void> {
    busy.value = true
    error.value = ''
    try {
      await relayAuthLogin<true>(
        withData({ body: { admin_token: adminToken } }),
      )
      await bootstrap(true)
    } catch (cause) {
      authenticated.value = false
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
      await relayAuthLogout<true>(withData())
    } finally {
      authenticated.value = false
      bootstrapped.value = false
      busy.value = false
    }
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
