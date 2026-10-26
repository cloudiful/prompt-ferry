/**
 * The admin Network settings tab: the relay IP whitelist, read and saved by
 * hand.
 *
 * Unlike the requests tab this section is not autosaved: the operator edits two
 * textareas and presses Save. Everything else follows the rules the requests
 * port established: one scoped read through the app query client, a read
 * generation so a superseded response cannot commit, one guarded write under the
 * shared `setting:relayIpWhitelist` key, the form and body captured before the
 * first await, and a success report only for a write that reached the server.
 */

import type { QueryClient } from '@tanstack/react-query'
import { create } from 'zustand'
import type { MeResponse, RelayIpPolicyResponse } from '@/generated/admin-api'
import { getRelayIpWhitelist, setRelayIpWhitelist } from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { capturePayload } from '@/stores/record-action-types'
import { runAsyncAction, type AsyncActionOutcome } from './async-action'
import {
  createRelayIpWhitelistForm,
  mergeRelayIpWhitelistResponse,
  relayPolicyToWhitelistForm,
  relayWhitelistDirtyFields,
  relayWhitelistFormToRequest,
  RELAY_IP_WHITELIST_KEY,
  type RelayIpWhitelistForm,
} from './settings-network-model'

const SETTINGS_NETWORK_NAMESPACE = ['settings-network'] as const
const RELAY_IP_WHITELIST_QUERY_KEY = [
  ...SETTINGS_NETWORK_NAMESPACE,
  'relay-ip-whitelist',
] as const

type SettingsIdentity = Pick<MeResponse, 'user_id' | 'login_name' | 'is_admin'>

export function networkIdentityKey(identity: SettingsIdentity | null): string {
  if (!identity) return ''
  return `${identity.user_id}:${identity.login_name}:${identity.is_admin}`
}

type SettingsNetworkState = {
  identityKey: string
  form: RelayIpWhitelistForm
  baseline: RelayIpWhitelistForm
  loaded: boolean
  loading: boolean
  loadError: boolean
  saveFailed: boolean
  initializeIdentity: (identity: SettingsIdentity | null) => void
  resetIdentity: (identity: SettingsIdentity | null) => void
  ensureLoaded: (queryClient: QueryClient) => Promise<void>
  refresh: (queryClient: QueryClient) => Promise<void>
  setField: (field: keyof RelayIpWhitelistForm, value: string) => void
  save: () => Promise<AsyncActionOutcome<void>>
}

let readGeneration = 0
let reportedNotifier: {
  notifyError: (cause: unknown) => void
  notifySaved: () => void
} | null = null

/**
 * Register the notifier the tab reports through, so the store never reaches into
 * a React context and the tab owns the notification surface.
 */
export function setSettingsNetworkNotifier(
  notifier: {
    notifyError: (cause: unknown) => void
    notifySaved: () => void
  } | null,
): void {
  reportedNotifier = notifier
}

/** Drop a read that is still in flight once the operator edits. */
function invalidatePendingRead(): void {
  if (!useSettingsNetworkStore.getState().loading) return
  ++readGeneration
  useSettingsNetworkStore.setState({ loading: false })
}

/**
 * Drop an in-flight read on unmount.
 *
 * The store outlives the tab, so a response that arrives after the operator left
 * must commit nothing and must not leave the loading flag behind.
 */
export function disposeSettingsNetwork(): void {
  ++readGeneration
  if (useSettingsNetworkStore.getState().loading) {
    useSettingsNetworkStore.setState({ loading: false })
  }
}

async function fetchFresh(
  queryClient: QueryClient,
): Promise<RelayIpPolicyResponse> {
  await queryClient.cancelQueries({
    queryKey: RELAY_IP_WHITELIST_QUERY_KEY,
    exact: true,
  })
  await queryClient.invalidateQueries({
    queryKey: SETTINGS_NETWORK_NAMESPACE,
    refetchType: 'none',
  })
  return queryClient.fetchQuery({
    queryKey: RELAY_IP_WHITELIST_QUERY_KEY,
    queryFn: async () =>
      expectData(await getRelayIpWhitelist<true>(withData())),
    staleTime: 0,
  })
}

export const useSettingsNetworkStore = create<SettingsNetworkState>(
  (set, get) => ({
    identityKey: '',
    form: createRelayIpWhitelistForm(),
    baseline: createRelayIpWhitelistForm(),
    loaded: false,
    loading: false,
    loadError: false,
    saveFailed: false,

    initializeIdentity(identity) {
      const key = networkIdentityKey(identity)
      if (get().identityKey === key) return
      get().resetIdentity(identity)
    },

    resetIdentity(identity) {
      ++readGeneration
      set({
        identityKey: networkIdentityKey(identity),
        form: createRelayIpWhitelistForm(),
        baseline: createRelayIpWhitelistForm(),
        loaded: false,
        loading: false,
        loadError: false,
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
      // A refresh never discards an unsaved draft.
      if (relayWhitelistDirtyFields(state.form, state.baseline).length > 0) {
        return
      }
      const identityKey = state.identityKey
      const generation = ++readGeneration
      set({ loading: true, loadError: false })
      try {
        const policy = await fetchFresh(queryClient)
        if (generation !== readGeneration) return
        if (get().identityKey !== identityKey) return
        const form = relayPolicyToWhitelistForm(policy)
        set({
          form,
          baseline: form,
          loaded: true,
          loading: false,
          loadError: false,
          saveFailed: false,
        })
      } catch {
        if (generation !== readGeneration) return
        if (get().identityKey !== identityKey) return
        set({ loading: false, loadError: true })
      }
    },

    setField(field, value) {
      invalidatePendingRead()
      set({ form: { ...get().form, [field]: value } })
    },

    async save() {
      const identityKey = get().identityKey
      if (identityKey === '') return { executed: false }
      const submitted = capturePayload(get().form)
      const body = relayWhitelistFormToRequest(submitted)
      // A read that started before this save describes the pre-write value.
      invalidatePendingRead()
      try {
        const outcome = await runAsyncAction(
          RELAY_IP_WHITELIST_KEY,
          async () => {
            const response = expectData(
              await setRelayIpWhitelist<true>(withData({ body })),
            )
            if (get().identityKey !== identityKey) return
            const latest = get()
            set({
              // The accepted policy replaces the textareas only where the
              // operator has not edited again since the submit.
              form: mergeRelayIpWhitelistResponse(
                latest.form,
                submitted,
                response,
              ),
              // The baseline is what the server accepted, so a field edited
              // during the write stays dirty against it.
              baseline: relayPolicyToWhitelistForm(response),
              loaded: true,
              saveFailed: false,
            })
          },
          'settings',
        )
        if (outcome.executed) {
          set({ saveFailed: false })
          reportedNotifier?.notifySaved()
        }
        return outcome
      } catch (cause) {
        set({ saveFailed: true })
        reportedNotifier?.notifyError(cause)
        return { executed: false }
      }
    },
  }),
)
