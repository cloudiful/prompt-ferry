/**
 * The admin cache-alert settings tab: the DingTalk alert policy, read and saved
 * by hand.
 *
 * The shape follows the accepted network, review and storage ports: one scoped
 * read through the app query client with a read generation, one guarded write
 * under `setting:cacheAlert` with the form captured before the first await, a
 * per-leaf merge that keeps a newer edit, a secret draft cleared only where a
 * save consumed it, and a success report only for a write that reached the
 * server.
 *
 * Two things are specific to this tab. The DingTalk secret is write-only: the
 * server echoes only `has_dingtalk_secret`, which stays the sole source of the
 * presence badge, while the local draft decides the keep/replace mode. And the
 * whole setting is unavailable on a backend that cannot keep it, which the
 * server answers with the structured `capability_unavailable` code; that becomes
 * an explicit state, so the warning and its retry read do not depend on matching
 * an error message.
 */

import type { QueryClient } from '@tanstack/react-query'
import { create } from 'zustand'
import type {
  CacheAlertSettingsResponse,
  MeResponse,
} from '@/generated/admin-api'
import {
  getCacheAlertSetting,
  setCacheAlertSetting,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { capturePayload } from '@/stores/record-action-types'
import { runAsyncAction, type AsyncActionOutcome } from './async-action'
import {
  CACHE_ALERT_KEY,
  cacheAlertDirty,
  cacheAlertFormToRequest,
  clearSavedCacheAlertSecret,
  createCacheAlertForm,
  formFromCacheAlertResponse,
  isCacheAlertCapabilityUnavailable,
  mergeCacheAlertResponse,
  setCacheAlertSecretDraft,
  type CacheAlertForm,
} from './settings-cache-alert-model'

const SETTINGS_CACHE_ALERT_NAMESPACE = ['settings-cache-alert'] as const
const CACHE_ALERT_QUERY_KEY = [
  ...SETTINGS_CACHE_ALERT_NAMESPACE,
  'cache-alert',
] as const

type SettingsIdentity = Pick<MeResponse, 'user_id' | 'login_name' | 'is_admin'>

export function cacheAlertIdentityKey(
  identity: SettingsIdentity | null,
): string {
  if (!identity) return ''
  return `${identity.user_id}:${identity.login_name}:${identity.is_admin}`
}

export type SettingsCacheAlertState = {
  identityKey: string
  form: CacheAlertForm
  baseline: CacheAlertForm
  loaded: boolean
  loading: boolean
  loadError: boolean
  unavailable: boolean
  unavailableMessage: string
  saveFailed: boolean
  initializeIdentity: (identity: SettingsIdentity | null) => void
  resetIdentity: (identity: SettingsIdentity | null) => void
  ensureLoaded: (queryClient: QueryClient) => Promise<void>
  refresh: (queryClient: QueryClient) => Promise<void>
  setField: <K extends Exclude<keyof CacheAlertForm, 'has_dingtalk_secret'>>(
    field: K,
    value: CacheAlertForm[K],
  ) => void
  setSecretDraft: (value: string) => void
  save: () => Promise<AsyncActionOutcome<void>>
}

let readGeneration = 0
let reportedNotifier: {
  notifyApiError: (cause: unknown) => void
  notifySaved: () => void
} | null = null

/**
 * Register the notifications the tab reports through, so the store never reaches
 * into a React context and the tab owns the notification surface.
 */
export function setSettingsCacheAlertNotifier(
  notifier: {
    notifyApiError: (cause: unknown) => void
    notifySaved: () => void
  } | null,
): void {
  reportedNotifier = notifier
}

/** Drop a read that is still in flight once the operator edits or saves. */
function invalidatePendingRead(): void {
  if (!useSettingsCacheAlertStore.getState().loading) return
  ++readGeneration
  useSettingsCacheAlertStore.setState({ loading: false })
}

/**
 * Drop the tab: an in-flight read commits nothing, and the form — which holds
 * the operator's secret draft — is emptied so nothing sensitive survives the tab
 * on the module that outlives it.
 */
export function disposeSettingsCacheAlert(): void {
  ++readGeneration
  useSettingsCacheAlertStore.setState({
    form: createCacheAlertForm(),
    baseline: createCacheAlertForm(),
    loaded: false,
    loading: false,
    loadError: false,
    unavailable: false,
    unavailableMessage: '',
    saveFailed: false,
  })
}

async function fetchFresh(
  queryClient: QueryClient,
): Promise<CacheAlertSettingsResponse> {
  await queryClient.cancelQueries({
    queryKey: CACHE_ALERT_QUERY_KEY,
    exact: true,
  })
  await queryClient.invalidateQueries({
    queryKey: SETTINGS_CACHE_ALERT_NAMESPACE,
    refetchType: 'none',
  })
  return queryClient.fetchQuery({
    queryKey: CACHE_ALERT_QUERY_KEY,
    queryFn: async () =>
      expectData(await getCacheAlertSetting<true>(withData())),
    staleTime: 0,
  })
}

export const useSettingsCacheAlertStore = create<SettingsCacheAlertState>(
  (set, get) => ({
    identityKey: '',
    form: createCacheAlertForm(),
    baseline: createCacheAlertForm(),
    loaded: false,
    loading: false,
    loadError: false,
    unavailable: false,
    unavailableMessage: '',
    saveFailed: false,

    initializeIdentity(identity) {
      const key = cacheAlertIdentityKey(identity)
      if (get().identityKey === key) return
      get().resetIdentity(identity)
    },

    resetIdentity(identity) {
      ++readGeneration
      set({
        identityKey: cacheAlertIdentityKey(identity),
        form: createCacheAlertForm(),
        baseline: createCacheAlertForm(),
        loaded: false,
        loading: false,
        loadError: false,
        unavailable: false,
        unavailableMessage: '',
        saveFailed: false,
      })
    },

    async ensureLoaded(queryClient) {
      const state = get()
      if (state.identityKey === '' || state.loaded || state.loading) return
      await get().refresh(queryClient)
    },

    async refresh(queryClient) {
      const state = get()
      if (state.identityKey === '') return
      if (cacheAlertDirty(state.form, state.baseline)) return
      const identityKey = state.identityKey
      const generation = ++readGeneration
      set({ loading: true, loadError: false })
      try {
        const response = await fetchFresh(queryClient)
        if (generation !== readGeneration) return
        if (get().identityKey !== identityKey) return
        const form = formFromCacheAlertResponse(response)
        set({
          form,
          baseline: form,
          loaded: true,
          loading: false,
          loadError: false,
          unavailable: false,
          unavailableMessage: '',
          saveFailed: false,
        })
      } catch (cause) {
        if (generation !== readGeneration) return
        if (get().identityKey !== identityKey) return
        const unavailable = isCacheAlertCapabilityUnavailable(cause)
        set({
          loading: false,
          loadError: true,
          unavailable,
          unavailableMessage: unavailable ? formatCause(cause) : '',
        })
      }
    },

    setField(field, value) {
      invalidatePendingRead()
      set({ form: { ...get().form, [field]: value } })
    },

    setSecretDraft(value) {
      invalidatePendingRead()
      set({ form: setCacheAlertSecretDraft(get().form, value) })
    },

    async save() {
      const state = get()
      // Saving before a successful read would submit the defaults over a
      // configured policy.
      if (state.identityKey === '' || !state.loaded || state.unavailable) {
        return { executed: false }
      }
      const identityKey = state.identityKey
      const submitted = capturePayload(state.form)
      invalidatePendingRead()

      try {
        const outcome = await runAsyncAction(
          CACHE_ALERT_KEY,
          async () => {
            const body = cacheAlertFormToRequest(submitted)
            const response = expectData(
              await setCacheAlertSetting<true>(withData({ body })),
            )
            if (get().identityKey !== identityKey) return
            const latest = get()
            set({
              form: clearSavedCacheAlertSecret(
                mergeCacheAlertResponse(latest.form, submitted, response),
                submitted,
              ),
              // The baseline is the last accepted value, per leaf: a leaf or a
              // secret draft edited during the write stays dirty against it.
              baseline: formFromCacheAlertResponse(response),
              loaded: true,
              saveFailed: false,
            })
          },
          'cacheAlert',
        )
        if (outcome.executed) {
          set({ saveFailed: false })
          reportedNotifier?.notifySaved()
        }
        return outcome
      } catch (cause) {
        set({ saveFailed: true })
        reportedNotifier?.notifyApiError(cause)
        return { executed: false }
      }
    },
  }),
)

/** A capability answer's own text, without any request payload. */
function formatCause(cause: unknown): string {
  if (cause && typeof cause === 'object' && 'error' in cause) {
    const envelope = (cause as { error?: unknown }).error
    if (envelope && typeof envelope === 'object') {
      const code = (envelope as { code?: unknown }).code
      const message = (envelope as { message?: unknown }).message
      if (typeof code === 'string' && typeof message === 'string') {
        return `${code}: ${message}`
      }
      if (typeof message === 'string') return message
    }
  }
  return ''
}
