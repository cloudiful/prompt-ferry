import { create } from 'zustand'
import {
  authLogin,
  authLogout,
  authMe,
  bridgeStatus,
  setupInitialize,
} from '@/generated/admin-api'
import type {
  BridgeStatus,
  MeResponse,
  SetupStatus,
} from '@/generated/admin-api'
import { expectData, formatApiError, withData } from '@/api'
import { runAsyncAction, type AsyncActionOutcome } from './async-action'
import { messages, normalizeLocale, type MessageKey } from '@/i18n'
import { LOCALE_STORAGE_KEY, readStorage } from '@/storage'

export const SESSION_AUTH_KEY = 'session:authentication'

function localizedMessage(key: MessageKey): string {
  const locale = normalizeLocale(readStorage(LOCALE_STORAGE_KEY))
  return messages[locale][key]
}

function isUnauthorized(cause: unknown): boolean {
  if (!cause || typeof cause !== 'object' || !('error' in cause)) return false
  const error = (cause as { error?: unknown }).error
  return Boolean(
    error &&
    typeof error === 'object' &&
    'code' in error &&
    (error as { code?: unknown }).code === 'unauthorized',
  )
}

function isConflict(cause: unknown): boolean {
  if (!cause || typeof cause !== 'object' || !('error' in cause)) return false
  const error = (cause as { error?: unknown }).error
  return Boolean(
    error &&
    typeof error === 'object' &&
    'code' in error &&
    (error as { code?: unknown }).code === 'conflict',
  )
}

export type LoginPayload = {
  login_name: string
  password: string
}

export type SetupPayload = {
  master_secret: string
  login_name: string
  password: string
  display_name: string | null
}

type SessionState = {
  me: MeResponse | null
  bridgeStatus: BridgeStatus | null
  bootstrapped: boolean
  bootstrapping: boolean
  error: string
  setupError: string
  bridgeStatusError: string
  /**
   * The authenticated session epoch.
   *
   * It advances exactly when the identity behind the session changes, so a
   * consumer can use it as the boundary of one authenticated session: a new
   * principal, a new role, a new login of the same principal, and the end of
   * a session are all different epochs.
   */
  epoch: number
  bootstrapAuth: (force?: boolean) => Promise<boolean>
  refreshBridgeStatus: () => Promise<void>
  setup: (
    payload: SetupPayload,
    refreshStatus: (status?: SetupStatus) => Promise<void>,
  ) => Promise<AsyncActionOutcome<SetupStatus>>
  login: (payload: LoginPayload) => Promise<AsyncActionOutcome<void>>
  logout: () => Promise<AsyncActionOutcome<void>>
}

let sessionGeneration = 0

/**
 * The epoch that follows a successful authentication.
 *
 * A forced bootstrap is a new authenticated session even when the principal
 * and the role are unchanged, so it always advances. An ordinary bootstrap
 * advances only when the identity behind the session really changed.
 */
function epochAfterAuthentication(
  state: SessionState,
  me: MeResponse,
  force: boolean,
): number {
  const current = state.me
  const changed =
    current === null ||
    current.user_id !== me.user_id ||
    current.is_admin !== me.is_admin
  return force || changed ? state.epoch + 1 : state.epoch
}

export const useSessionStore = create<SessionState>((set, get) => ({
  me: null,
  bridgeStatus: null,
  bootstrapped: false,
  bootstrapping: false,
  error: '',
  setupError: '',
  bridgeStatusError: '',
  epoch: 0,

  async bootstrapAuth(force = false) {
    if (get().bootstrapped && !force) return true
    const generation = ++sessionGeneration
    set({ bootstrapping: true, error: '' })
    try {
      const me = expectData(await authMe<true>(withData()))
      if (generation !== sessionGeneration) return false
      // The identity and the epoch move together, so no render ever sees the
      // new identity under the previous epoch.
      set({
        me,
        bootstrapped: true,
        epoch: epochAfterAuthentication(get(), me, force),
      })
      return true
    } catch (cause) {
      if (generation !== sessionGeneration) return false
      const wasAuthenticated = get().me !== null
      set({
        me: null,
        error: isUnauthorized(cause) ? '' : formatApiError(cause),
        bootstrapped: true,
        epoch: wasAuthenticated ? get().epoch + 1 : get().epoch,
      })
      return false
    } finally {
      if (generation === sessionGeneration) set({ bootstrapping: false })
    }
  },

  async refreshBridgeStatus() {
    set({ bridgeStatusError: '' })
    try {
      const status = expectData(await bridgeStatus<true>(withData()))
      set({ bridgeStatus: status })
    } catch (cause) {
      set({
        bridgeStatus: null,
        bridgeStatusError:
          cause instanceof Error
            ? cause.message
            : 'Failed to load bridge status',
      })
    }
  },

  setup(payload, refreshStatus) {
    const captured: SetupPayload = {
      master_secret: payload.master_secret,
      login_name: payload.login_name,
      password: payload.password,
      display_name: payload.display_name,
    }
    return runAsyncAction(
      SESSION_AUTH_KEY,
      async () => {
        set({ setupError: '' })
        try {
          const status = expectData(
            await setupInitialize<true>(withData({ body: captured })),
          )
          await refreshStatus(status)
          return status
        } catch (cause) {
          if (isConflict(cause)) {
            await refreshStatus().catch(() => undefined)
          }
          set({ setupError: formatApiError(cause) })
          throw cause
        }
      },
      'setup',
    )
  },

  login(payload) {
    const captured = {
      login_name: payload.login_name,
      password: payload.password,
    }
    return runAsyncAction(
      SESSION_AUTH_KEY,
      async () => {
        set({ error: '' })
        try {
          await authLogin<true>(withData({ body: captured }))
          const loaded = await get().bootstrapAuth(true)
          if (!loaded) {
            if (!get().error)
              set({ error: localizedMessage('sessionLoadFailed') })
            throw new Error(get().error)
          }
          await get().refreshBridgeStatus()
        } catch (cause) {
          set({
            error: isUnauthorized(cause)
              ? localizedMessage('invalidCredentials')
              : formatApiError(cause),
          })
          throw cause
        }
      },
      'login',
    )
  },

  logout() {
    return runAsyncAction(
      SESSION_AUTH_KEY,
      async () => {
        set({ error: '' })
        try {
          await authLogout<true>(withData())
        } finally {
          sessionGeneration += 1
          // Ending a session always ends its epoch, even when the session was
          // already empty.
          set({
            me: null,
            bridgeStatus: null,
            bootstrapped: false,
            bootstrapping: false,
            bridgeStatusError: '',
            epoch: get().epoch + 1,
          })
        }
      },
      'logout',
    )
  },
}))
